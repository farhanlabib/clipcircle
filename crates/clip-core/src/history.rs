//! A short, local history of what was copied here or received from the circle.
//! Only text is kept in full; images are listed but not stored.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::clipboard::Clip;

/// Entries kept; older ones are dropped.
pub const MAX_ENTRIES: usize = 50;
/// Longer texts are cut to this many bytes in the history.
pub const MAX_TEXT: usize = 64 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Content {
    Text { text: String, truncated: bool },
    Image { width: usize, height: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Entry {
    /// Seconds since the Unix epoch.
    pub at: u64,
    /// Name of the device it was copied on.
    pub from: String,
    pub content: Content,
}

impl Entry {
    /// Text that can be put back on the clipboard, if this entry has it all.
    pub fn full_text(&self) -> Option<&str> {
        match &self.content {
            Content::Text {
                text,
                truncated: false,
            } => Some(text),
            _ => None,
        }
    }
}

pub struct History {
    path: PathBuf,
    /// Oldest first.
    entries: Vec<Entry>,
}

fn content_of(clip: &Clip) -> Content {
    match clip {
        Clip::Text(t) if t.len() > MAX_TEXT => {
            let mut end = MAX_TEXT;
            while !t.is_char_boundary(end) {
                end -= 1;
            }
            Content::Text {
                text: t[..end].to_owned(),
                truncated: true,
            }
        }
        Clip::Text(t) => Content::Text {
            text: t.clone(),
            truncated: false,
        },
        Clip::Image { width, height, .. } => Content::Image {
            width: *width,
            height: *height,
        },
    }
}

impl History {
    /// Where the history for the state file at `state_path` lives.
    pub fn path_for(state_path: &Path) -> PathBuf {
        state_path.with_extension("history.json")
    }

    /// Loads the history at `path`; a missing file is an empty history.
    pub fn load(path: &Path) -> Result<Self> {
        let entries = match std::fs::read_to_string(path) {
            Ok(raw) => {
                serde_json::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => return Err(e).with_context(|| format!("reading {}", path.display())),
        };
        Ok(Self {
            path: path.to_owned(),
            entries,
        })
    }

    /// Newest first.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().rev()
    }

    /// Adds `clip` unless it repeats the newest entry, then saves.
    pub fn record(&mut self, from: &str, clip: &Clip) -> Result<()> {
        let content = content_of(clip);
        if self.entries.last().is_some_and(|e| e.content == content) {
            return Ok(());
        }
        let at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        self.entries.push(Entry {
            at,
            from: from.to_owned(),
            content,
        });
        let excess = self.entries.len().saturating_sub(MAX_ENTRIES);
        self.entries.drain(..excess);
        self.save()
    }

    pub fn clear(&mut self) -> Result<()> {
        self.entries.clear();
        self.save()
    }

    fn save(&self) -> Result<()> {
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = self.path.with_extension("tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(&self.entries)?)?;
        crate::state::restrict_permissions(&tmp)?;
        std::fs::rename(&tmp, &self.path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_history() -> (History, PathBuf) {
        let dir = std::env::temp_dir().join(format!("uc-history-{}", rand::random::<u64>()));
        let path = dir.join("state.history.json");
        (History::load(&path).unwrap(), dir)
    }

    #[test]
    fn records_newest_first_skips_repeats_and_caps_length() {
        let (mut h, dir) = temp_history();
        h.record("mac", &Clip::Text("one".into())).unwrap();
        h.record("mac", &Clip::Text("one".into())).unwrap();
        h.record(
            "windows",
            &Clip::Image {
                width: 2,
                height: 1,
                rgba: vec![0; 8],
            },
        )
        .unwrap();
        let got: Vec<_> = h.entries().map(|e| e.from.as_str()).collect();
        assert_eq!(got, ["windows", "mac"]);

        for i in 0..MAX_ENTRIES + 5 {
            h.record("mac", &Clip::Text(i.to_string())).unwrap();
        }
        assert_eq!(h.entries().count(), MAX_ENTRIES);
        assert_eq!(
            h.entries().next().unwrap().full_text(),
            Some(&*(MAX_ENTRIES + 4).to_string())
        );

        // Survives a reload.
        let reloaded = History::load(&h.path).unwrap();
        assert_eq!(reloaded.entries, h.entries);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn long_text_is_truncated_and_cannot_be_copied_back() {
        let (mut h, dir) = temp_history();
        h.record("mac", &Clip::Text("é".repeat(MAX_TEXT))).unwrap();
        let e = h.entries().next().unwrap();
        assert_eq!(e.full_text(), None);
        let Content::Text { text, truncated } = &e.content else {
            panic!()
        };
        assert!(*truncated && text.len() <= MAX_TEXT);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
