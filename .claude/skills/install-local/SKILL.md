---
name: install-local
description: Build the current checkout and install it on this Mac (/Applications) and on the Android phone connected over USB. Use when asked to install, try or test the app on real devices.
---

# Install ClipCircle locally

Install only what was asked for (Mac, phone, or both). Work from the repo root.

## Mac app

1. Run `cd crates/clip-tray && npx @tauri-apps/cli@2 build --bundles app`.
2. Quit the running app: `osascript -e 'quit app "ClipCircle"'`, or `pkill -x clipcircle`.
3. Replace `/Applications/ClipCircle.app` with `target/release/bundle/macos/ClipCircle.app` and run `open /Applications/ClipCircle.app`.
4. If clips from the phone don't arrive, check System Settings → Network → Firewall. The unsigned app must be allowed to accept incoming connections.

## Android phone

1. Run `adb devices`. If no device is listed, ask the user to plug in the phone and allow USB debugging.
2. Build the native library and the bindings (one-time setup: `rustup target add aarch64-linux-android x86_64-linux-android`, `cargo install cargo-ndk`, an NDK, and JDK 17):
   ```sh
   cargo ndk -t arm64-v8a -t x86_64 -o android/app/src/main/jniLibs build -p clip-ffi --release
   cargo run -p clip-ffi --features bindgen --bin uniffi-bindgen -- generate \
     --library target/aarch64-linux-android/release/libclip_ffi.so --language kotlin \
     --out-dir android/app/src/main/java --no-format
   ```
3. Run `cd android && gradle assembleDebug`, then `adb install -r app/build/outputs/apk/debug/app-debug.apk`.
4. If the install fails with a signature mismatch (a release build is installed), ask the user before running `adb uninstall dev.farhanlabib.clipcircle`. Uninstalling wipes the pairing.
5. Never grant READ_LOGS and never turn on Send automatically. The user opts in from the app.

## After

- Delete large build output once installed (`target/`, `android/app/build/`). The Mac's disk is small.
- A fresh install, or a switch between debug and release builds, means the devices must be paired again.
- Commit nothing; installing is not a code change.
