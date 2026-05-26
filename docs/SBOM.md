# SBOM (Software Bill of Materials) Operations Guide

> Introduced in Sprint 4-3. Nexterm generates a CycloneDX SBOM for every release and attaches it to the GitHub Release.

## Goals

- **Supply-chain transparency**: users can obtain a machine-readable list of every dependency, version, and license shipped inside nexterm.
- **Vulnerability tracking**: when a new CVE is published later, the SBOM can be scanned to determine whether nexterm is affected.
- **Compliance**: satisfies the dependency-inventory requirements of ISO 27001 and SLSA L2-equivalent processes.
- **Auditability**: third parties can compare the published SBOM against the released binaries.

## Format

We use [CycloneDX](https://cyclonedx.org/) JSON v1.5 because:

- It is the de-facto standard in the Rust ecosystem (`cargo-cyclonedx` is officially maintained).
- It has wide tooling support (OSV Scanner, Trivy, Dependency-Track, …).
- The full document is plain JSON, which is easy to process programmatically.

If you need SPDX format, `cyclonedx-cli` can convert between the two.

## Generation triggers

| Trigger | Behaviour |
|---------|-----------|
| `v[0-9]+.[0-9]+.[0-9]+` tag push | `.github/workflows/sbom.yml` runs: generate SBOM → attach SLSA Provenance → attach the `.tar.gz` to the GitHub Release |
| `workflow_dispatch` (manual) | Generate SBOM only — retained as an artifact for 90 days, not attached to a Release |

## Release attachment

Each release ships a single archive named `nexterm-sbom-vX.Y.Z.tar.gz`. Extracting it yields the SBOM for each of the 11 workspace crates (12 crates up to v1.3.1; `nexterm-launcher` was removed in v1.4.0):

```
nexterm-sbom-v1.4.0/
  nexterm-client-gpu.cdx.json          # binary name: nexterm (single binary)
  nexterm-server.cdx.json
  nexterm-client-tui.cdx.json
  nexterm-client-core.cdx.json
  nexterm-ctl.cdx.json
  nexterm-config.cdx.json
  nexterm-vt.cdx.json
  nexterm-proto.cdx.json
  nexterm-i18n.cdx.json
  nexterm-ssh.cdx.json
  nexterm-plugin.cdx.json
```

## Local generation

```bash
# One-time setup
cargo install cargo-cyclonedx --locked

# Generate SBOMs for every workspace crate (JSON)
cargo cyclonedx --all --format json

# Each crate directory will contain <crate-name>.cdx.json
find . -name '*.cdx.json' -not -path './target/*'
```

## Verification

### 1. File integrity

The release archive ships with SLSA Provenance attached. Verify it with the `gh` CLI:

```bash
gh attestation verify nexterm-sbom-v1.0.2.tar.gz -R mizu-jun/Nexterm
```

If the release also ships a minisign signature (once that flow is enabled in operations):

```bash
minisign -V -p nexterm.pub -m nexterm-sbom-v1.0.2.tar.gz -x nexterm-sbom-v1.0.2.tar.gz.minisig
```

### 2. Inspecting SBOM contents

CycloneDX JSON can be parsed with any standard tooling:

```bash
# List every component's purl with jq
jq -r '.components[].purl' nexterm-server.cdx.json

# Check the version of a specific crate
jq '.components[] | select(.name == "ring")' nexterm-server.cdx.json
```

### 3. Known-vulnerability scanning

Scan the SBOM directly with [OSV Scanner](https://github.com/google/osv-scanner):

```bash
osv-scanner --sbom=nexterm-server.cdx.json
```

[Trivy](https://github.com/aquasecurity/trivy) works as well:

```bash
trivy sbom nexterm-server.cdx.json
```

### 4. License summary

```bash
jq -r '.components[] | "\(.name)\t\(.version)\t\(.licenses[]?.license.id // .licenses[]?.license.name // "Unknown")"' nexterm-server.cdx.json | sort -u
```

## CI guardrails

Even for PRs that are not releases, dependency policy is enforced continuously through `cargo-deny` (see `deny.toml`). SBOMs serve as the artefact of record for releases, while `cargo-deny` provides up-front detection on every change — a two-layered design.

| Tool | When it runs | Role |
|------|--------------|------|
| `cargo-deny` (the `deny` job in `.github/workflows/ci.yml`) | Every PR / push | Block license violations, known vulnerabilities, and disallowed sources up front |
| `cargo-audit` (the `security` job, same workflow) | Every PR / push | Cross-check against the RustSec Advisory DB |
| `cargo-cyclonedx` (`.github/workflows/sbom.yml`) | On release tag | Generate the SBOM and attach it to the release |
| SLSA Provenance | On release tag | Detect tampering of the build origin |
| `minisign` | On release tag (only when keys are configured) | Verify archive integrity |

## Troubleshooting

### `cargo cyclonedx` fails dependency resolution

This is caused by missing native libraries (X11, ALSA, libudev, etc.). Install the same Linux build dependencies that CI uses:

```bash
sudo apt-get install -y libx11-dev libxkbcommon-dev libwayland-dev libasound2-dev libpulse-dev libudev-dev
```

### The SBOM is large

The full workspace produces 12 files × a few hundred KB each. The compressed `.tar.gz` is typically 100–300 KB. If size grows unexpectedly, check for duplicate versions (detectable via `cargo deny check bans`).

### `cargo-cyclonedx` version compatibility

CycloneDX spec v1.5 output requires `cargo-cyclonedx` 0.5.0 or newer. CI uses `cargo install --locked` so the version is pinned via `Cargo.lock`, which gives us reproducibility.

## References

- [CycloneDX specification](https://cyclonedx.org/specification/overview/)
- [SLSA Build Provenance](https://slsa.dev/spec/v1.0/provenance)
- [OSV Scanner documentation](https://google.github.io/osv-scanner/)
- [RustSec Advisory Database](https://rustsec.org/)
