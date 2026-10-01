use serde::{Deserialize, Serialize};

use crate::clipboard::WireClip;
use crate::Member;

/// Messages exchanged inside an encrypted channel.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    /// Pairing: the joining device introduces itself.
    Join { device: Member },
    /// Pairing: the host admits the joiner into its circle.
    Welcome {
        circle_id: String,
        members: Vec<Member>,
    },
    /// Sync: first message each way, used to gossip circle membership.
    Hello {
        circle_id: String,
        members: Vec<Member>,
        /// Public keys of devices removed from the circle.
        #[serde(default)]
        removed: Vec<String>,
    },
    /// Sync: new clipboard contents.
    Clip { clip: WireClip },
    /// Sync: a connection check; sent instead of a clip and needs no answer.
    Ping,
}
