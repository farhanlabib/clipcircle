use std::borrow::Cow;
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Something on the clipboard, as read from or written to the OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clip {
    Text(String),
    /// Raw RGBA pixels, 4 bytes per pixel, row by row.
    Image { width: usize, height: usize, rgba: Vec<u8> },
}

impl Clip {
    pub fn digest(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        match self {
            Clip::Text(t) => {
                h.update(b"text\0");
                h.update(t.as_bytes());
            }
            Clip::Image { width, height, rgba } => {
                h.update(b"image\0");
                h.update((*width as u64).to_be_bytes());
                h.update((*height as u64).to_be_bytes());
                h.update(rgba);
            }
        }
        h.finalize().into()
    }

    pub fn len(&self) -> usize {
        match self {
            Clip::Text(t) => t.len(),
            Clip::Image { rgba, .. } => rgba.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Wire form: images travel as PNG, which is far smaller than raw RGBA.
    pub fn to_wire(&self) -> Result<WireClip> {
        Ok(match self {
            Clip::Text(t) => WireClip::Text { text: t.clone() },
            Clip::Image { width, height, rgba } => WireClip::Image {
                png: base64::engine::general_purpose::STANDARD.encode(encode_png(*width, *height, rgba)?),
            },
        })
    }

    pub fn from_wire(wire: WireClip) -> Result<Self> {
        Ok(match wire {
            WireClip::Text { text } => Clip::Text(text),
            WireClip::Image { png } => {
                let bytes = base64::engine::general_purpose::STANDARD.decode(png)?;
                decode_png(&bytes)?
            }
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireClip {
    Text { text: String },
    /// Base64-encoded PNG.
    Image { png: String },
}

fn encode_png(width: usize, height: usize, rgba: &[u8]) -> Result<Vec<u8>> {
    if rgba.len() != width * height * 4 {
        bail!("image is {}x{} but has {} bytes", width, height, rgba.len());
    }
    let mut out = Vec::new();
    let mut enc = png::Encoder::new(&mut out, width as u32, height as u32);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.set_compression(png::Compression::Fast);
    enc.write_header()?.write_image_data(rgba)?;
    Ok(out)
}

/// Images bigger than this many pixels are refused (64 megapixels).
const MAX_PIXELS: usize = 64 * 1024 * 1024;

fn decode_png(bytes: &[u8]) -> Result<Clip> {
    let mut decoder = png::Decoder::new(bytes);
    decoder.set_transformations(png::Transformations::normalize_to_color8());
    let mut reader = decoder.read_info().context("not a PNG")?;
    let (width, height) = {
        let info = reader.info();
        (info.width as usize, info.height as usize)
    };
    if width * height > MAX_PIXELS {
        bail!("image of {width}x{height} is too large");
    }
    let mut buf = vec![0u8; reader.output_buffer_size()];
    let frame = reader.next_frame(&mut buf)?;
    buf.truncate(frame.buffer_size());
    let rgba = match frame.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf.chunks(3).flat_map(|p| [p[0], p[1], p[2], 255]).collect(),
        png::ColorType::GrayscaleAlpha => buf.chunks(2).flat_map(|p| [p[0], p[0], p[0], p[1]]).collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        other => bail!("unsupported PNG color type {other:?}"),
    };
    Ok(Clip::Image { width, height, rgba })
}

/// The OS clipboard, abstracted so the engine can be tested without one.
pub trait Clipboard: Send {
    /// Current contents, text preferred over image.
    fn get(&mut self) -> Option<Clip>;
    fn set(&mut self, clip: &Clip) -> Result<()>;
}

pub struct SystemClipboard(arboard::Clipboard);

impl SystemClipboard {
    pub fn new() -> Result<Self> {
        Ok(Self(arboard::Clipboard::new()?))
    }
}

impl Clipboard for SystemClipboard {
    fn get(&mut self) -> Option<Clip> {
        if let Ok(text) = self.0.get_text() {
            return Some(Clip::Text(text));
        }
        let img = self.0.get_image().ok()?;
        Some(Clip::Image { width: img.width, height: img.height, rgba: img.bytes.into_owned() })
    }

    fn set(&mut self, clip: &Clip) -> Result<()> {
        match clip {
            Clip::Text(t) => self.0.set_text(t)?,
            Clip::Image { width, height, rgba } => self.0.set_image(arboard::ImageData {
                width: *width,
                height: *height,
                bytes: Cow::Borrowed(rgba),
            })?,
        }
        Ok(())
    }
}

/// In-memory clipboard for tests; clones share the same contents.
#[derive(Clone, Default)]
pub struct MemoryClipboard(Arc<Mutex<Option<Clip>>>);

impl Clipboard for MemoryClipboard {
    fn get(&mut self) -> Option<Clip> {
        self.0.lock().unwrap().clone()
    }

    fn set(&mut self, clip: &Clip) -> Result<()> {
        *self.0.lock().unwrap() = Some(clip.clone());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image_survives_png_round_trip() {
        let rgba: Vec<u8> = (0..3 * 2 * 4).map(|i| i as u8 * 7).collect();
        let clip = Clip::Image { width: 3, height: 2, rgba };
        let back = Clip::from_wire(clip.to_wire().unwrap()).unwrap();
        assert_eq!(back, clip);
    }

    #[test]
    fn mismatched_image_size_is_rejected() {
        let clip = Clip::Image { width: 10, height: 10, rgba: vec![0; 4] };
        assert!(clip.to_wire().is_err());
    }
}
