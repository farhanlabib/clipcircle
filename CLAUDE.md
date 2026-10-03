# ClipCircle: notes for working on this repo

ClipCircle is a cross-platform universal clipboard. Devices pair into a
**circle** with a 6-digit code. After that, whatever is copied on one device can
be pasted on the others while they're on the same LAN. It handles text,
images and files, end-to-end encrypted, with no server and no account.
Supported: macOS, Windows, Linux, Android. iOS is out of scope because it
can't read the clipboard in the background.

Owner: Md. Farhan Labib (GitHub `farhanlabib`). The repo is public: github.com/farhanlabib/clipcircle.

## Layout

| Path | What it is |
|---|---|
| `crates/clip-core` | All platform-independent logic. `state` (device keys, members, tombstones), `pairing` (SPAKE2 → Noise NNpsk0), `discovery` (mDNS `_clipcircle._tcp`, `_clipcircle-pair._tcp`), `transport` (Noise XX over TCP, u16-length frames), `sync` (the `Engine`: poll clipboard, push to members, apply incoming, gossip members, `check_members`), `transfer` (files streamed disk to disk, 4 GiB cap), `history` (last 50 clips, text kept in full), `keychain`, `clipboard` (`Clip`, `Clipboard` trait, arboard impl), `service` (`Service::start` = engine + listener + mDNS + pairing; shared by every app). Default port 47800. |
| `crates/clip-core/tests/end_to_end.rs` | Two or more in-process devices pairing and syncing. Add a test here for any protocol change. |
| `crates/clipd` | Command-line daemon: `pair`, `join`, `run`, `devices`, `remove`, `history`, `doctor`. |
| `crates/clip-tray` | Desktop app (Tauri 2, binary `clipcircle`). `src/main.rs` holds the tray, the window and the Tauri commands. `ui/` is plain HTML/CSS/JS with no bundler; styles switch on `data-os` (macos/windows/linux). Per-OS window config is in `tauri.macos.conf.json` (transparent popover, vibrancy) and `tauri.windows.conf.json` (Mica). |
| `crates/clip-ffi` | UniFFI 0.29 bindings for Android: `Node`, `ClipListener`, records `Device`, `DeviceCheck`, `HistoryItem`, `SharedFile`. |
| `android/` | Kotlin app, package `dev.farhanlabib.clipcircle`. Framework Views only (`android.useAndroidX=false`, no Compose). `Look.kt` has the Material 3 palette and building blocks, `MainActivity` the screen, `SyncService` the foreground service, `SendActivity` sends the clipboard or a share, `AutoSend` handles opt-in automatic sending, `Outbox` holds the shared send code. |
| `scripts/check-android-kotlin.sh` | Type-checks the Kotlin code without the Android SDK. |
| `.github/workflows/ci.yml` | Lint, tests on Linux/macOS/Windows, and an Android APK build. |
| `.github/workflows/release.yml` | Builds every installer and publishes a GitHub Release (see Releasing). |
| `.claude/` | Claude Code settings (pre-approved build and test commands) and skills: `install-local` (build and install on this Mac and the USB phone) and `release`. |

The version lives in one place: `version` in the root `Cargo.toml`. Tauri and the Android build (`versionName`, `versionCode = major*10000+minor*100+patch`) read it from there.

## Checks before every push

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace            # on Linux without a display: xvfb-run -a cargo test --workspace --exclude clip-tray
bash scripts/check-android-kotlin.sh   # when Android or clip-ffi changed
```

CI runs the same checks and must be green before merging.

## Running things

- Desktop app: `cargo run -p clip-tray --release`. Installer: `cd crates/clip-tray && npx @tauri-apps/cli@2 build` (`--bundles app` for just the .app on macOS).
- Two devices on one machine: `clipd --state /tmp/a.json run --port 47801`, and the same with another state file and port.
- Android: needs the Rust targets `aarch64-linux-android x86_64-linux-android`, `cargo-ndk`, the NDK, JDK 17 and Gradle 8.11. Then:
  ```sh
  cargo ndk -t arm64-v8a -t x86_64 -o android/app/src/main/jniLibs build -p clip-ffi --release
  cargo run -p clip-ffi --features bindgen --bin uniffi-bindgen -- generate \
    --library target/aarch64-linux-android/release/libclip_ffi.so --language kotlin \
    --out-dir android/app/src/main/java --no-format
  cd android && gradle assembleDebug && adb install -r app/build/outputs/apk/debug/app-debug.apk
  ```
  `jniLibs/` and `java/uniffi/` are generated and ignored by git.
- The desktop app and `clipd` share a state file (`~/Library/Application Support/clipcircle/state.json` on macOS; `CLIPD_STATE` overrides it). Don't run both at once.

## How the pieces fit

- Every app goes through `clip_core::service::Service`, so behaviour stays the same everywhere. Put logic in clip-core, not in an app.
- Desktop apps poll the system clipboard every 500 ms. Android can't read the clipboard in the background, so the app hands clips over itself: `Node::send_text`, `send_image` or `send_files` from `SendActivity` (the Send clipboard button, the notification action, or a share). Incoming clips arrive through `ClipListener` and go on the clipboard via `ClipApp.setOwnClip`.
- Echo guard: clips the circle puts on the clipboard carry a label in `ClipApp.OWN_LABELS` and are never sent back out. The engine also skips a clip whose hash matches the last one sent or received.
- Received files go to a temp folder on desktop and to Downloads/ClipCircle on Android. Received files are never re-sent.

## Product decisions (don't undo without asking Labib)

- **Devices are shown as a list, never a ring.** A ring looks bad with many devices.
- **Automatic sending on Android is opt-in, with informed consent.** Manual Send clipboard is the default. Auto-send turns on only after a sheet explains what it needs: READ_LOGS (granted once over adb, read for the "Denying clipboard access" line only) and display over other apps (a 1×1 window takes focus briefly). Never make it default-on, never grant READ_LOGS for the user, never shorten the consent text.
- **Design** (canvas: https://claude.ai/artifact/AnBdLyRBBaEjFziPdfKBVH):
  - macOS: a tray popover with a Liquid Glass feel.
  - Windows: Mica.
  - Android: Material 3, with a fixed teal palette in `Look.kt` (primary `#006a63` light, `#82d5cc` dark).
  - Desktop accent: `#0b7f78` light, `#45d3c7` dark.
  - The logo and app icon are still to be designed.
- No server, no account, LAN only. Keep it that way unless Labib decides otherwise.

## Conventions

- UI text is plain and short, written for a non-technical person. Say what happened and what to do next ("Not seen on this network. Check that…"), never internal terms.
- Rust: `anyhow` with context for errors, `tracing` for logs, doc comments that say why. Keep `clippy -D warnings` clean.
- UniFFI: error types must be `#[uniffi(flat_error)]`. A field named `message` clashes with Kotlin's `Throwable.message`.
- Android: no AndroidX or third-party UI libraries. Build views in code with the `Look` helpers. New drawables go in `res/drawable` as vectors. The Kotlin check script stubs `R.string` and `R.drawable` only.
- Match the surrounding code's style and comment density.

## Git, commits and releases

- **Commit only as `Md. Farhan Labib <farhan.labib4@gmail.com>`.** No other email may appear in any commit, trailers included.
- Branch, open a PR, squash-merge once CI is green. Labib is fine with merging without a human review.
- **Never commit signing keys** (`*.jks` and `*.keystore` are ignored). The Android release key is in the repo secrets `ANDROID_KEYSTORE_BASE64` and `ANDROID_KEYSTORE_PASSWORD` (alias `clipcircle`, key password = store password).
- **Never add a self-hosted runner.** The repo is public, so anyone's PR could run code on it. Hosted runners are free here.
- **Releasing:**
  1. Bump `version` in the root `Cargo.toml` and run `cargo metadata` so `Cargo.lock` updates, then merge to main.
  2. Push tag `v<version>`, or run the **Release** workflow on main with **publish** ticked.
  3. The workflow builds the dmg, exe/msi, AppImage/deb and signed APK, plus `SHA256SUMS.txt`.
  4. Asset names never change, so the README's `releases/latest/download/...` links keep working.
- Release builds are signed with the release key, while local and CI builds use the debug key. Switching a phone between the two needs an uninstall.

## Known issues and gotchas

- Desktop builds are ad-hoc signed only. On macOS the first launch needs System Settings → Privacy & Security → Open Anyway. The macOS firewall may also block incoming connections to an unsigned app, so a phone → Mac send fails while Mac → phone works. Check this first when "sending from the phone doesn't arrive".
- An unsigned `clipd` asks for Keychain access again after each rebuild (`CLIPD_NO_KEYCHAIN=1` skips the keychain).
- Some routers block mDNS (client/AP isolation). `clipd doctor --addr <ip>` checks a device directly.
- Labib's Mac has limited free disk. Delete `target/` and Android build output after big builds.
- Open: Labib reported (2026-10-02) that manual Send clipboard from the phone didn't reach the Mac. Not yet diagnosed; suspect the firewall point above.
