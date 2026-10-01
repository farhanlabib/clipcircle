//! Noise-encrypted, length-framed messages over TCP.
//!
//! Wire format: every Noise message is sent as a big-endian u16 length followed
//! by that many bytes. An application message is a 4-byte length header plus
//! the payload, split across as many Noise messages as needed.

use anyhow::{bail, Context, Result};
use serde::{de::DeserializeOwned, Serialize};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// Used between circle members: both sides prove their static key.
pub const SYNC_PARAMS: &str = "Noise_XX_25519_ChaChaPoly_BLAKE2s";
/// Used while pairing: the key comes from the SPAKE2 exchange on the pairing code.
pub const PAIR_PARAMS: &str = "Noise_NNpsk0_25519_ChaChaPoly_BLAKE2s";

const NOISE_MAX: usize = 65535;
const TAG_LEN: usize = 16;
const CHUNK: usize = NOISE_MAX - TAG_LEN;
/// Largest application message we accept (clipboard text included).
pub const MAX_MESSAGE: usize = 48 * 1024 * 1024;

pub fn generate_keypair() -> Result<snow::Keypair> {
    Ok(snow::Builder::new(SYNC_PARAMS.parse()?).generate_keypair()?)
}

pub async fn write_frame(stream: &mut TcpStream, data: &[u8]) -> Result<()> {
    let len = u16::try_from(data.len()).context("frame too large")?;
    stream.write_all(&len.to_be_bytes()).await?;
    stream.write_all(data).await?;
    Ok(())
}

pub async fn read_frame(stream: &mut TcpStream) -> Result<Vec<u8>> {
    let mut len = [0u8; 2];
    stream.read_exact(&mut len).await?;
    let mut buf = vec![0u8; u16::from_be_bytes(len) as usize];
    stream.read_exact(&mut buf).await?;
    Ok(buf)
}

/// Drives a Noise handshake to completion over `stream`.
async fn handshake(mut stream: TcpStream, mut hs: snow::HandshakeState) -> Result<SecureStream> {
    let mut buf = vec![0u8; NOISE_MAX];
    while !hs.is_handshake_finished() {
        if hs.is_my_turn() {
            let n = hs.write_message(&[], &mut buf)?;
            write_frame(&mut stream, &buf[..n]).await?;
        } else {
            let msg = read_frame(&mut stream).await?;
            hs.read_message(&msg, &mut buf)
                .context("noise handshake failed")?;
        }
    }
    let remote_static = hs.get_remote_static().map(<[u8]>::to_vec);
    Ok(SecureStream {
        stream,
        noise: hs.into_transport_mode()?,
        remote_static,
    })
}

pub async fn sync_initiator(stream: TcpStream, private_key: &[u8]) -> Result<SecureStream> {
    let hs = snow::Builder::new(SYNC_PARAMS.parse()?)
        .local_private_key(private_key)?
        .build_initiator()?;
    handshake(stream, hs).await
}

pub async fn sync_responder(stream: TcpStream, private_key: &[u8]) -> Result<SecureStream> {
    let hs = snow::Builder::new(SYNC_PARAMS.parse()?)
        .local_private_key(private_key)?
        .build_responder()?;
    handshake(stream, hs).await
}

pub async fn pair_initiator(stream: TcpStream, psk: &[u8; 32]) -> Result<SecureStream> {
    let hs = snow::Builder::new(PAIR_PARAMS.parse()?)
        .psk(0, psk)?
        .build_initiator()?;
    handshake(stream, hs).await
}

pub async fn pair_responder(stream: TcpStream, psk: &[u8; 32]) -> Result<SecureStream> {
    let hs = snow::Builder::new(PAIR_PARAMS.parse()?)
        .psk(0, psk)?
        .build_responder()?;
    handshake(stream, hs).await
}

pub struct SecureStream {
    stream: TcpStream,
    noise: snow::TransportState,
    remote_static: Option<Vec<u8>>,
}

impl SecureStream {
    /// The peer's long-term public key (only set for the XX sync handshake).
    pub fn remote_static(&self) -> Option<&[u8]> {
        self.remote_static.as_deref()
    }

    pub async fn send(&mut self, payload: &[u8]) -> Result<()> {
        if payload.len() > MAX_MESSAGE {
            bail!("message of {} bytes exceeds limit", payload.len());
        }
        let mut plain = Vec::with_capacity(payload.len() + 4);
        plain.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        plain.extend_from_slice(payload);
        let mut buf = vec![0u8; NOISE_MAX];
        for chunk in plain.chunks(CHUNK) {
            let n = self.noise.write_message(chunk, &mut buf)?;
            write_frame(&mut self.stream, &buf[..n]).await?;
        }
        self.stream.flush().await?;
        Ok(())
    }

    pub async fn recv(&mut self) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; NOISE_MAX];
        let mut plain = Vec::new();
        let mut expected = None;
        loop {
            let frame = read_frame(&mut self.stream).await?;
            let n = self
                .noise
                .read_message(&frame, &mut buf)
                .context("decrypt failed")?;
            plain.extend_from_slice(&buf[..n]);
            if expected.is_none() && plain.len() >= 4 {
                let len = u32::from_be_bytes(plain[..4].try_into().unwrap()) as usize;
                if len > MAX_MESSAGE {
                    bail!("peer announced {len} byte message, over limit");
                }
                expected = Some(len + 4);
            }
            match expected {
                Some(total) if plain.len() == total => return Ok(plain.split_off(4)),
                Some(total) if plain.len() > total => bail!("message longer than announced"),
                _ => {}
            }
        }
    }

    pub async fn send_json<T: Serialize>(&mut self, msg: &T) -> Result<()> {
        self.send(&serde_json::to_vec(msg)?).await
    }

    pub async fn recv_json<T: DeserializeOwned>(&mut self) -> Result<T> {
        Ok(serde_json::from_slice(&self.recv().await?)?)
    }
}
