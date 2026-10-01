use std::sync::{Arc, Mutex};

use anyhow::Result;

/// The OS clipboard, abstracted so the engine can be tested without one.
pub trait Clipboard: Send {
    fn get_text(&mut self) -> Option<String>;
    fn set_text(&mut self, text: &str) -> Result<()>;
}

pub struct SystemClipboard(arboard::Clipboard);

impl SystemClipboard {
    pub fn new() -> Result<Self> {
        Ok(Self(arboard::Clipboard::new()?))
    }
}

impl Clipboard for SystemClipboard {
    fn get_text(&mut self) -> Option<String> {
        self.0.get_text().ok()
    }

    fn set_text(&mut self, text: &str) -> Result<()> {
        Ok(self.0.set_text(text)?)
    }
}

/// In-memory clipboard for tests; clones share the same contents.
#[derive(Clone, Default)]
pub struct MemoryClipboard(Arc<Mutex<Option<String>>>);

impl Clipboard for MemoryClipboard {
    fn get_text(&mut self) -> Option<String> {
        self.0.lock().unwrap().clone()
    }

    fn set_text(&mut self, text: &str) -> Result<()> {
        *self.0.lock().unwrap() = Some(text.to_owned());
        Ok(())
    }
}
