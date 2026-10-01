//! Bindings for the mobile apps. The app owns one [`Node`]: it starts syncing,
//! hands over what the user copies with [`Node::send_text`] or
//! [`Node::send_image`], and gets clips from the circle through
//! [`ClipListener::on_clip`] and [`ClipListener::on_image`].
//!
//! Every method blocks until done, so call them off the UI thread.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use clip_core::clipboard::{Clip, Clipboard};
use clip_core::service::{self, Options, Service};
use clip_core::State;

uniffi::setup_scaffolding!();

// Flat so Kotlin gets the text as the exception message; a `message` field
// would clash with Throwable.message.
#[derive(Debug, thiserror::Error, uniffi::Error)]
#[uniffi(flat_error)]
pub enum NodeError {
    #[error("{message}")]
    Failed { message: String },
}

impl From<anyhow::Error> for NodeError {
    fn from(e: anyhow::Error) -> Self {
        NodeError::Failed {
            message: format!("{e:#}"),
        }
    }
}

type Result<T> = std::result::Result<T, NodeError>;

/// Implemented by the app.
#[uniffi::export(with_foreign)]
pub trait ClipListener: Send + Sync {
    /// Text arrived from the circle; put it on the device clipboard.
    fn on_clip(&self, text: String);
    /// An image arrived from the circle, as PNG bytes.
    fn on_image(&self, png: Vec<u8>);
    /// A device joined with the pairing code being shown.
    fn on_paired(&self, device_name: String);
    /// Showing a pairing code ended without a device joining.
    fn on_pairing_failed(&self, message: String);
}

#[derive(uniffi::Record)]
pub struct Device {
    pub id: String,
    pub name: String,
}

/// The engine's view of the clipboard: whatever the app last handed over or
/// was told to set. Mobile apps can't read the clipboard in the background.
struct AppClipboard {
    listener: Arc<dyn ClipListener>,
    current: Option<Clip>,
}

impl Clipboard for AppClipboard {
    fn get(&mut self) -> Option<Clip> {
        self.current.clone()
    }

    fn set(&mut self, clip: &Clip) -> anyhow::Result<()> {
        match clip {
            Clip::Text(text) => self.listener.on_clip(text.clone()),
            Clip::Image { .. } => self.listener.on_image(clip.to_png()?),
            // Files on mobile come later.
            Clip::Files(_) => tracing::info!("ignoring copied files on mobile"),
        }
        self.current = Some(clip.clone());
        Ok(())
    }
}

#[derive(uniffi::Object)]
pub struct Node {
    runtime: tokio::runtime::Runtime,
    state_path: PathBuf,
    listener: Arc<dyn ClipListener>,
    service: Mutex<Option<Service>>,
}

#[uniffi::export]
impl Node {
    /// `data_dir` is app-private storage; `device_name` is used the first time only.
    #[uniffi::constructor]
    pub fn new(
        data_dir: String,
        device_name: String,
        listener: Arc<dyn ClipListener>,
    ) -> Result<Arc<Self>> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .build()
            .map_err(anyhow::Error::from)?;
        let state_path = PathBuf::from(data_dir).join("state.json");
        if !state_path.exists() {
            State::generate(device_name)?.save(&state_path)?;
        }
        Ok(Arc::new(Self {
            runtime,
            state_path,
            listener,
            service: Mutex::new(None),
        }))
    }

    /// Starts (or restarts) syncing on the local network.
    pub fn start(&self) -> Result<()> {
        let mut service = self.service.lock().unwrap();
        *service = None;
        let clipboard = Box::new(AppClipboard {
            listener: self.listener.clone(),
            current: None,
        });
        let options = Options {
            watch_clipboard: false,
            ..Options::default()
        };
        let started =
            self.runtime
                .block_on(Service::start(self.state_path.clone(), clipboard, options))?;
        *service = Some(started);
        Ok(())
    }

    pub fn stop(&self) {
        *self.service.lock().unwrap() = None;
    }

    pub fn is_running(&self) -> bool {
        self.service.lock().unwrap().is_some()
    }

    pub fn device_name(&self) -> Result<String> {
        Ok(State::load_or_create(&self.state_path)?.device.name)
    }

    pub fn members(&self) -> Result<Vec<Device>> {
        let state = match self.service.lock().unwrap().as_ref() {
            Some(s) => self.runtime.block_on(s.engine().state()),
            None => State::load_or_create(&self.state_path)?,
        };
        Ok(state
            .members
            .into_iter()
            .map(|m| Device {
                id: m.id,
                name: m.name,
            })
            .collect())
    }

    /// Sends text the user copied on this device to the circle.
    pub fn send_text(&self, text: String) -> Result<()> {
        let engine = self.engine()?;
        self.runtime.block_on(engine.send_local(Clip::Text(text)));
        Ok(())
    }

    /// Sends an image the user copied or shared, as PNG bytes.
    pub fn send_image(&self, png: Vec<u8>) -> Result<()> {
        let clip = Clip::from_png(&png)?;
        let engine = self.engine()?;
        self.runtime.block_on(engine.send_local(clip));
        Ok(())
    }

    /// Shows a new pairing code; the outcome arrives through the listener.
    pub fn start_pairing(&self) -> Result<String> {
        let guard = self.service.lock().unwrap();
        let service = guard.as_ref().ok_or_else(not_running)?;
        let (code, done) = self.runtime.block_on(service.start_pairing())?;
        let listener = self.listener.clone();
        self.runtime.spawn(async move {
            match done.await {
                Ok(device) => listener.on_paired(device.name),
                Err(e) => listener.on_pairing_failed(format!("{e:#}")),
            }
        });
        Ok(code)
    }

    /// Joins the circle of a nearby device showing `code`, then restarts
    /// syncing. Returns the number of other devices in the circle.
    pub fn join(&self, code: String) -> Result<u32> {
        self.stop();
        let joined = self
            .runtime
            .block_on(service::join_nearby(&self.state_path, &code));
        self.start()?;
        Ok(joined? as u32)
    }

    pub fn remove_device(&self, id: String) -> Result<()> {
        let engine = self.engine()?;
        self.runtime
            .block_on(engine.update_state(|s| s.remove_member(&id)))??;
        Ok(())
    }

    pub fn set_paused(&self, paused: bool) -> Result<()> {
        self.engine()?.set_paused(paused);
        Ok(())
    }

    pub fn is_paused(&self) -> bool {
        self.engine().map(|e| e.is_paused()).unwrap_or(false)
    }
}

impl Node {
    fn engine(&self) -> Result<clip_core::sync::Engine> {
        let guard = self.service.lock().unwrap();
        Ok(guard.as_ref().ok_or_else(not_running)?.engine().clone())
    }
}

fn not_running() -> NodeError {
    NodeError::Failed {
        message: "syncing is not running".into(),
    }
}
