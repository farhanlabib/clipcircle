# Contributing to ClipCircle

Thanks for helping. Bug reports, ideas and pull requests are all welcome.

## Reporting a bug

Open an issue with what you did, what you expected and what happened, plus the
systems involved (for example "macOS 15 → Android 14"). If clips don't arrive,
include the output of **Check connections** in the app or `clipd doctor`.

Security problems should not go in a public issue; see [SECURITY.md](SECURITY.md).

## Making a change

1. Fork the repository and create a branch.
2. Make the change, with a test where it makes sense.
3. Run the checks below.
4. Open a pull request saying what changes for someone using the app.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
bash scripts/check-android-kotlin.sh   # type-checks the Android app, no SDK needed
```

On a desktop (or Linux with `xvfb-run`),
`cargo test -p clip-core --features system-clipboard -- --ignored` also checks
the real clipboard.

CI runs the same checks on Linux, macOS and Windows and builds the Android app.

## Releasing

Maintainers bump `version` in the root `Cargo.toml`, merge, and then either
push a tag `v<version>` or run the **Release** workflow on `main` with
**publish** ticked. It builds every installer and publishes them on the
Releases page.

## License

By contributing you agree that your contributions are licensed under the
project's MIT OR Apache-2.0 dual license.
