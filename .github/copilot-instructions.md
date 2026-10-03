# Instructions for AI assistants on GitHub

Follow [CLAUDE.md](../CLAUDE.md) at the repo root. The essentials:

- Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`
  and `cargo test --workspace` before pushing. Also run `bash scripts/check-android-kotlin.sh`
  when `android/` or `crates/clip-ffi` changed.
- Platform-independent logic belongs in `crates/clip-core`; the apps stay thin.
- UI text is plain language for non-technical people.
- Android uses framework Views only (no AndroidX), built with the helpers in `Look.kt`.
- Devices are shown as a list. Automatic sending on Android stays opt-in, behind its consent sheet.
- Never commit signing keys and never add self-hosted runners (this repo is public).
