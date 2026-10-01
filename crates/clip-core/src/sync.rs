//! The sync engine: watch the local clipboard, push changes to every reachable
//! circle member, and apply changes pushed to us.
//!
//! Each push is its own short connection (XX handshake, Hello both ways, one
//! Clip, and for files their contents). That keeps the engine stateless;
//! long-lived connections can come later.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{bail, Context, Result};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

use crate::clipboard::{Clip, Clipboard, FileRef, WireClip};
use crate::discovery::PeerMap;
use crate::history::{Entry, History};
use crate::protocol::Message;
use crate::transfer;
use crate::transport::{self, SecureStream};
use crate::State;

pub const POLL_INTERVAL: Duration = Duration::from_millis(500);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// What a connection check found for one circle member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// Answered over an encrypted connection and accepted us as a member.
    Ok { addr: SocketAddr, millis: u64 },
    /// Not announced on this network.
    NotFound,
    /// Announced, but no address worked; this is the last error.
    Failed { error: String },
}

impl Reach {
    /// A short, plain-language next step for a failed check.
    pub fn hint(&self) -> Option<&'static str> {
        match self {
            Reach::Ok { .. } => None,
            Reach::NotFound => Some(
                "Not seen on this network. Check that the app is running there and that \
                 both devices are on the same Wi-Fi. Some routers block device discovery \
                 (client or AP isolation).",
            ),
            Reach::Failed { error } if error.contains("timed out") => Some(
                "The connection timed out. A firewall on that device may be \
                 blocking the sync port.",
            ),
            Reach::Failed { error } if error.contains("refused") => {
                Some("Nothing answered on the sync port. The app there may not be running.")
            }
            Reach::Failed { error }
                if error.contains("removed this one") || error.contains("not in our circle") =>
            {
                Some("That device no longer accepts this one. Pair the two again.")
            }
            Reach::Failed { .. } => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct MemberReach {
    pub id: String,
    pub name: String,
    /// Addresses it was announced at.
    pub addrs: Vec<SocketAddr>,
    pub reach: Reach,
}

#[derive(Clone)]
pub struct Engine {
    state: Arc<Mutex<State>>,
    state_path: Option<PathBuf>,
    clipboard: Arc<Mutex<Box<dyn Clipboard>>>,
    /// Hash of the last clip we sent or received, so we don't echo it back.
    last: Arc<Mutex<Option<[u8; 32]>>>,
    pub peers: PeerMap,
    history: Option<Arc<std::sync::Mutex<History>>>,
    /// While set, nothing is sent and incoming clips are refused.
    paused: Arc<AtomicBool>,
    /// Received files are written to a new folder in here.
    received_dir: PathBuf,
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
            paused: Arc::new(AtomicBool::new(false)),
            received_dir: transfer::default_received_dir(),
        }
    }

    /// Keeps received files in `dir` instead of the temp directory.
    pub fn with_received_dir(mut self, dir: PathBuf) -> Self {
        self.received_dir = dir;
        self
    }

    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Relaxed);
    }

    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Relaxed)
    }

    /// Recorded clips, newest first (empty without a history).
    pub fn history_entries(&self) -> Vec<Entry> {
        match &self.history {
            Some(h) => h.lock().unwrap().entries().cloned().collect(),
            None => Vec::new(),
        }
    }

    /// Changes the circle state and saves it.
    pub async fn update_state<R>(&self, f: impl FnOnce(&mut State) -> R) -> Result<R> {
        let mut state = self.state.lock().await;
        let r = f(&mut state);
        if let Some(path) = &self.state_path {
            state.save(path)?;
        }
        Ok(r)
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
            // Copies made while paused are marked seen above, so they are not
            // sent later when syncing resumes.
            if self.is_paused() {
                continue;
            }
            // Files we (or another circle device on this machine) received are
            // already in the circle. Sending them on would bounce them between
            // devices that share one clipboard, rewriting them every poll.
            if self.is_received(&clip) {
                continue;
            }
            let me = self.state.lock().await.device.name.clone();
            self.record(&me, &clip);
            self.broadcast(&clip).await;
        }
    }

    /// Whether `clip` is files that were written to the received-files folder.
    fn is_received(&self, clip: &Clip) -> bool {
        let Clip::Files(files) = clip else {
            return false;
        };
        // Resolve symlinks: on macOS the clipboard reports /private/var/...
        // for a temp dir that is /var/... in $TMPDIR.
        let Ok(root) = self.received_dir.canonicalize() else {
            return false;
        };
        !files.is_empty()
            && files
                .iter()
                .all(|f| f.path.canonicalize().is_ok_and(|p| p.starts_with(&root)))
    }

    /// Sends a clip copied on this device, for platforms where the app hands
    /// over copies itself instead of [`Engine::watch`] polling for them.
    pub async fn send_local(&self, clip: Clip) {
        *self.last.lock().await = Some(clip.digest());
        if self.is_paused() {
            return;
        }
        let me = self.state.lock().await.device.name.clone();
        self.record(&me, &clip);
        self.broadcast(&clip).await;
    }

    pub async fn broadcast(&self, clip: &Clip) {
        // Encode once (PNG for images) and share it with every push.
        let files = Arc::new(files_of(clip).to_vec());
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
            let files = files.clone();
            tokio::spawn(async move {
                for addr in addrs {
                    match engine.push_wire(addr, &wire, &files).await {
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
        self.push_wire(addr, &clip.to_wire()?, files_of(clip)).await
    }

    async fn push_wire(&self, addr: SocketAddr, wire: &WireClip, files: &[FileRef]) -> Result<()> {
        let (mut chan, _) = self.connect(addr).await?;
        chan.send_json(&Message::Clip { clip: wire.clone() })
            .await?;
        if files.is_empty() {
            return Ok(());
        }
        transfer::send(&mut chan, files).await?;
        match tokio::time::timeout(transfer::STALL_TIMEOUT, chan.recv_json()).await {
            Ok(Ok(Message::Received)) => Ok(()),
            Ok(Ok(_)) => bail!("expected received message"),
            Ok(Err(e)) => Err(e.context("files were not received")),
            Err(_) => bail!("no answer after sending the files"),
        }
    }

    /// Opens a sync connection to `addr`: handshake, membership check and
    /// Hello both ways. Returns the channel and the peer's name.
    async fn connect(&self, addr: SocketAddr) -> Result<(SecureStream, String)> {
        let stream = tokio::time::timeout(CONNECT_TIMEOUT, TcpStream::connect(addr))
            .await
            .context("connect timed out")??;
        let key = self.state.lock().await.private_key_bytes()?;
        let mut chan = transport::sync_initiator(stream, &key).await?;
        let name = self.check_member(&chan).await?;
        self.send_hello(&mut chan).await?;
        self.recv_hello(&mut chan)
            .await
            .context("no hello back (has that device removed this one?)")?;
        Ok((chan, name))
    }

    /// Checks that the device at `addr` is a circle member that accepts us.
    /// Returns its name and how long the check took.
    pub async fn ping(&self, addr: SocketAddr) -> Result<(String, Duration)> {
        let start = Instant::now();
        let (mut chan, name) = self.connect(addr).await?;
        chan.send_json(&Message::Ping).await?;
        Ok((name, start.elapsed()))
    }

    /// Checks every circle member at the addresses discovery has for it.
    pub async fn check_members(&self) -> Vec<MemberReach> {
        let members = self.state.lock().await.members.clone();
        let peers = self.peers.lock().unwrap().clone();
        let mut checks = tokio::task::JoinSet::new();
        for (index, member) in members.into_iter().enumerate() {
            let addrs = peers.get(&member.id).cloned().unwrap_or_default();
            let engine = self.clone();
            checks.spawn(async move {
                let reach = if addrs.is_empty() {
                    Reach::NotFound
                } else {
                    let mut last = String::new();
                    let mut found = None;
                    for &addr in &addrs {
                        match engine.ping(addr).await {
                            Ok((_, took)) => {
                                found = Some(Reach::Ok {
                                    addr,
                                    millis: took.as_millis() as u64,
                                });
                                break;
                            }
                            Err(e) => last = format!("{e:#}"),
                        }
                    }
                    found.unwrap_or(Reach::Failed { error: last })
                };
                (
                    index,
                    MemberReach {
                        id: member.id,
                        name: member.name,
                        addrs,
                        reach,
                    },
                )
            });
        }
        let mut out: Vec<(usize, MemberReach)> = checks.join_all().await;
        out.sort_by_key(|(i, _)| *i);
        out.into_iter().map(|(_, r)| r).collect()
    }

    async fn handle_incoming(&self, stream: TcpStream) -> Result<()> {
        let key = self.state.lock().await.private_key_bytes()?;
        let mut chan = transport::sync_responder(stream, &key).await?;
        let from = self.check_member(&chan).await?;
        self.recv_hello(&mut chan).await?;
        self.send_hello(&mut chan).await?;
        let clip = match chan.recv_json().await? {
            Message::Clip { clip } => clip,
            Message::Ping => {
                tracing::debug!(from, "connection check");
                return Ok(());
            }
            _ => bail!("expected clip message"),
        };
        if self.is_paused() {
            bail!("syncing is paused");
        }
        // Files can take a while; what was copied here in the meantime wins.
        let mut copied_before = None;
        let clip = match clip {
            WireClip::Files { files } => {
                copied_before = Some(*self.last.lock().await);
                let files = transfer::receive(&mut chan, &files, &self.received_dir).await?;
                chan.send_json(&Message::Received).await?;
                Clip::Files(files)
            }
            wire => tokio::task::spawn_blocking(move || Clip::from_wire(wire)).await??,
        };
        tracing::info!(from, bytes = clip.len(), "received clip");
        self.record(&from, &clip);
        let mut last = self.last.lock().await;
        if copied_before.is_some_and(|before| before != *last) {
            tracing::info!("not pasting received files: something newer was copied");
            return Ok(());
        }
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

fn files_of(clip: &Clip) -> &[FileRef] {
    match clip {
        Clip::Files(files) => files,
        _ => &[],
    }
}
