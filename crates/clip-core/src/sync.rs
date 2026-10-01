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
use sha2::{Digest, Sha256};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::clipboard::Clipboard;
use crate::discovery::PeerMap;
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
    /// Hash of the last text we sent or received, so we don't echo it back.
    last: Arc<Mutex<Option<[u8; 32]>>>,
    pub peers: PeerMap,
}

fn digest(text: &str) -> [u8; 32] {
    Sha256::digest(text.as_bytes()).into()
}

impl Engine {
    pub fn new(state: State, state_path: Option<PathBuf>, clipboard: Box<dyn Clipboard>) -> Self {
        Self {
            state: Arc::new(Mutex::new(state)),
            state_path,
            clipboard: Arc::new(Mutex::new(clipboard)),
            last: Arc::new(Mutex::new(None)),
            peers: PeerMap::default(),
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

    /// Polls the local clipboard and pushes every new text to the circle.
    pub async fn watch(&self) -> Result<()> {
        // Don't broadcast whatever was already on the clipboard at startup.
        if let Some(text) = self.clipboard.lock().await.get_text() {
            *self.last.lock().await = Some(digest(&text));
        }
        let mut tick = tokio::time::interval(POLL_INTERVAL);
        loop {
            tick.tick().await;
            let Some(text) = self.clipboard.lock().await.get_text() else { continue };
            let hash = digest(&text);
            {
                let mut last = self.last.lock().await;
                if *last == Some(hash) {
                    continue;
                }
                *last = Some(hash);
            }
            self.broadcast(&text).await;
        }
    }

    pub async fn broadcast(&self, text: &str) {
        let targets: Vec<(String, Vec<SocketAddr>)> =
            self.peers.lock().unwrap().iter().map(|(k, v)| (k.clone(), v.clone())).collect();
        for (device, addrs) in targets {
            let engine = self.clone();
            let text = text.to_owned();
            tokio::spawn(async move {
                for addr in addrs {
                    match engine.push(addr, &text).await {
                        Ok(()) => return tracing::debug!(device, %addr, "pushed clip"),
                        Err(e) => tracing::debug!(device, %addr, "push failed: {e:#}"),
                    }
                }
                tracing::warn!(device, "could not reach device");
            });
        }
    }

    /// Sends `text` to the device at `addr`.
    pub async fn push(&self, addr: SocketAddr, text: &str) -> Result<()> {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .context("connect timed out")??;
        let key = self.state.lock().await.private_key_bytes()?;
        let mut chan = transport::sync_initiator(stream, &key).await?;
        self.check_member(&chan).await?;
        self.send_hello(&mut chan).await?;
        self.recv_hello(&mut chan).await?;
        chan.send_json(&Message::Clip { text: text.to_owned() }).await
    }

    async fn handle_incoming(&self, stream: TcpStream) -> Result<()> {
        let key = self.state.lock().await.private_key_bytes()?;
        let mut chan = transport::sync_responder(stream, &key).await?;
        let from = self.check_member(&chan).await?;
        self.recv_hello(&mut chan).await?;
        self.send_hello(&mut chan).await?;
        let Message::Clip { text } = chan.recv_json().await? else {
            bail!("expected clip message");
        };
        tracing::info!(from, bytes = text.len(), "received clip");
        *self.last.lock().await = Some(digest(&text));
        self.clipboard.lock().await.set_text(&text)?;
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
            Message::Hello { circle_id: state.circle_id.clone(), members: state.all_members() }
        };
        chan.send_json(&msg).await
    }

    /// Learns about members the peer knows and we don't (e.g. a third device
    /// that paired with the peer while we were offline).
    async fn recv_hello(&self, chan: &mut SecureStream) -> Result<()> {
        let Message::Hello { circle_id, members } = chan.recv_json().await? else {
            bail!("expected hello message");
        };
        let mut state = self.state.lock().await;
        if circle_id != state.circle_id {
            bail!("peer is in a different circle");
        }
        if state.merge_members(&members) {
            tracing::info!("learned new circle members");
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
