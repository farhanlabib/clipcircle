# ClipCircle

**Copy on one device, paste on another.** Put your computers and phone in a
**circle**, and whatever you copy on one is ready to paste on the others while
they're on the same network. Text, images and files, end-to-end encrypted,
with no server and no account.

It works like Apple's Universal Clipboard, but across macOS, Windows, Linux
and Android.

## Download

| System | Installer |
|---|---|
| macOS 11+ (Apple silicon and Intel) | [ClipCircle-macos.dmg](https://github.com/farhanlabib/clipcircle/releases/latest/download/ClipCircle-macos.dmg) |
| Windows 10/11 | [ClipCircle-windows-setup.exe](https://github.com/farhanlabib/clipcircle/releases/latest/download/ClipCircle-windows-setup.exe) ([.msi](https://github.com/farhanlabib/clipcircle/releases/latest/download/ClipCircle-windows.msi)) |
| Android 10+ | [ClipCircle-android.apk](https://github.com/farhanlabib/clipcircle/releases/latest/download/ClipCircle-android.apk) |
| Linux (x86-64) | [AppImage](https://github.com/farhanlabib/clipcircle/releases/latest/download/ClipCircle-linux-amd64.AppImage) or [.deb](https://github.com/farhanlabib/clipcircle/releases/latest/download/ClipCircle-linux-amd64.deb) |

All versions are on the [Releases](https://github.com/farhanlabib/clipcircle/releases) page.

The desktop builds are not signed with a paid certificate yet, so the first
launch needs one extra click:

- **macOS:** open the app once, then go to System Settings → Privacy &
  Security and click **Open Anyway**.
- **Windows:** in the SmartScreen prompt click **More info**, then **Run anyway**.
- **Android:** allow your browser or file manager to install unknown apps when asked.

## Getting started

1. Install ClipCircle on two devices on the same Wi-Fi or LAN.
2. On the first one, choose **Add a device**. It shows a 6-digit code.
3. On the second one, choose **Join a circle** and type the code.
4. Copy something on one device and paste it on the other.

Add more devices the same way, from any device already in the circle. On a
computer ClipCircle lives in the menu bar (macOS) or system tray (Windows,
Linux); turn on **Start at login** to keep it running.

## Features

- **Text, images and files.** Files (not folders) copied in Finder, Explorer
  or a Linux file manager are sent, up to 4 GiB at a time, streamed from disk
  to disk. Paste them in the file manager on the other device.
- **Android.** Text and images from your computers land on the phone's
  clipboard; files go to Downloads/ClipCircle. Android only lets the app in
  front read the clipboard, so to send from the phone tap **Send clipboard**
  (in the app or its notification), or share text, images or files to
  **Send to my devices**.
- **Private by design.** Devices pair with a short code and then talk only to
  each other, encrypted. Nothing goes through a server.
- **Recent clips.** The last 50 text clips are kept locally; click one to copy
  it again.
- **Connection check.** When a device doesn't get clips, **Check connections**
  (or `clipd doctor`) says what's wrong: a firewall, a router that blocks
  discovery, the app not running.

iOS isn't supported: iOS doesn't let apps read the clipboard in the background.

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

- `crates/clip-core`: everything platform-independent (state, pairing,
  transport, discovery, sync, file transfer).
- `crates/clipd`: the command-line daemon.
- `crates/clip-tray`: the desktop app (Tauri; the window is plain HTML/JS in `ui/`).
- `crates/clip-ffi` and `android/`: the Android app, on UniFFI bindings to the
  same Rust core.

## Command line

`clipd` does everything the desktop app does, for servers or scripting:

```sh
cargo build --release -p clipd
clipd pair            # device A: prints a 6-digit code
clipd join 123456     # device B, same network
clipd run             # both: sync until stopped
```

- `clipd devices` lists the circle; `clipd remove <name or id>` takes a device out.
- `clipd history` lists recent clips (`--copy N` puts one back, `--clear`
  deletes them, `run --no-history` turns history off).
- `clipd doctor` checks the network and every device in the circle.
  `--addr 192.168.1.20` checks one device directly on networks that block
  discovery; pass the same `--port` you gave `run`.
- `--state <file>` runs more than one device on one machine.

Received files are saved under the temp folder (the last three batches are
kept). The desktop app and `clipd` share their state file (`CLIPD_STATE`
overrides the path), so don't run both at once.

## Building from source

You need [Rust](https://rustup.rs) (stable).

```sh
cargo run -p clip-tray --release                     # the desktop app
cd crates/clip-tray && npx @tauri-apps/cli@2 build   # its installer
```

On Linux the desktop app needs the WebKitGTK and appindicator libraries; see
`.github/workflows/ci.yml` for the package list.

The Android app needs the Android SDK and NDK, `cargo-ndk` and Gradle 8.11;
the steps are the `android` job in `.github/workflows/release.yml`.

## Contributing

Issues and pull requests are welcome; see [CONTRIBUTING.md](CONTRIBUTING.md).

## Known gaps

- On macOS an unsigned `clipd` build asks for Keychain access again after
  every rebuild; choose "Always Allow".
- A removal only spreads when a member that knows about it syncs with the
  others, and a removed device that pairs again through a member that hasn't
  heard yet can be removed again by gossip.
- Images are read from the clipboard on every poll, which costs more than text
  for big images.
- A big file goes to each device separately, and there is no progress shown yet.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT), at your option.

Unless you explicitly state otherwise, any contribution you intentionally
submit for inclusion in this project, as defined in the Apache-2.0 license,
shall be dual licensed as above, without any additional terms or conditions.
