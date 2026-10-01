//! Universal Clipboard tray app: runs the sync engine in the background and
//! offers a small window to pair devices, manage the circle and see history.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use clip_core::clipboard::{Clip, Clipboard, SystemClipboard};
use clip_core::history::Content;
use clip_core::service::{self, Options, Service};
use clip_core::sync::Reach;
use clip_core::State;
use serde::Serialize;
use tauri::menu::{CheckMenuItem, Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter, Listener, Manager, WindowEvent};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};
use tokio::sync::Mutex;

struct App {
    state_path: PathBuf,
    service: Mutex<Option<Service>>,
}

/// Commands report errors to the window as plain strings.
type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

impl App {
    async fn engine(&self) -> CmdResult<clip_core::sync::Engine> {
        self.service
            .lock()
            .await
            .as_ref()
            .map(|s| s.engine().clone())
            .ok_or_else(|| "syncing is not running".to_owned())
    }

    async fn restart(&self) -> anyhow::Result<()> {
        let mut service = self.service.lock().await;
        // Stop the old one first so the port is free.
        *service = None;
        let clipboard = Box::new(SystemClipboard::new()?);
        *service =
            Some(Service::start(self.state_path.clone(), clipboard, Options::default()).await?);
        Ok(())
    }
}

#[derive(Serialize)]
struct DeviceView {
    id: String,
    name: String,
    /// Announced on this network right now.
    online: bool,
}

#[derive(Serialize)]
struct StatusView {
    device: DeviceView,
    members: Vec<DeviceView>,
    paused: bool,
}

#[tauri::command]
async fn status(app: tauri::State<'_, App>) -> CmdResult<StatusView> {
    let engine = app.engine().await?;
    let state = engine.state().await;
    let peers = engine.peers.lock().unwrap().clone();
    Ok(StatusView {
        device: DeviceView {
            id: state.device.id,
            name: state.device.name,
            online: true,
        },
        members: state
            .members
            .into_iter()
            .map(|m| DeviceView {
                online: peers.contains_key(&m.id),
                id: m.id,
                name: m.name,
            })
            .collect(),
        paused: engine.is_paused(),
    })
}

#[derive(Serialize)]
struct CheckView {
    id: String,
    ok: bool,
    /// One line on what the check found, plus a next step when it failed.
    detail: String,
}

/// Connects to every device in the circle and reports what worked.
#[tauri::command]
async fn check_devices(app: tauri::State<'_, App>) -> CmdResult<Vec<CheckView>> {
    let engine = app.engine().await?;
    Ok(engine
        .check_members()
        .await
        .into_iter()
        .map(|m| {
            let found = match &m.reach {
                Reach::Ok { millis, .. } => format!("Connected ({millis} ms)"),
                Reach::NotFound => String::new(),
                Reach::Failed { error } => format!("Could not connect: {error}"),
            };
            let detail = match (&m.reach, m.reach.hint()) {
                (Reach::NotFound, Some(hint)) => hint.to_owned(),
                (_, Some(hint)) => format!("{found}. {hint}"),
                (_, None) => found,
            };
            CheckView {
                ok: matches!(m.reach, Reach::Ok { .. }),
                detail,
                id: m.id,
            }
        })
        .collect())
}

/// Shows a code and waits in the background for one device to join with it.
/// The window hears back through the `paired` / `pairing-failed` events.
#[tauri::command]
async fn start_pairing(handle: AppHandle, app: tauri::State<'_, App>) -> CmdResult<String> {
    let guard = app.service.lock().await;
    let service = guard.as_ref().ok_or("syncing is not running")?;
    let (code, done) = service.start_pairing().await.map_err(err)?;
    tauri::async_runtime::spawn(async move {
        let _ = match done.await {
            Ok(device) => handle.emit("paired", device.name),
            Err(e) => handle.emit("pairing-failed", format!("{e:#}")),
        };
    });
    Ok(code)
}

/// Joins the circle of a nearby device that is showing `code`.
#[tauri::command]
async fn join(code: String, app: tauri::State<'_, App>) -> CmdResult<usize> {
    // Stop syncing while the circle changes, then restart in the new one.
    *app.service.lock().await = None;
    let joined = service::join_nearby(&app.state_path, &code).await;
    app.restart().await.map_err(|e| format!("{e:#}"))?;
    joined.map_err(|e| format!("{e:#}"))
}

#[tauri::command]
async fn remove_device(id: String, app: tauri::State<'_, App>) -> CmdResult<()> {
    let engine = app.engine().await?;
    engine
        .update_state(|s| s.remove_member(&id))
        .await
        .map_err(err)?
        .map_err(err)?;
    Ok(())
}

#[tauri::command]
async fn set_paused(paused: bool, handle: AppHandle, app: tauri::State<'_, App>) -> CmdResult<()> {
    app.engine().await?.set_paused(paused);
    let _ = handle.emit("paused-changed", paused);
    Ok(())
}

#[derive(Serialize)]
struct HistoryView {
    from: String,
    secs_ago: u64,
    preview: String,
    can_copy: bool,
}

#[tauri::command]
async fn history(app: tauri::State<'_, App>) -> CmdResult<Vec<HistoryView>> {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let entries = app.engine().await?.history_entries();
    Ok(entries
        .into_iter()
        .map(|e| {
            let can_copy = e.full_text().is_some();
            let preview = match &e.content {
                Content::Text { text, .. } => text.chars().take(200).collect(),
                Content::Image { width, height } => format!("Image {width}×{height}"),
                Content::Files { names } if names.len() == 1 => format!("File: {}", names[0]),
                Content::Files { names } => format!("{} files: {}", names.len(), names.join(", ")),
            };
            HistoryView {
                from: e.from,
                secs_ago: now.saturating_sub(e.at),
                preview,
                can_copy,
            }
        })
        .collect())
}

/// Puts history entry `index` (0 = newest) back on the clipboard.
#[tauri::command]
async fn copy_history(index: usize, app: tauri::State<'_, App>) -> CmdResult<()> {
    let entries = app.engine().await?.history_entries();
    let text = entries
        .get(index)
        .and_then(|e| e.full_text())
        .ok_or("only complete text entries can be copied back")?;
    SystemClipboard::new()
        .map_err(err)?
        .set(&Clip::Text(text.to_owned()))
        .map_err(err)
}

/// Whether the app starts when the user logs in.
#[tauri::command]
fn autostart(handle: AppHandle) -> CmdResult<bool> {
    handle.autolaunch().is_enabled().map_err(err)
}

#[tauri::command]
fn set_autostart(enabled: bool, handle: AppHandle) -> CmdResult<()> {
    apply_autostart(&handle, enabled)
}

fn apply_autostart(handle: &AppHandle, enabled: bool) -> CmdResult<()> {
    let launcher = handle.autolaunch();
    if enabled {
        launcher.enable()
    } else {
        launcher.disable()
    }
    .map_err(err)?;
    let _ = handle.emit("autostart-changed", enabled);
    Ok(())
}

fn show_window(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.set_focus();
    }
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,mdns_sd=warn".into()),
        )
        .init();

    let state_path = std::env::var_os("CLIPD_STATE")
        .map(PathBuf::from)
        .unwrap_or_else(|| State::default_path().expect("no config directory"));

    tauri::Builder::default()
        .plugin(tauri_plugin_autostart::init(
            MacosLauncher::LaunchAgent,
            None,
        ))
        .manage(App {
            state_path,
            service: Mutex::new(None),
        })
        .invoke_handler(tauri::generate_handler![
            status,
            start_pairing,
            join,
            remove_device,
            set_paused,
            history,
            copy_history,
            autostart,
            set_autostart,
            check_devices
        ])
        .setup(|app| {
            // A menu bar app on macOS: no Dock icon.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let open =
                MenuItem::with_id(app, "open", "Open Universal Clipboard", true, None::<&str>)?;
            let pause = MenuItem::with_id(app, "pause", "Pause syncing", true, None::<&str>)?;
            let at_login = CheckMenuItem::with_id(
                app,
                "autostart",
                "Start at login",
                true,
                app.autolaunch().is_enabled().unwrap_or(false),
                None::<&str>,
            )?;
            let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&open, &pause, &at_login, &quit])?;

            let pause_item = pause.clone();
            let at_login_item = at_login.clone();
            TrayIconBuilder::with_id("main")
                .icon(app.default_window_icon().cloned().expect("app icon"))
                .tooltip("Universal Clipboard")
                .menu(&menu)
                .show_menu_on_left_click(false)
                .on_menu_event(move |app, event| match event.id.as_ref() {
                    "open" => show_window(app),
                    "pause" => {
                        let app = app.clone();
                        let item = pause_item.clone();
                        tauri::async_runtime::spawn(async move {
                            let Ok(engine) = app.state::<App>().engine().await else {
                                return;
                            };
                            let paused = !engine.is_paused();
                            engine.set_paused(paused);
                            let _ = item.set_text(if paused {
                                "Resume syncing"
                            } else {
                                "Pause syncing"
                            });
                            let _ = app.emit("paused-changed", paused);
                        });
                    }
                    "autostart" => {
                        // The check mark has already flipped; make it so.
                        let enabled = at_login_item.is_checked().unwrap_or(false);
                        if let Err(e) = apply_autostart(app, enabled) {
                            tracing::warn!("could not change start at login: {e}");
                            let _ = at_login_item.set_checked(!enabled);
                        }
                    }
                    "quit" => app.exit(0),
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        show_window(tray.app_handle());
                    }
                })
                .build(app)?;

            // Keep the paused state and the tray menu in step when the window toggles it.
            let item = pause.clone();
            app.listen("paused-changed", move |event| {
                let paused = event.payload() == "true";
                let _ = item.set_text(if paused {
                    "Resume syncing"
                } else {
                    "Pause syncing"
                });
            });

            app.listen("autostart-changed", move |event| {
                let _ = at_login.set_checked(event.payload() == "true");
            });

            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = handle.state::<App>().restart().await {
                    tracing::error!("could not start syncing: {e:#}");
                    let _ = handle.emit("service-error", format!("{e:#}"));
                    show_window(&handle);
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            // Closing the window keeps the app running in the tray.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
