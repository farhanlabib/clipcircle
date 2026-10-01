# Universal Clipboard

Copy on one device, paste on another. Add your devices to a **circle**; any
circle devices on the same network share their clipboard, end-to-end encrypted,
with no server or account.

## Status: milestone 1

Text sync between desktop devices (macOS, Windows, Linux) via the `clipd` CLI.
Images, files, history, a tray app (Tauri) and Android come next. iOS will need
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

Copy text on one device and paste it on the other. `clipd devices` lists the
circle. Use `--state <file>` to run more than one device on one machine.

## How it works

| Piece | Choice |
| --- | --- |
| Identity | Each device has an X25519 key pair; the public key is its identity. |
| Pairing | 6-digit code → SPAKE2 → Noise `NNpsk0`. Wrong codes fail; an attacker gets one guess per attempt. The host shares the circle's member list with the joiner. |
| Discovery | mDNS: `_clipcircle._tcp` (sync, tagged with the circle id) and `_clipcircle-pair._tcp` (while showing a code). |
| Transport | TCP + Noise `XX`. Each side must prove a key that is in its circle, otherwise the connection is dropped. |
| Sync | Poll the clipboard every 500 ms; on change, push to every reachable member. Members gossip the member list so a new device spreads through the circle. |

Code layout:

- `crates/clip-core`: everything platform-independent (`state`, `pairing`, `transport`, `discovery`, `sync`).
- `crates/clipd`: the CLI daemon.

## Known gaps

- The private key is stored in a plain JSON file; it should move to the OS keychain.
- No way to remove a device from a circle yet.
- One connection per push; fine for text, to be revisited for files.
