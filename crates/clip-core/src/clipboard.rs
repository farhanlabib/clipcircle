use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::{bail, Context, Result};
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// Files copied together may hold at most this many bytes in total. They are
/// streamed from disk to disk, so this only guards the receiver's disk.
pub const MAX_FILES_BYTES: u64 = 4 << 30;

/// Something on the clipboard, as read from or written to the OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Clip {
    Text(String),
    /// Raw RGBA pixels, 4 bytes per pixel, row by row.
    Image {
        width: usize,
        height: usize,
        rgba: Vec<u8>,
    },
    /// Files copied in a file manager (not folders). Their contents stay on
    /// disk and are read only while sending.
    Files(Vec<FileRef>),
}

/// A file on disk, as copied or as received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileRef {
    /// A bare file name, never a path. Usually the last part of `path`.
    pub name: String,
    pub size: u64,
    pub path: PathBuf,
}

impl FileRef {
    /// Refers to the file at `path` under its own name.
    pub fn from_path(path: &Path) -> Result<Self> {
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .with_context(|| format!("unusable file name {}", path.display()))?;
        Self::named(name, path)
    }

    /// Refers to the file at `path`, to be sent as `name`.
    pub fn named(name: &str, path: &Path) -> Result<Self> {
        if !is_safe_file_name(name) {
            bail!("unusable file name {name:?}");
        }
        let meta =
            std::fs::metadata(path).with_context(|| format!("reading {}", path.display()))?;
        if !meta.is_file() {
            bail!("{} is not a file (folders are not synced)", path.display());
        }
        Ok(Self {
            name: name.to_owned(),
            size: meta.len(),
            path: path.to_owned(),
        })
    }
}

/// Whether `name` can be used as a file name without escaping a directory.
pub fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.contains(['/', '\\', '\0', ':'])
        && name.len() <= 255
}

impl Clip {
    pub fn digest(&self) -> [u8; 32] {
        let mut h = Sha256::new();
        match self {
            Clip::Text(t) => {
                h.update(b"text\0");
                h.update(t.as_bytes());
            }
            Clip::Image {
                width,
                height,
                rgba,
            } => {
                h.update(b"image\0");
                h.update((*width as u64).to_be_bytes());
                h.update((*height as u64).to_be_bytes());
                h.update(rgba);
            }
            // Contents are not hashed: that would mean reading every copied
            // file each poll. The path and size tell copies apart well enough.
            Clip::Files(files) => {
                h.update(b"files\0");
                for f in files {
                    for part in [f.name.as_bytes(), f.path.as_os_str().as_encoded_bytes()] {
                        h.update((part.len() as u64).to_be_bytes());
                        h.update(part);
                    }
                    h.update(f.size.to_be_bytes());
                }
            }
        }
        h.finalize().into()
    }

    pub fn len(&self) -> usize {
        match self {
            Clip::Text(t) => t.len(),
            Clip::Image { rgba, .. } => rgba.len(),
            Clip::Files(files) => files.iter().map(|f| f.size as usize).sum(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Decodes a PNG into an image clip.
    pub fn from_png(bytes: &[u8]) -> Result<Self> {
        decode_png(bytes)
    }

    /// The image as PNG bytes; fails for anything else.
    pub fn to_png(&self) -> Result<Vec<u8>> {
        match self {
            Clip::Image {
                width,
                height,
                rgba,
            } => encode_png(*width, *height, rgba),
            _ => bail!("not an image"),
        }
    }

    /// Wire form: images travel as PNG, which is far smaller than raw RGBA.
    /// Files travel as names and sizes; their contents follow the message.
    pub fn to_wire(&self) -> Result<WireClip> {
        Ok(match self {
            Clip::Text(t) => WireClip::Text { text: t.clone() },
            Clip::Image {
                width,
                height,
                rgba,
            } => WireClip::Image {
                png: base64::engine::general_purpose::STANDARD
                    .encode(encode_png(*width, *height, rgba)?),
            },
            Clip::Files(files) => WireClip::Files {
                files: files
                    .iter()
                    .map(|f| WireFile {
                        name: f.name.clone(),
                        size: f.size,
                    })
                    .collect(),
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
            WireClip::Files { .. } => bail!("files are received with their contents"),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WireClip {
    Text {
        text: String,
    },
    /// Base64-encoded PNG.
    Image {
        png: String,
    },
    /// Names and sizes; the contents follow as raw messages, file by file.
    Files {
        files: Vec<WireFile>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WireFile {
    pub name: String,
    pub size: u64,
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
        png::ColorType::Rgb => buf
            .chunks(3)
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .chunks(2)
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        other => bail!("unsupported PNG color type {other:?}"),
    };
    Ok(Clip::Image {
        width,
        height,
        rgba,
    })
}

/// The OS clipboard, abstracted so the engine can be tested without one.
pub trait Clipboard: Send {
    /// Current contents, text preferred over image.
    fn get(&mut self) -> Option<Clip>;
    fn set(&mut self, clip: &Clip) -> Result<()>;
}

#[cfg(feature = "system-clipboard")]
pub struct SystemClipboard {
    inner: arboard::Clipboard,
    /// The file list last read and what it gave, so polling doesn't warn
    /// again about the same unusable files.
    files_cache: Option<(Vec<FileStamp>, Option<Clip>)>,
}

/// A copied file's path, size and modification time.
#[cfg(feature = "system-clipboard")]
type FileStamp = (std::path::PathBuf, u64, Option<std::time::SystemTime>);

#[cfg(feature = "system-clipboard")]
impl SystemClipboard {
    pub fn new() -> Result<Self> {
        Ok(Self {
            inner: arboard::Clipboard::new()?,
            files_cache: None,
        })
    }

    fn read_files(&mut self, paths: Vec<std::path::PathBuf>) -> Option<Clip> {
        let stamps: Vec<FileStamp> = paths
            .into_iter()
            .map(|p| {
                let meta = std::fs::metadata(&p).ok();
                let len = meta.as_ref().map_or(0, |m| m.len());
                let modified = meta.and_then(|m| m.modified().ok());
                (p, len, modified)
            })
            .collect();
        if let Some((cached, clip)) = &self.files_cache {
            if *cached == stamps {
                return clip.clone();
            }
        }
        let clip = match load_files(&stamps) {
            Ok(clip) => Some(clip),
            Err(e) => {
                tracing::warn!("not syncing the copied files: {e:#}");
                None
            }
        };
        self.files_cache = Some((stamps, clip.clone()));
        clip
    }
}

#[cfg(feature = "system-clipboard")]
fn load_files(stamps: &[FileStamp]) -> Result<Clip> {
    let files = stamps
        .iter()
        .map(|(path, _, _)| FileRef::from_path(path))
        .collect::<Result<Vec<_>>>()?;
    if files.iter().map(|f| f.size).sum::<u64>() > MAX_FILES_BYTES {
        bail!("they are larger than {} GiB", MAX_FILES_BYTES >> 30);
    }
    Ok(Clip::Files(files))
}

#[cfg(feature = "system-clipboard")]
impl Clipboard for SystemClipboard {
    fn get(&mut self) -> Option<Clip> {
        // Copied files come first: their clipboard entry often has the file
        // name as text too.
        if let Ok(paths) = self.inner.get().file_list() {
            if !paths.is_empty() {
                return self.read_files(paths);
            }
        }
        if let Ok(text) = self.inner.get_text() {
            return Some(Clip::Text(text));
        }
        let img = self.inner.get_image().ok()?;
        Some(Clip::Image {
            width: img.width,
            height: img.height,
            rgba: img.bytes.into_owned(),
        })
    }

    fn set(&mut self, clip: &Clip) -> Result<()> {
        match clip {
            Clip::Text(t) => self.inner.set_text(t)?,
            Clip::Image {
                width,
                height,
                rgba,
            } => self.inner.set_image(arboard::ImageData {
                width: *width,
                height: *height,
                bytes: std::borrow::Cow::Borrowed(rgba),
            })?,
            Clip::Files(files) => {
                let paths: Vec<&Path> = files.iter().map(|f| f.path.as_path()).collect();
                self.inner.set().file_list(&paths)?;
            }
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
        let clip = Clip::Image {
            width: 3,
            height: 2,
            rgba,
        };
        let back = Clip::from_wire(clip.to_wire().unwrap()).unwrap();
        assert_eq!(back, clip);
    }

    #[test]
    fn png_bytes_round_trip_and_text_has_none() {
        let clip = Clip::Image {
            width: 2,
            height: 2,
            rgba: vec![9; 16],
        };
        assert_eq!(Clip::from_png(&clip.to_png().unwrap()).unwrap(), clip);
        assert!(Clip::Text("hi".into()).to_png().is_err());
        assert!(Clip::from_png(b"not a png").is_err());
    }

    /// Writes small files named `names` into a fresh temp folder.
    fn files(tag: &str, names: &[&str]) -> Clip {
        let dir = std::env::temp_dir().join(format!("clip-core-test-{tag}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // macOS hands back /private/var for /var.
        let dir = dir.canonicalize().unwrap();
        Clip::Files(
            names
                .iter()
                .map(|n| {
                    let path = dir.join(n);
                    std::fs::write(&path, n.repeat(3)).unwrap();
                    FileRef::from_path(&path).unwrap()
                })
                .collect(),
        )
    }

    #[test]
    fn files_travel_as_names_and_sizes() {
        let clip = files("wire", &["report.pdf", "photo 1.jpg"]);
        let WireClip::Files { files: wire } = clip.to_wire().unwrap() else {
            panic!("not files");
        };
        assert_eq!(wire[1].name, "photo 1.jpg");
        assert_eq!(wire[1].size, 33);
        assert_eq!(clip.len(), 30 + 33);
        assert_ne!(clip.digest(), files("wire", &["report.pdf"]).digest());
    }

    #[test]
    fn unsafe_file_names_are_refused() {
        for bad in ["", ".", "..", "../evil", "a/b", "a\\b", "C:evil", "nul\0"] {
            assert!(!is_safe_file_name(bad), "{bad:?}");
            assert!(
                FileRef::named(bad, Path::new("Cargo.toml")).is_err(),
                "{bad:?}"
            );
        }
        assert!(is_safe_file_name("notes (final).txt"));
    }

    #[test]
    fn folders_are_not_files() {
        assert!(FileRef::from_path(&std::env::temp_dir()).is_err());
        assert!(FileRef::from_path(Path::new("/no/such/file.txt")).is_err());
    }

    /// Needs a desktop session: `cargo test -- --ignored`.
    #[cfg(feature = "system-clipboard")]
    #[test]
    #[ignore]
    fn system_clipboard_round_trips_files() {
        let clip = files("system", &["a.txt", "b.txt"]);
        let mut cb = SystemClipboard::new().unwrap();
        cb.set(&clip).unwrap();
        assert_eq!(cb.get(), Some(clip));
    }

    #[test]
    fn mismatched_image_size_is_rejected() {
        let clip = Clip::Image {
            width: 10,
            height: 10,
            rgba: vec![0; 4],
        };
        assert!(clip.to_wire().is_err());
    }
}
