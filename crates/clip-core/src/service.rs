//! A running device: sync engine, listener and mDNS, plus pairing. Shared by
//! the tray app and the mobile bindings so they behave the same.

use std::future::Future;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use mdns_sd::ServiceDaemon;
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;

use crate::clipboard::Clipboard;
use crate::discovery::{self, PAIR_SERVICE, SYNC_SERVICE};
use crate::history::History;
use crate::sync::Engine;
use crate::{pairing, Member, State};

pub const DEFAULT_PORT: u16 = 47800;
/// How long a pairing code stays valid.
pub const PAIRING_TIMEOUT: Duration = Duration::from_secs(300);

pub struct Options {
    pub port: u16,
    /// Poll the clipboard and send changes. Off where the platform does not
    /// allow background clipboard reads (Android); the app calls
    /// [`Engine::send_local`] instead.
    pub watch_clipboard: bool,
    pub history: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            watch_clipboard: true,
            history: true,
        }
    }
}

pub struct Service {
    engine: Engine,
    daemons: Vec<ServiceDaemon>,
    tasks: Vec<JoinHandle<()>>,
}

impl Service {
    /// Starts syncing as the device stored at `state_path`.
    pub async fn start(
        state_path: PathBuf,
        clipboard: Box<dyn Clipboard>,
        options: Options,
    ) -> Result<Self> {
        let state = State::load_or_create(&state_path)?;
        let listener = TcpListener::bind(("0.0.0.0", options.port))
            .await
            .with_context(|| {
                format!(
                    "port {} is in use (is clipd already running?)",
                    options.port
                )
            })?;
        let history_path = History::path_for(&state_path);
        let mut engine = Engine::new(state.clone(), Some(state_path), clipboard);
        if options.history {
            engine = engine.with_history(History::load(&history_path)?);
        }
        let advertiser = discovery::advertise(
            SYNC_SERVICE,
            &state.device.id,
            options.port,
            &[("circle", &state.circle_id), ("device", &state.device.id)],
        )?;
        let browser = discovery::browse_circle(
            state.circle_id.clone(),
            state.device.id.clone(),
            engine.peers.clone(),
        )?;
        let mut tasks = Vec::new();
        let e = engine.clone();
        tasks.push(tokio::spawn(async move {
            if let Err(err) = e.serve(listener).await {
                tracing::error!("sync listener stopped: {err:#}");
            }
        }));
        if options.watch_clipboard {
            let e = engine.clone();
            tasks.push(tokio::spawn(async move {
                if let Err(err) = e.watch().await {
                    tracing::error!("clipboard watcher stopped: {err:#}");
                }
            }));
        }
        Ok(Self {
            engine,
            daemons: vec![advertiser, browser],
            tasks,
        })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Shows a new pairing code. Returns the code and a future that resolves
    /// to the device that joined with it (or an error / timeout); the device
    /// is already added to the circle and saved by then.
    pub async fn start_pairing(
        &self,
    ) -> Result<(
        String,
        impl Future<Output = Result<Member>> + Send + 'static,
    )> {
        let engine = self.engine.clone();
        let listener = TcpListener::bind("0.0.0.0:0").await?;
        let port = listener.local_addr()?.port();
        let mut state = engine.state().await;
        let advert = discovery::advertise(PAIR_SERVICE, &state.device.id, port, &[])?;
        let code = pairing::generate_code();
        let code_for_host = code.clone();
        let done = async move {
            let result = tokio::time::timeout(
                PAIRING_TIMEOUT,
                pairing::host(&listener, &code_for_host, &mut state),
            )
            .await;
            let _ = advert.shutdown();
            let device = result.map_err(|_| anyhow::anyhow!("the code expired"))??;
            let joined = device.clone();
            engine
                .update_state(move |s| {
                    // Pairing again is how a removed device is let back in.
                    s.removed.retain(|k| *k != joined.public_key);
                    s.merge_members(std::slice::from_ref(&joined));
                })
                .await?;
            Ok(device)
        };
        Ok((code, done))
    }
}

impl Drop for Service {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
        for daemon in &self.daemons {
            let _ = daemon.shutdown();
        }
    }
}

/// Joins the circle of a nearby device showing `code`, saving the result at
/// `state_path`. Stop any running [`Service`] first and start a new one after,
/// since the circle changes. Returns the number of other devices in the circle.
pub async fn join_nearby(state_path: &std::path::Path, code: &str) -> Result<usize> {
    let addr = discovery::find_pairing_host(Duration::from_secs(10)).await?;
    let stream = TcpStream::connect(addr).await?;
    let mut state = State::load_or_create(state_path)?;
    pairing::join(stream, code, &mut state).await?;
    state.save(state_path)?;
    Ok(state.members.len())
}
