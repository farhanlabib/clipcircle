# Universal Clipboard

Copy on one device, paste on another. Add your devices to a **circle**; any
circle devices on the same network share their clipboard, end-to-end encrypted,
with no server or account.

## Status: milestone 2

Text and image sync between desktop devices (macOS, Windows, Linux) via the
`clipd` CLI, and removing a device from a circle. Files, history, a tray app
(Tauri) and Android come next. iOS will need
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

Copy text or an image on one device and paste it on the other. `clipd devices`
lists the circle; `clipd remove <name or id>` takes a device out of it.
`clipd history` lists the last clips copied or received while `clipd run` was
running (`--copy N` puts one back, `--clear` deletes it, `run --no-history`
turns it off). Only text is kept, at most 50 entries, in a file only you can read. Use `--state <file>` to run more than one device on one machine.

## How it works

| Piece | Choice |
| --- | --- |
| Identity | Each device has an X25519 key pair; the public key is its identity. The private key is kept in the OS keychain (Keychain, Credential Manager, Secret Service). Where none is available it stays in the state file, readable only by you. Set `CLIPD_NO_KEYCHAIN=1` to skip the keychain. |
| Pairing | 6-digit code → SPAKE2 → Noise `NNpsk0`. Wrong codes fail; an attacker gets one guess per attempt. The host shares the circle's member list with the joiner. |
| Discovery | mDNS: `_clipcircle._tcp` (sync, tagged with the circle id) and `_clipcircle-pair._tcp` (while showing a code). |
| Transport | TCP + Noise `XX`. Each side must prove a key that is in its circle, otherwise the connection is dropped. |
| Sync | Poll the clipboard every 500 ms; on change, push to every reachable member. Images travel as PNG. Members gossip the member list so a new device spreads through the circle. |
| Removal | The removed device's key is kept as a tombstone and gossiped with the member list, so other members drop it too and can't re-add it. Pairing the device again lets it back in. |

Code layout:

- `crates/clip-core`: everything platform-independent (`state`, `pairing`, `transport`, `discovery`, `sync`).
- `crates/clipd`: the CLI daemon.

## Known gaps

- On macOS an unsigned `clipd` build asks for Keychain access again after every rebuild; choose "Always Allow".

- A removal only spreads when a member that knows about it syncs with the others, and a removed device that pairs again through a member that hasn't heard yet can be removed again by gossip.
- Images are read from the clipboard on every poll, which costs more than text for big images.
- One connection per push; fine for text, to be revisited for files.
