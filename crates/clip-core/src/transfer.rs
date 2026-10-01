//! Streams copied files over a sync connection, disk to disk, so how big they
//! can be depends on free space rather than memory.
//!
//! After a `Clip` message listing the files' names and sizes, the sender sends
//! each file's bytes in order as messages of up to [`BLOCK`] bytes. The
//! receiver answers `Received` once everything is on its disk.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Context, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::clipboard::{is_safe_file_name, FileRef, WireFile, MAX_FILES_BYTES};
use crate::transport::SecureStream;

/// Bytes per message while streaming a file.
const BLOCK: usize = 1 << 20;
/// A transfer that makes no progress for this long is abandoned.
pub const STALL_TIMEOUT: Duration = Duration::from_secs(60);
/// Folders of received files kept on disk; older ones are deleted.
const KEEP_RECEIVED: usize = 3;

/// Where received files go unless the app picks a folder.
pub fn default_received_dir() -> PathBuf {
    std::env::temp_dir().join("universal-clipboard")
}

/// Sends the contents of `files`, which the peer expects after the clip.
pub async fn send(chan: &mut SecureStream, files: &[FileRef]) -> Result<()> {
    let mut buf = vec![0u8; BLOCK];
    for f in files {
        let mut file = tokio::fs::File::open(&f.path)
            .await
            .with_context(|| format!("opening {}", f.path.display()))?;
        let mut left = f.size;
        while left > 0 {
            let n = left.min(BLOCK as u64) as usize;
            file.read_exact(&mut buf[..n])
                .await
                .with_context(|| format!("{} got shorter while sending", f.path.display()))?;
            chan.send(&buf[..n]).await?;
            left -= n as u64;
        }
    }
    Ok(())
}

/// Receives the contents of `files` into a new folder under `root` and
/// returns where they are. Nothing is left behind if the transfer fails.
pub async fn receive(
    chan: &mut SecureStream,
    files: &[WireFile],
    root: &Path,
) -> Result<Vec<FileRef>> {
    check(files)?;
    let dir = new_folder(root)?;
    match receive_into(chan, files, &dir).await {
        Ok(refs) => {
            prune(root, &dir);
            Ok(refs)
        }
        Err(e) => {
            let _ = tokio::fs::remove_dir_all(&dir).await;
            Err(e)
        }
    }
}

fn check(files: &[WireFile]) -> Result<()> {
    if files.is_empty() {
        bail!("no files");
    }
    let mut total = 0u64;
    for f in files {
        if !is_safe_file_name(&f.name) {
            bail!("refusing file name {:?}", f.name);
        }
        total = total.saturating_add(f.size);
    }
    if total > MAX_FILES_BYTES {
        bail!("files are larger than {} GiB", MAX_FILES_BYTES >> 30);
    }
    Ok(())
}

async fn receive_into(
    chan: &mut SecureStream,
    files: &[WireFile],
    dir: &Path,
) -> Result<Vec<FileRef>> {
    let mut taken = HashSet::new();
    let mut out = Vec::with_capacity(files.len());
    for f in files {
        let name = unique_name(&f.name, &mut taken);
        let path = dir.join(&name);
        let mut file = tokio::fs::File::create(&path)
            .await
            .with_context(|| format!("creating {}", path.display()))?;
        let mut left = f.size;
        while left > 0 {
            let block = tokio::time::timeout(STALL_TIMEOUT, chan.recv())
                .await
                .context("transfer stalled")??;
            if block.is_empty() || block.len() as u64 > left {
                bail!("data for {:?} does not match its size", f.name);
            }
            file.write_all(&block)
                .await
                .with_context(|| format!("writing {}", path.display()))?;
            left -= block.len() as u64;
        }
        file.flush().await?;
        out.push(FileRef {
            name,
            size: f.size,
            path,
        });
    }
    Ok(out)
}

/// `name`, or `name (2)` and so on if a file copied along with it (from
/// another folder) already took it. Case-insensitive, like most desktops.
fn unique_name(name: &str, taken: &mut HashSet<String>) -> String {
    let (stem, ext) = match name.rfind('.') {
        Some(i) if i > 0 => name.split_at(i),
        _ => (name, ""),
    };
    let mut candidate = name.to_owned();
    let mut n = 2;
    while !taken.insert(candidate.to_lowercase()) {
        candidate = format!("{stem} ({n}){ext}");
        n += 1;
    }
    candidate
}

fn new_folder(root: &Path) -> Result<PathBuf> {
    let id: [u8; 8] = rand::random();
    let dir = root.join(hex::encode(id));
    std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    Ok(dir)
}

/// Deletes all but the newest few folders of received files. `keep` was
/// just written and always stays.
fn prune(root: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let mut dirs: Vec<(std::time::SystemTime, PathBuf)> = entries
        .filter_map(|e| e.ok())
        .filter(|e| e.path() != keep && e.file_type().is_ok_and(|t| t.is_dir()))
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    dirs.sort();
    let old = dirs.len().saturating_sub(KEEP_RECEIVED - 1);
    for (_, dir) in dirs.into_iter().take(old) {
        if let Err(e) = std::fs::remove_dir_all(&dir) {
            tracing::debug!("could not delete {}: {e}", dir.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clashing_names_get_numbered() {
        let mut taken = HashSet::new();
        let names: Vec<String> = ["a.txt", "A.txt", "a.txt", "notes", "notes", ".env", ".env"]
            .iter()
            .map(|n| unique_name(n, &mut taken))
            .collect();
        assert_eq!(
            names,
            [
                "a.txt",
                "A (2).txt",
                "a (3).txt",
                "notes",
                "notes (2)",
                ".env",
                ".env (2)"
            ]
        );
    }

    #[test]
    fn bad_file_lists_are_refused() {
        let file = |name: &str, size| WireFile {
            name: name.into(),
            size,
        };
        assert!(check(&[]).is_err());
        assert!(check(&[file("../evil", 1)]).is_err());
        assert!(check(&[file("big.bin", MAX_FILES_BYTES + 1)]).is_err());
        assert!(check(&[file("a", u64::MAX), file("b", 2)]).is_err());
        assert!(check(&[file("ok.txt", 10)]).is_ok());
    }

    #[test]
    fn only_the_newest_folders_are_kept() {
        let root = std::env::temp_dir().join(format!("clip-prune-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut made = Vec::new();
        for _ in 0..5 {
            made.push(new_folder(&root).unwrap());
            std::thread::sleep(Duration::from_millis(20));
        }
        // The folder just written stays even if its time looks older.
        prune(&root, &made[0]);
        let left: Vec<bool> = made.iter().map(|d| d.exists()).collect();
        assert_eq!(left, [true, false, false, true, true]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
