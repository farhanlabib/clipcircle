//! Adding a device to a circle with a short code.
//!
//! The host shows a 6-digit code; the joiner types it. Both run SPAKE2 on the
//! code, so an eavesdropper learns nothing and an active attacker gets exactly
//! one guess per attempt. The SPAKE2 key then seeds a Noise NNpsk0 channel that
//! carries the joiner's identity one way and the circle's member list back.

use anyhow::{anyhow, bail, Context, Result};
use rand::Rng;
use spake2::{Ed25519Group, Identity, Password, Spake2};
use tokio::net::{TcpListener, TcpStream};

use crate::protocol::Message;
use crate::transport::{self, read_frame, write_frame};
use crate::{Member, State};

const SPAKE_ID: &[u8] = b"clipcircle/pair/v1";
/// Wrong codes the host tolerates before it stops pairing.
pub const MAX_ATTEMPTS: usize = 3;

pub fn generate_code() -> String {
    format!("{:06}", rand::thread_rng().gen_range(0..1_000_000))
}

async fn spake(stream: &mut TcpStream, code: &str) -> Result<[u8; 32]> {
    let (spake, outbound) = Spake2::<Ed25519Group>::start_symmetric(
        &Password::new(code.trim().as_bytes()),
        &Identity::new(SPAKE_ID),
    );
    write_frame(stream, &outbound).await?;
    let inbound = read_frame(stream).await?;
    let key = spake
        .finish(&inbound)
        .map_err(|e| anyhow!("pairing exchange failed: {e:?}"))?;
    key.try_into()
        .map_err(|_| anyhow!("unexpected pairing key length"))
}

/// Waits for one device to join with `code` and admits it into `state`'s circle.
pub async fn host(listener: &TcpListener, code: &str, state: &mut State) -> Result<Member> {
    for attempt in 1..=MAX_ATTEMPTS {
        let (mut stream, peer) = listener.accept().await?;
        let result = async {
            let key = spake(&mut stream, code).await?;
            let mut chan = transport::pair_responder(stream, &key).await?;
            let Message::Join { device } = chan.recv_json().await? else {
                bail!("expected join message");
            };
            // Pairing again is how a removed device is let back in.
            state.removed.retain(|k| *k != device.public_key);
            state.merge_members(std::slice::from_ref(&device));
            chan.send_json(&Message::Welcome {
                circle_id: state.circle_id.clone(),
                members: state.all_members(),
            })
            .await?;
            Ok(device)
        }
        .await;
        match result {
            Ok(device) => return Ok(device),
            Err(e) => tracing::warn!(%peer, attempt, "pairing attempt failed: {e:#}"),
        }
    }
    bail!("too many failed pairing attempts")
}

/// Joins the circle hosted at `stream`'s peer, replacing `state`'s circle.
pub async fn join(mut stream: TcpStream, code: &str, state: &mut State) -> Result<()> {
    let key = spake(&mut stream, code).await?;
    let mut chan = transport::pair_initiator(stream, &key)
        .await
        .context("wrong code?")?;
    chan.send_json(&Message::Join {
        device: state.device.clone(),
    })
    .await?;
    let Message::Welcome { circle_id, members } = chan.recv_json().await.context("wrong code?")?
    else {
        bail!("expected welcome message");
    };
    state.circle_id = circle_id;
    state.members.clear();
    state.merge_members(&members);
    Ok(())
}
