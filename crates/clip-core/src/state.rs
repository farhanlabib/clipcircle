use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use rand::RngCore;
use serde::{Deserialize, Serialize};

use crate::transport;

/// A device in the circle, identified by its long-term Noise public key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Member {
    pub id: String,
    pub name: String,
    /// Hex-encoded X25519 static public key.
    pub public_key: String,
}

/// Everything this device persists: its identity and the circle it belongs to.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct State {
    pub device: Member,
    /// Hex-encoded X25519 static private key.
    /// TODO: move into the OS keychain (Keychain / DPAPI / Secret Service / Keystore).
    pub private_key: String,
    pub circle_id: String,
    /// Other devices in the circle (never includes `device`).
    pub members: Vec<Member>,
    /// Public keys of devices removed from the circle. Kept so that gossip
    /// from a member that hasn't heard about the removal can't re-add them.
    #[serde(default)]
    pub removed: Vec<String>,
}

fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    rand::thread_rng().fill_bytes(&mut buf);
    hex::encode(buf)
}

impl State {
    /// A fresh device that is alone in its own circle.
    pub fn generate(name: impl Into<String>) -> Result<Self> {
        let keypair = transport::generate_keypair()?;
        Ok(Self {
            device: Member {
                id: random_hex(8),
                name: name.into(),
                public_key: hex::encode(&keypair.public),
            },
            private_key: hex::encode(&keypair.private),
            circle_id: random_hex(16),
            members: Vec::new(),
            removed: Vec::new(),
        })
    }

    pub fn default_path() -> Result<PathBuf> {
        let dir = dirs::config_dir().context("no config directory on this platform")?;
        Ok(dir.join("universal-clipboard").join("state.json"))
    }

    /// Loads the state at `path`, creating a new device there if none exists.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let raw = std::fs::read_to_string(path)
                .with_context(|| format!("reading {}", path.display()))?;
            return serde_json::from_str(&raw)
                .with_context(|| format!("parsing {}", path.display()));
        }
        let name = gethostname::gethostname().to_string_lossy().into_owned();
        let state = Self::generate(name)?;
        state.save(path)?;
        Ok(state)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    pub fn private_key_bytes(&self) -> Result<Vec<u8>> {
        Ok(hex::decode(&self.private_key)?)
    }

    pub fn member_by_key(&self, public_key: &[u8]) -> Option<&Member> {
        let key = hex::encode(public_key);
        self.members.iter().find(|m| m.public_key == key)
    }

    /// Adds members we did not know about yet. Returns true if anything changed.
    pub fn merge_members(&mut self, others: &[Member]) -> bool {
        let mut changed = false;
        for m in others {
            if m.public_key == self.device.public_key || self.removed.contains(&m.public_key) {
                continue;
            }
            if !self
                .members
                .iter()
                .any(|known| known.public_key == m.public_key)
            {
                self.members.push(m.clone());
                changed = true;
            }
        }
        changed
    }

    /// Removes the member whose id or name is `who`. Errors if none or several match.
    pub fn remove_member(&mut self, who: &str) -> Result<Member> {
        let matches: Vec<usize> = (0..self.members.len())
            .filter(|&i| self.members[i].id == who || self.members[i].name == who)
            .collect();
        match matches.as_slice() {
            [] => anyhow::bail!("no device called {who:?} in this circle"),
            [i] => {
                let m = self.members.remove(*i);
                self.removed.push(m.public_key.clone());
                Ok(m)
            }
            _ => anyhow::bail!("several devices are called {who:?}; use the device id instead"),
        }
    }

    /// Applies removals another member knows about. Returns true if anything changed.
    pub fn merge_removed(&mut self, keys: &[String]) -> bool {
        let mut changed = false;
        for key in keys {
            if *key == self.device.public_key || self.removed.contains(key) {
                continue;
            }
            self.removed.push(key.clone());
            self.members.retain(|m| m.public_key != *key);
            changed = true;
        }
        changed
    }

    /// This device plus every known member, as shared with peers.
    pub fn all_members(&self) -> Vec<Member> {
        std::iter::once(self.device.clone())
            .chain(self.members.iter().cloned())
            .collect()
    }
}
