//! The sync engine: watch the local clipboard, push changes to every reachable
//! circle member, and apply changes pushed to us.
//!
//! Each push is its own short connection (XX handshake, Hello both ways, one
//! Clip). That keeps the engine stateless; long-lived connections can come later.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::clipboard::{Clip, Clipboard, WireClip};
use crate::discovery::PeerMap;
use crate::history::History;
use crate::protocol::Message;
use crate::transport::{self, SecureStream};
use crate::State;

pub const POLL_INTERVAL: Duration = Duration::from_millis(500);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone)]
pub struct Engine {
    state: Arc<Mutex<State>>,
    state_path: Option<PathBuf>,
    clipboard: Arc<Mutex<Box<dyn Clipboard>>>,
    /// Hash of the last clip we sent or received, so we don't echo it back.
    last: Arc<Mutex<Option<[u8; 32]>>>,
    pub peers: PeerMap,
    history: Option<Arc<std::sync::Mutex<History>>>,
}

impl Engine {
    pub fn new(state: State, state_path: Option<PathBuf>, clipboard: Box<dyn Clipboard>) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
            state_path,
            clipboard: Arc::new(Mutex::new(clipboard)),
            last: Arc::new(Mutex::new(None)),
            peers: PeerMap::default(),
            history: None,
        }
    }

    /// Records every clip copied here or received into `history`.
    pub fn with_history(mut self, history: History) -> Self {
        self.history = Some(Arc::new(std::sync::Mutex::new(history)));
        self
    }

    fn record(&self, from: &str, clip: &Clip) {
        if let Some(history) = &self.history {
            if let Err(e) = history.lock().unwrap().record(from, clip) {
                tracing::warn!("could not save clipboard history: {e:#}");
            }
        }
    }

    /// Accepts pushes from circle members until the listener fails.
    pub async fn serve(&self, listener: TcpListener) -> Result<()> {
        loop {
            let (stream, peer) = listener.accept().await?;
            let engine = self.clone();
            tokio::spawn(async move {
                if let Err(e) = engine.handle_incoming(stream).await {
                    tracing::warn!(%peer, "rejected incoming sync: {e:#}");
                }
            });
        }
    }

    /// Polls the local clipboard and pushes every new clip to the circle.
    pub async fn watch(&self) -> Result<()> {
        // Don't broadcast whatever was already on the clipboard at startup.
        if let Some(clip) = self.clipboard.lock().await.get() {
            *self.last.lock().await = Some(clip.digest());
        }
        let mut tick = tokio::time::interval(POLL_INTERVAL);
        loop {
            tick.tick().await;
            let Some(clip) = self.clipboard.lock().await.get() else {
                continue;
            };
            let hash = clip.digest();
            {
                let mut last = self.last.lock().await;
                if *last == Some(hash) {
                    continue;
                }
                *last = Some(hash);
            }
            let me = self.state.lock().await.device.name.clone();
            self.record(&me, &clip);
            self.broadcast(&clip).await;
        }
    }

    pub async fn broadcast(&self, clip: &Clip) {
        // Encode once (PNG for images) and share it with every push.
        let clip = clip.clone();
        let wire = match tokio::task::spawn_blocking(move || clip.to_wire()).await {
            Ok(Ok(wire)) => Arc::new(wire),
            Ok(Err(e)) => return tracing::warn!("could not encode clip: {e:#}"),
            Err(e) => return tracing::warn!("clip encoder crashed: {e}"),
        };
        let targets: Vec<(String, Vec<SocketAddr>)> = self
            .peers
            .lock()
            .unwrap()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        for (device, addrs) in targets {
            let engine = self.clone();
            let wire = wire.clone();
            tokio::spawn(async move {
                for addr in addrs {
                    match engine.push_wire(addr, &wire).await {
                        Ok(()) => return tracing::debug!(device, %addr, "pushed clip"),
                        Err(e) => tracing::debug!(device, %addr, "push failed: {e:#}"),
                    }
                }
                tracing::warn!(device, "could not reach device");
            });
        }
    }

    /// Sends `clip` to the device at `addr`.
    pub async fn push(&self, addr: SocketAddr, clip: &Clip) -> Result<()> {
        self.push_wire(addr, &clip.to_wire()?).await
    }

    async fn push_wire(&self, addr: SocketAddr, wire: &WireClip) -> Result<()> {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .context("connect timed out")??;
        let key = self.state.lock().await.private_key_bytes()?;
        let mut chan = transport::sync_initiator(stream, &key).await?;
        self.check_member(&chan).await?;
        self.send_hello(&mut chan).await?;
        self.recv_hello(&mut chan).await?;
        chan.send_json(&Message::Clip { clip: wire.clone() }).await
    }

    async fn handle_incoming(&self, stream: TcpStream) -> Result<()> {
        let key = self.state.lock().await.private_key_bytes()?;
        let mut chan = transport::sync_responder(stream, &key).await?;
        let from = self.check_member(&chan).await?;
        self.recv_hello(&mut chan).await?;
        self.send_hello(&mut chan).await?;
        let Message::Clip { clip } = chan.recv_json().await? else {
            bail!("expected clip message");
        };
        let clip = tokio::task::spawn_blocking(move || Clip::from_wire(clip)).await??;
        tracing::info!(from, bytes = clip.len(), "received clip");
        self.record(&from, &clip);
        let mut last = self.last.lock().await;
        let mut clipboard = self.clipboard.lock().await;
        let digest = clip.digest();
        // Already on our clipboard (e.g. both sides copied it at once): writing it
        // again is redundant and can fail on macOS while another write is in flight.
        if *last == Some(digest) || clipboard.get().map(|c| c.digest()) == Some(digest) {
            *last = Some(digest);
            return Ok(());
        }
        clipboard.set(&clip)?;
        // Remember what the OS actually stored, which may differ from what we
        // set (e.g. color conversion of images). Otherwise the watcher would
        // see a "new" clip and send it back, and the two could bounce forever.
        *last = Some(clipboard.get().unwrap_or(clip).digest());
        Ok(())
    }

    /// Returns the peer's name if its static key belongs to our circle.
    async fn check_member(&self, chan: &SecureStream) -> Result<String> {
        let key = chan.remote_static().context("peer sent no static key")?;
        let state = self.state.lock().await;
        match state.member_by_key(key) {
            Some(m) => Ok(m.name.clone()),
            None => bail!("peer is not in our circle"),
        }
    }

    async fn send_hello(&self, chan: &mut SecureStream) -> Result<()> {
        let msg = {
            let state = self.state.lock().await;
            Message::Hello {
                circle_id: state.circle_id.clone(),
                members: state.all_members(),
                removed: state.removed.clone(),
            }
        };
        chan.send_json(&msg).await
    }

    /// Learns about members the peer knows and we don't (e.g. a third device
    /// that paired with the peer while we were offline).
    async fn recv_hello(&self, chan: &mut SecureStream) -> Result<()> {
        let Message::Hello {
            circle_id,
            members,
            removed,
        } = chan.recv_json().await?
        else {
            bail!("expected hello message");
        };
        let mut state = self.state.lock().await;
        if circle_id != state.circle_id {
            bail!("peer is in a different circle");
        }
        // Removals first, so a member removed elsewhere isn't re-added.
        let removed_any = state.merge_removed(&removed);
        let added_any = state.merge_members(&members);
        if removed_any || added_any {
            tracing::info!(removed_any, added_any, "circle membership updated");
            if let Some(path) = &self.state_path {
                state.save(path)?;
            }
        }
        Ok(())
    }

    pub async fn state(&self) -> State {
        self.state.lock().await.clone()
    }
}
