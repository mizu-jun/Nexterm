---
name: release-flow
description: Nexterm release, CI and packaging mechanics — version tagging, the GitHub Actions release/CI workflows, the WiX v3 MSI installer, the Flatpak build and its vendored cargo sources, and the russh feature flags that keep Windows builds working. Use when cutting a release, bumping the workspace version, or debugging the release/CI/Flatpak/MSI pipelines.
---

# Release Flow

Releases are automated by `.github/workflows/release.yml` and triggered by pushing a version tag (`v*.*.*`). The Windows installer (`.msi`) is built with WiX v3; components are managed in `wix/main.wxs` (`nexterm-client-gpu.exe` is intentionally excluded).

CI is configured at `.github/workflows/ci.yml` and runs on push/PR against `master`. The 3-OS matrix (Linux / macOS / Windows) runs `cargo test`, `cargo clippy -- -D warnings`, and `cargo fmt --check`.

Bump the version in `Cargo.toml` under `[workspace.package] version` only (not in individual crate `Cargo.toml` files). The workspace uses Rust 2024 edition (`edition = "2024"`), so Rust 1.85+ is required. When bumping a minor or major version, also review `docs/PRODUCT.md` (update the "Last reviewed" line and reconcile any shipped or dropped requirements).

The Flatpak build (`.github/workflows/flatpak.yml`) runs on `ubuntu-latest`. Do not use a `container:` block — it disables `apt-get`. `flatpak remote-add`, `flatpak install`, and `flatpak-builder` all require the `--user` flag (CI has no system-level privileges).

The flatpak-builder sandbox is network-isolated, so cargo dependencies are vendored ahead of time into `pkg/flatpak/cargo-sources.json` and referenced from the manifest's `sources` (see the root `CLAUDE.md` for the rule on regenerating it whenever `Cargo.lock` changes). The flatpak CI runs `flatpak-cargo-generator.py` as its first step and diffs against `cargo-sources.json`; mismatches fail the job, catching missed regenerations. The build forces offline mode with `CARGO_NET_OFFLINE=true` + `cargo --offline build`.

For SSH agent authentication on russh 0.59 / 0.60, the loop variable from `request_identities()` is `&AgentIdentity`. `authenticate_publickey_with` takes an `ssh_key::PublicKey`, so call `identity.public_key().into_owned()` (russh 0.58 returned a `PublicKey` directly from `identity.clone()`, but the type changed in 0.59). There were no breaking API changes between 0.59 and 0.60 for our code. In `Cargo.toml`, set `default-features = false, features = ["ring", "rsa", "flate2"]` to avoid the `aws-lc-rs` backend so the project builds on platforms without NASM (e.g. Windows).

When passing preprocessor variables to WiX v3's `candle.exe`, use the `-dName=Value` form (no space). Calling from PowerShell as `-d "Name=Value"` splits into two arguments and yields `CNDL0289`. The correct form is `"-dVersion=$version"`.
