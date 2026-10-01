# Universal Clipboard

Copy on one device, paste on another. Add your devices to a **circle**; any
circle devices on the same network share their clipboard, end-to-end encrypted,
with no server or account.

## Status

Text, image and file sync between desktop devices (macOS, Windows, Linux),
with the `clipd` CLI or the tray app, plus an Android app.
Devices can be removed from a circle, and recent clips are kept in a local
history. iOS will need
the app to be opened (or a Share/Shortcut action) to send, because iOS does not
let apps read the clipboard in the background.

## Try it

```sh
cargo build --release
# Device A
clipd pair            # prints a 6-digit code
# Device B (same Wi-Fi)
clipd join 123456
# Both devices
clipd run
```

Copy text, an image or files on one device and paste on the other. Files
(not folders) copied in Finder, Explorer or a Linux file manager are sent
when they total 32 MiB or less; on the other device they are saved under the
temp folder and pasting in the file manager copies them where you want. `clipd devices`
lists the circle; `clipd remove <name or id>` takes a device out of it.
`clipd history` lists the last clips copied or received while `clipd run` was
running (`--copy N` puts one back, `--clear` deletes it, `run --no-history`
turns it off). Only text is kept, at most 50 entries, in a file only you can read. Use `--state <file>` to run more than one device on one machine.

If a device doesn't get clips, run `clipd doctor`. It lists this device's
network addresses, whether something is syncing here, and for each device in
the circle whether it was found on the network and answered, with a hint when
not (firewall, router blocking discovery, app not running, removed). On
networks that block discovery, `clipd doctor --addr 192.168.1.20` checks a
device directly. The tray app shows a green dot next to devices seen on the
network and has a **Check connections** button that runs the same check.

## Tray app

`crates/clip-tray` is a menu bar (macOS) / system tray (Windows, Linux) app
built with Tauri. It runs syncing in the background and has a small window to
add a device (shows a pairing code), join a circle (type a code), remove
devices, pause syncing, and click a recent clip to copy it again. Turn on
**Start at login** (in the window or the tray menu) to have it start with
your computer; it is off until you turn it on.

```sh
cargo run -p clip-tray --release
```

It uses the same state file as `clipd` (`CLIPD_STATE` overrides the path), so
don't run both at once. On Linux it needs the WebKitGTK and appindicator
libraries; see `.github/workflows/ci.yml` for the package list.

### Installers

The **Installers** workflow builds the tray app for every push to `main`:
open the latest run under Actions and download the artifact for your system.

| Artifact | Contains |
|---|---|
| `universal-clipboard-macos` | `.dmg` (Apple silicon and Intel) |
| `universal-clipboard-windows` | `.msi` and setup `.exe` |
| `universal-clipboard-linux` | `.deb` and `.AppImage` |

The builds are not signed yet. On macOS, right-click the app and choose
**Open** the first time (or run `xattr -dr com.apple.quarantine
"/Applications/Universal Clipboard.app"`). On Windows, click **More info**,
then **Run anyway** in the SmartScreen prompt.

To build one locally: `cd crates/clip-tray && npx @tauri-apps/cli@2 build`.

## Android app

`android/` is a small Android app (Android 10+) on top of `crates/clip-ffi`,
the UniFFI bindings to the same Rust core. It keeps syncing in a foreground
service, so text and images from your other devices land on the phone's
clipboard. Files copied on a computer are saved to Downloads/Universal
Clipboard, put on the clipboard, and announced in a notification. Android only
lets the app in front read the clipboard, so to send from the phone tap **Send
clipboard** in the app or the notification, or share text, an image or files
(up to 32 MB) to **Send to my devices**. Photos larger than 4096 pixels on
a side are scaled down before sending.

CI builds a debug APK on every push (download it from the run's artifacts).
To build locally you need the Android SDK and NDK, `cargo-ndk`, and Gradle 8.11;
the steps are the `android` job in `.github/workflows/ci.yml`.

## How it works

| Piece | Choice |
| --- | --- |
| Identity | Each device has an X25519 key pair; the public key is its identity. The private key is kept in the OS keychain (Keychain, Credential Manager, Secret Service). Where none is available it stays in the state file, readable only by you. Set `CLIPD_NO_KEYCHAIN=1` to skip the keychain. |
| Pairing | 6-digit code → SPAKE2 → Noise `NNpsk0`. Wrong codes fail; an attacker gets one guess per attempt. The host shares the circle's member list with the joiner. |
| Discovery | mDNS: `_clipcircle._tcp` (sync, tagged with the circle id) and `_clipcircle-pair._tcp` (while showing a code). |
| Transport | TCP + Noise `XX`. Each side must prove a key that is in its circle, otherwise the connection is dropped. |
| Sync | Poll the clipboard every 500 ms; on change, push to every reachable member. Images travel as PNG, files as their bytes with a bare file name (names that could escape a folder are refused). Members gossip the member list so a new device spreads through the circle. |
| Removal | The removed device's key is kept as a tombstone and gossiped with the member list, so other members drop it too and can't re-add it. Pairing the device again lets it back in. |

Code layout:

- `crates/clip-core`: everything platform-independent (`state`, `pairing`, `transport`, `discovery`, `sync`).
- `crates/clipd`: the CLI daemon.
- `crates/clip-tray`: the tray app (Tauri; the window is plain HTML/JS in `ui/`).
- `crates/clip-ffi` and `android/`: the Android app.

## Known gaps

- On macOS an unsigned `clipd` build asks for Keychain access again after every rebuild; choose "Always Allow".

- A removal only spreads when a member that knows about it syncs with the others, and a removed device that pairs again through a member that hasn't heard yet can be removed again by gossip.
- Images are read from the clipboard on every poll, which costs more than text for big images.
- One connection per push, and files travel in one message, so file sync is capped at 32 MiB.
