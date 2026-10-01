//! Core of the universal clipboard: circle membership, pairing, discovery,
//! encrypted transport and the clipboard sync engine. Every platform app
//! (desktop via Tauri, Android via UniFFI) is meant to sit on top of this.

pub mod clipboard;
pub mod discovery;
pub mod history;
pub mod keychain;
pub mod pairing;
pub mod protocol;
pub mod service;
pub mod state;
pub mod sync;
pub mod transport;

pub use state::{Member, State};
