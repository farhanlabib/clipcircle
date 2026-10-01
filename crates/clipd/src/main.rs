//! `clipd`: command-line daemon for the universal clipboard (milestone 1).

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use clip_core::clipboard::{Clip, Clipboard, SystemClipboard};
use clip_core::discovery::{self, PAIR_SERVICE, SYNC_SERVICE};
use clip_core::history::{Content, History};
use clip_core::sync::Engine;
use clip_core::{pairing, State};
use tokio::net::{TcpListener, TcpStream};

#[derive(Parser)]
#[command(
    version,
    about = "Share your clipboard with the devices in your circle"
)]
struct Cli {
    /// Where this device's identity and circle are stored.
    #[arg(long, global = true)]
    state: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Show this device and the devices in its circle.
    Devices,
    /// Show a code that another device can use to join this circle.
    Pair,
    /// Join the circle of a device that is showing a pairing code.
    Join {
        code: String,
        /// Connect to this address instead of finding the device with mDNS.
        #[arg(long)]
        addr: Option<SocketAddr>,
    },
    /// Remove a device from the circle, by name or id. Other members learn
    /// about it the next time they sync with this device.
    Remove { device: String },
    /// Sync the clipboard with the circle until stopped.
    Run {
        #[arg(long, default_value_t = 47800)]
        port: u16,
        /// Don't keep a history of copied text on this device.
        #[arg(long)]
        no_history: bool,
    },
    /// Show recently copied and received clips, newest first.
    History {
        /// How many entries to show.
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Put entry N (as numbered in the list) back on the clipboard; a
        /// running `clipd run` then sends it to the circle.
        #[arg(long, value_name = "N")]
        copy: Option<usize>,
        /// Delete the history on this device.
        #[arg(long)]
        clear: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "info,mdns_sd=warn".into()),
        )
        .init();

    let cli = Cli::parse();
    let path = match cli.state {
        Some(p) => p,
        None => State::default_path()?,
    };
    let mut state = State::load_or_create(&path)?;

    match cli.command {
        Command::Devices => {
            println!("This device: {} ({})", state.device.name, state.device.id);
            println!("Circle: {}", state.circle_id);
            if state.members.is_empty() {
                println!("No other devices yet. Run `clipd pair` here and `clipd join <code>` on another device.");
            }
            for m in &state.members {
                println!("  - {} ({})", m.name, m.id);
            }
        }
        Command::Pair => {
            let listener = TcpListener::bind("0.0.0.0:0").await?;
            let port = listener.local_addr()?.port();
            let _ad = discovery::advertise(PAIR_SERVICE, &state.device.id, port, &[])?;
            let code = pairing::generate_code();
            println!("Pairing code: {code}");
            println!("On the other device run: clipd join {code}   (listening on port {port})");
            let device = pairing::host(&listener, &code, &mut state).await?;
            state.save(&path)?;
            println!("{} joined the circle.", device.name);
        }
        Command::Join { code, addr } => {
            let addr = match addr {
                Some(a) => a,
                None => discovery::find_pairing_host(Duration::from_secs(10)).await?,
            };
            let stream = TcpStream::connect(addr).await?;
            pairing::join(stream, &code, &mut state).await?;
            state.save(&path)?;
            println!(
                "Joined the circle with {} other device(s).",
                state.members.len()
            );
        }
        Command::Remove { device } => {
            let removed = state.remove_member(&device)?;
            state.save(&path)?;
            println!("Removed {} ({}) from the circle.", removed.name, removed.id);
        }
        Command::History { limit, copy, clear } => {
            let mut history = History::load(&History::path_for(&path))?;
            if clear {
                history.clear()?;
                println!("History cleared.");
            } else if let Some(n) = copy {
                let entry = history
                    .entries()
                    .nth(n.wrapping_sub(1))
                    .context("no such entry")?;
                let text = entry
                    .full_text()
                    .context("only complete text entries can be copied back")?;
                SystemClipboard::new()?.set(&Clip::Text(text.to_owned()))?;
                println!("Copied entry {n} to the clipboard.");
            } else {
                print_history(&history, limit);
            }
        }
        Command::Run { port, no_history } => {
            let listener = TcpListener::bind(("0.0.0.0", port)).await?;
            let _ad = discovery::advertise(
                SYNC_SERVICE,
                &state.device.id,
                port,
                &[("circle", &state.circle_id), ("device", &state.device.id)],
            )?;
            let history_path = History::path_for(&path);
            let mut engine =
                Engine::new(state.clone(), Some(path), Box::new(SystemClipboard::new()?));
            if !no_history {
                engine = engine.with_history(History::load(&history_path)?);
            }
            let _browser = discovery::browse_circle(
                state.circle_id.clone(),
                state.device.id.clone(),
                engine.peers.clone(),
            )?;
            println!(
                "Syncing clipboard as {} on port {port}. Ctrl+C to stop.",
                state.device.name
            );
            tokio::select! {
                r = engine.serve(listener) => r?,
                r = engine.watch() => r?,
                _ = tokio::signal::ctrl_c() => {}
            }
        }
    }
    Ok(())
}

fn print_history(history: &History, limit: usize) {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let mut empty = true;
    for (i, e) in history.entries().take(limit).enumerate() {
        empty = false;
        let preview = match &e.content {
            Content::Text { text, truncated } => {
                let line = text.lines().next().unwrap_or("");
                let mut p: String = line.chars().take(60).collect();
                if *truncated || p.len() < text.len() {
                    p.push('…');
                }
                p
            }
            Content::Image { width, height } => format!("[image {width}x{height}]"),
            Content::Files { names } => format!("[files: {}]", names.join(", ")),
        };
        println!(
            "{:>3}  {:>8}  {:<16}  {}",
            i + 1,
            ago(now.saturating_sub(e.at)),
            e.from,
            preview
        );
    }
    if empty {
        println!("Nothing copied yet. History is recorded while `clipd run` is running.");
    }
}

fn ago(secs: u64) -> String {
    match secs {
        0..=59 => format!("{secs}s ago"),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86399 => format!("{}h ago", secs / 3600),
        _ => format!("{}d ago", secs / 86400),
    }
}
