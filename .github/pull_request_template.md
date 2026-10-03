## What changes for someone using ClipCircle

Before:

After:

## How

<!-- A short paragraph on the approach. -->

## Checked

- [ ] `cargo fmt --all --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `bash scripts/check-android-kotlin.sh` (if Android or clip-ffi changed)
- [ ] Tried on real devices (list them):
