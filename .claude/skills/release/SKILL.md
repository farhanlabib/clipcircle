---
name: release
description: Publish a new ClipCircle version to GitHub Releases (installers for macOS, Windows, Linux and a signed Android APK). Use when asked to release, ship or publish a version.
---

# Release a version

1. Start from an up-to-date `main` with CI green.
2. Bump `version` in the root `Cargo.toml` (semver; Tauri and Android read it from there). Run `cargo metadata --format-version 1 >/dev/null` so `Cargo.lock` updates.
3. Commit "Release X.Y.Z" as `Md. Farhan Labib <farhan.labib4@gmail.com>` and merge it to main.
4. Publish, either way:
   - `git tag vX.Y.Z && git push origin vX.Y.Z`
   - or run the **Release** workflow on main with **publish** ticked (`gh workflow run release.yml -f publish=true`).
5. Watch the run. It builds `ClipCircle-macos.dmg`, `ClipCircle-windows-setup.exe`, `ClipCircle-windows.msi`, `ClipCircle-linux-amd64.AppImage`/`.deb`, `ClipCircle-android.apk` and `SHA256SUMS.txt`, then creates the release with `.github/release-notes.md` on top.
6. Check that the release page lists all seven files. Never rename assets: the README links to `releases/latest/download/<name>`.

Signing secrets `ANDROID_KEYSTORE_BASE64` and `ANDROID_KEYSTORE_PASSWORD` must exist in the repo settings. If the android job fails on signing, tell the owner instead of changing how signing works.
