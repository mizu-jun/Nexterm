# Nexterm Threat Model (STRIDE)

> **Target version**: as of v1.0.2
> **Authored**: Sprint 4-3 (2026-05-10)
> **Methodology**: Microsoft STRIDE
> **Related documents**: [ARCHITECTURE.md](ARCHITECTURE.md) / [SECURITY.md](../SECURITY.md) / [SBOM.md](SBOM.md)

This document systematically enumerates the threats against nexterm using the six STRIDE categories, and makes the current mitigations and residual risk visible.

| Code | Category | Description |
|------|----------|-------------|
| **S** | Spoofing | Identity impersonation |
| **T** | Tampering | Data alteration |
| **R** | Repudiation | Denying actions |
| **I** | Information Disclosure | Leakage of sensitive data |
| **D** | Denial of Service | Service disruption |
| **E** | Elevation of Privilege | Unauthorized privilege escalation |

---

## 1. System overview and trust boundaries

### 1.1 Actors

| Actor | Trust level | Description |
|-------|-------------|-------------|
| Local user | Trusted | The OS user (UID/SID) who launched nexterm |
| Other users on the same host | Untrusted | Other local accounts and services |
| PTY child processes (shell / command output) | Semi-trusted | Run with the user's privileges but the output may originate externally |
| SSH remote hosts | Untrusted by default | Output from the remote is untrusted; initial trust is established via host-key verification |
| Web-terminal clients | Untrusted | Browser-based connections |
| Plugins (WASM) | Sandboxed | Potentially third-party code |
| GitHub (update / SBOM verification) | External | Integrity verified via minisign / SLSA |

### 1.2 Trust boundaries (data-flow view)

```
                                    ┌─────────────────┐
                                    │ GitHub Releases │
                                    │ (signed by      │
                                    │  maintainer)    │
                                    └────────┬────────┘
                                             │ HTTPS + minisign
                                             ▼
┌──────────────────────────┐  IPC      ┌─────────────────┐  PTY  ┌─────────┐
│ Client (nexterm-client-  │◀════════▶│ nexterm-server  │◀═════▶│  Shell  │
│ gpu / nexterm-client-tui)│ (1)      │                 │ (2)   │ (child) │
└──────────────────────────┘          │                 │       └─────────┘
        │                              │                 │
        │ (6) GitHub API               │                 │  SSH  ┌─────────┐
        │                              │                 │◀═════▶│ Remote  │
        ▼                              │                 │ (3)   │  host   │
   Update                              │                 │       └─────────┘
   Checker                             │                 │
                                       │                 │  WS   ┌─────────┐
                                       │                 │◀═════▶│ Browser │
                                       │                 │ (4)   └─────────┘
                                       │                 │
                                       │                 │ WASM  ┌─────────┐
                                       │                 │◀═════▶│ Plugin  │
                                       │                 │ (5)   └─────────┘
                                       └────┬────────────┘
                                            │
                                ┌───────────┼───────────┐
                                ▼           ▼           ▼
                          config.toml  snapshot   recordings
                          + Lua (7)    .json (8)  *.log (9)
```

Each boundary is numbered (1)–(9) and analysed individually below.

---

## 2. STRIDE analysis per boundary

### 2.1 Boundary 1: client ↔ server (local IPC)

**Channel**: Unix domain socket (`$XDG_RUNTIME_DIR/nexterm.sock`) or Windows named pipe (`\\.\pipe\nexterm-<USERNAME>`)
**Protocol**: 4-byte LE length prefix + postcard (`nexterm-proto`; migrated from bincode in Sprint 5-1 / ADR-0006)
**Trust direction**: bidirectional within a single UID

| Threat | Scenario | Existing mitigation | Residual risk |
|--------|----------|---------------------|---------------|
| **S** | Another user connects to someone else's socket/pipe and eavesdrops on a PTY | Unix: validate UID via `SO_PEERCRED` / `getpeereid` and reject other UIDs (`nexterm-server/src/ipc/platform.rs`). Windows: set the named-pipe DACL to allow only the creating user. The Hello message exchanges `client_kind` / `version` | Root can bypass these checks via OS features (the OS itself is trusted) |
| **T** | A postcard message is rewritten in transit | The channel is local UDS / named pipe within the same process boundary, so in-transit tampering is out of scope. Malicious clients sending invalid structs are handled under (E) | — |
| **R** | Client-side commands are not recorded | Web-based access is logged to `access_log` in CSV with rotation (Sprint 3-3). Per-operation logs for local IPC are not collected | If a local-operation audit log is needed, it must be added separately |
| **I** | PTY output leaks to other processes | UID validation only allows the same user. Tmpfs / `$XDG_RUNTIME_DIR` permissions are protected by the OS (typically 0700) | Leaks via swap / core dumps belong at the OS layer (mlock, etc.) |
| **D** | A client sends huge messages to trigger OOM | `validate_msg_len()` checks the 4-byte length prefix right after Hello and disconnects on excess. The receive task drains message-by-message via `tokio::io::AsyncReadExt::read_exact` (Sprint 1, B1) | The exact limit may need re-evaluation as telemetry accrues |
| **E** | A client uses `RecordSession` to write to an arbitrary path | `dispatch_util::validate_recording_path()` only permits paths under `allowed_recording_dirs()` (Sprint 2-2 Phase A). `canonicalize` prevents symlink escapes | Assumes the recording directory itself is not writable by other users (OS permissions) |

**Assessment**: the design boundary is reasonable. Processes under the same UID are treated as one trust domain (standard Unix model).

---

### 2.2 Boundary 2: server ↔ PTY (child process)

**Channel**: portable-pty (Linux: openpty, Windows: ConPTY)
**Trust direction**: server → child (commands) and child → server (output)

| Threat | Scenario | Existing mitigation | Residual risk |
|--------|----------|---------------------|---------------|
| **S** | Take over another user's PTY master | The slave is handed to the child immediately after `fork+exec`; the master fd lives inside the server with the reader thread holding it exclusively | Relies on the kernel PTY implementation |
| **T** | Malicious child output confuses the VT parser | `nexterm-vt` strictly parses VT100 / OSC / DCS / APC, with APC overflow guards (`nexterm-vt/src/lib.rs`). Four `cargo-fuzz` targets run daily (Sprint 3-5) | The full VT specification cannot be exhaustively covered; known bugs are caught and fixed via fuzzing |
| **R** | No record of what a child wrote | `RecordSession` IPC can write frames to a recording file on demand. Always-on recording is opt-in | Environments needing an operation audit should make recording mandatory at start-up |
| **I** | OSC 52 (clipboard write) silently exfiltrates secrets | Sprint 4-1 added consent dialogs for OSC 52 / OSC 9 / OSC 777 / URL open (`SecurityConfig` with `prompt` / `allow` / `deny`). OSC 52 read requests (`?`) are denied; writes are capped at 1 MiB | Setting the write-only path to `allow` lets writes through without consent (operational policy) |
| **D** | A child floods the parser with huge OSC / DCS / Sixel data | OSC is truncated at 16 MiB; APC has a configurable cap. Sixel decoding goes through `image::decode_sixel()` with size limits (the `sixel_decode` fuzz target confirms crash resilience) | Memory grows for sessions that contain very many images (manage via operational limits) |
| **E** | Escape sequences trigger arbitrary code in the server process | The parser is built on the safe-Rust `vte` crate plus the `nexterm-vt` wrapper. `unsafe` blocks are minimal and require comments (per `rules/rust/security.md`) | Undiscovered dependency bugs are caught by `cargo audit` / `cargo deny` |

**Assessment**: child output is treated as "semi-trusted" and is sanitized, capped, and gated by consent UI before display. Sprint 4-1's consent dialogs significantly improved transparency.

---

### 2.3 Boundary 3: server ↔ SSH remote (`nexterm-ssh`)

**Library**: russh 0.60 (fixes GHSA-f5v4-2wr6-hqmg pre-auth DoS)
**Trust direction**: server (acting as client) → remote host (trust established via host-key verification)

| Threat | Scenario | Existing mitigation | Residual risk |
|--------|----------|---------------------|---------------|
| **S** | MITM connecting to a fake server | Host-key verification is mandatory. A known_hosts-equivalent trust-establishment flow is implemented. With SSH agent auth, identities from `request_identities()` are validated against the russh 0.60 API | First-time TOFU (Trust On First Use) is the general SSH problem |
| **T** | Passwords/keys are tampered with | The client-side `PasswordModal` wraps values in `Zeroizing<String>` (Sprint 3-2). The OS keyring is integrated (Service=`nexterm-ssh`, Account=`<user>@<host>`). `host_history.json` never stores passwords | Environments without a keyring (CLI-only servers) risk plaintext storage (operational workaround) |
| **R** | No audit trail for remote operations | `access_log` is web-only. SSH-session activity must be logged on the remote side | nexterm itself does not record SSH client operations by design |
| **I** | Key fingerprints / passwords exfiltrated from memory dumps | `Zeroizing<String>` zeroes memory on Drop. Debugger attachment is in the OS trust boundary | Core-dump suppression should be enforced at OS / startup configuration |
| **D** | Pre-auth DoS in russh ≤ 0.59 (GHSA-f5v4-2wr6-hqmg) | Already upgraded to 0.60 | New russh vulnerabilities will be flagged by `cargo audit` / `cargo deny advisories` |
| **E** | Output from an SSH remote enables local privilege escalation via nexterm | Remote output goes through the same VT parser as (2.2), so OSC consent and size caps apply | Setting consent to `allow` weakens this defence |

**Assessment**: the upgrade to russh 0.60 plus keyring integration substantially improved practical security. A public-key fingerprint review UI in nexterm itself is a future improvement.

---

### 2.4 Boundary 4: server ↔ Web terminal (axum WebSocket + xterm.js)

**Channel**: WSS (TLS) over HTTP/1.1 or HTTP/2
**Authentication**: token auth + OAuth + TOTP (Sprint 1 fixed an OAuth Organization-validation bypass)

| Threat | Scenario | Existing mitigation | Residual risk |
|--------|----------|---------------------|---------------|
| **S** | Guess or steal an auth token | Tokens are CSPRNG-generated. Two-factor with OAuth + TOTP (`web/oauth.rs` + `web/otp.rs`). The OAuth Organization-validation bypass was fixed in Sprint 1 | Token-expiry policy depends on configuration |
| **T** | WebSocket frames are tampered with in transit | TLS is mandatory (`web/tls.rs` loads certificates); the `web` config can require TLS | Misconfigured TLS (exposing HTTP) is an operational concern |
| **R** | Unable to investigate unauthorized access after the fact | `web/access_log.rs` writes CSV with query-string stripping (Sprint 3-3, first half) and rotation (10 MiB / 7 generations / gzip, Sprint 3-3, second half) | Forwarding to a log-collection server is an external configuration |
| **I** | Cookies / Authorization headers leak into logs | Query strings are stripped and standard loggers do not see secrets by design | Custom error-response messages must be audited by operators |
| **D** | OOM under massive concurrent connections | Use axum's built-in connection limit, combined with OS-level `ulimit` | DDoS-specific defences are delegated to the reverse-proxy layer |
| **E** | Arbitrary command execution via the web | Authentication is required and PTYs are bound only after token verification | TOTP/OAuth operational policy (replay prevention, rotation on admin-key leakage) is up to operators |

**Assessment**: critical issues were resolved in Sprints 1 and 3-3. With the web feature disabled (no `web` section in the config), this boundary does not exist and local-only deployments are unaffected.

---

### 2.5 Boundary 5: server ↔ plugins (WASM)

**Runtime**: wasmi (pure-Rust implementation, no JIT)
**API**: `nexterm-plugin` `PLUGIN_API_VERSION = 1`

| Threat | Scenario | Existing mitigation | Residual risk |
|--------|----------|---------------------|---------------|
| **S** | A malicious plugin forges its metadata | Name/version come from a `nexterm_meta` export. The `PluginManager` records the load path | Verifying plugin authors is the user's responsibility (signature verification is a v2 candidate) |
| **T** | A plugin rewrites host memory | wasmi keeps WASM linear memory isolated. Plugins only interact through host functions | Host-function argument validation is performed in `nexterm-server`'s `plugin_dispatch.rs` |
| **R** | Plugin behaviour is not recorded | Load/unload/reload happen over IPC and therefore appear in the `tracing` logs | Detailed API-call history is deferred to a future v2 |
| **I** | A plugin reads another plugin's data | wasmi instances are isolated. The `PluginManager` is protected by `Arc<Mutex<...>>` | Inter-plugin messaging will be strengthened by the v2 API |
| **D** | A plugin loops forever or allocates huge memory | `consume_fuel` caps instruction count; memory is capped at 256 pages (= 16 MiB) (Sprint 1) | The fuel cap may need re-evaluation per use-case |
| **E** | A plugin reaches the filesystem or network | wasmi provides no I/O host functions by default. Each host binding must be added explicitly | Sprint 4-2 (API v2) plans to add "sanitized inputs / PaneId allow list" |

**Assessment**: meets the basic requirements of a WASM sandbox. Sprint 4-2 will further strengthen this with API v2 (with backwards-compatible graceful degradation) before the plugin ecosystem matures.

---

### 2.6 Boundary 6: client ↔ update checker (GitHub Releases)

**Channel**: HTTPS to api.github.com / github.com
**Authentication**: public API (no GitHub PAT required)
**Integrity**: minisign public-key verification + SLSA Build Provenance (Sprint 3-4)

| Threat | Scenario | Existing mitigation | Residual risk |
|--------|----------|---------------------|---------------|
| **S** | A DNS hijack / TLS spoof serves a fake release | All archives are signed and verified with minisign. The public key `NEXTERM_MINISIGN_PUBLIC_KEY` is embedded at build time (option_env!). Verification failures produce a clear error (enabled once keys are configured in operations) | Builds without keys configured skip verification (intended for local dev builds) |
| **T** | The binary is tampered with in transit | Multi-layer verification with minisign + SLSA Provenance; `gh attestation verify` enables external verification | If the minisign private key leaks, key rotation is required (see `rules/common/secret-rotation.md`) |
| **R** | An attacker rewrites releases to fake history | GitHub Releases provides immutable history and Provenance attestations | Compromise of a maintainer account requires a separate response |
| **I** | The update checker sends sensitive data | Only the version string is sent to the GitHub API; no tokens or UIDs are sent | Adding telemetry would require a prior review |
| **D** | Exhausting the GitHub API rate limit | Polling fires once, 5 seconds after start (can be disabled with `auto_check_update = false`) | Bursty access still fits inside the unauthenticated rate limit (60/h) |
| **E** | Arbitrary code execution via a fake release | Integrity is guaranteed by minisign + SLSA; downloads are discarded on verification failure | Same as (S) — depends on key management |

**Assessment**: Sprint 3-4 completed sign-and-verify. Once operational setup (key generation + GitHub Variables/Secrets registration; see the tail of `project_sprint_progress.md`) is done, this is enabled from the first release onwards.

---

### 2.7 Boundary 7: configuration files (config.toml + Lua)

**Location**: `$XDG_CONFIG_HOME/nexterm/` or `%APPDATA%\nexterm\`
**Load order**: defaults → config.toml → config.lua (Lua may override)

| Threat | Scenario | Existing mitigation | Residual risk |
|--------|----------|---------------------|---------------|
| **S** | Another user rewrites the config | OS file permissions (under the user's home directory) | Falls apart if OS permissions are broken |
| **T** | Behaviour is steered by tampered values | Startup schema validation (`nexterm-config/src/schema/`) returns clear errors on type mismatches | Malicious values that pass schema validation (e.g. huge buffer sizes) are handled under (D) |
| **R** | No audit trail of config changes | `arc-swap::ArcSwap<RuntimeConfig>` logs reload timestamps (Sprint 2-5) | Per-field diff logging is a future improvement |
| **I** | Passwords end up in the config | The design policy is that secrets do not belong in the config. SSH passwords are separated into the keyring (Sprint 3-2) | If users hardcode secrets in Lua hooks, leakage is possible (operational policy) |
| **D** | A malicious Lua function loops forever | `mlua` runs on a dedicated OS thread; channel-based communication keeps the main thread responsive (`StatusBarEvaluator` returns the cached value immediately on each per-second re-evaluation) | Lua scripts that deliberately burn 100% CPU need monitoring |
| **E** | Lua makes system calls | The `mlua` configuration restricts parts of the standard library (Sprint 1 sandbox hardening). OS commands can only be invoked through host functions | The sandbox boundary needs continuous review |

**Assessment**: users own their config, so "user misconfiguration" is the dominant risk. Sprint 1's Lua sandbox hardening blocks privilege escalation from a malicious config.

---

### 2.8 Boundary 8: snapshot persistence

**Location**: `$XDG_STATE_HOME/nexterm/snapshot.json` (Linux/macOS) / `%LOCALAPPDATA%\nexterm\snapshot.json` (Windows)
**Schema**: `SNAPSHOT_VERSION = 3` (auto-migrates v1 / v2; Sprint 5-7 / Phase 2-1 added `workspace_name`)

| Threat | Scenario | Existing mitigation | Residual risk |
|--------|----------|---------------------|---------------|
| **S** | An attacker forges a snapshot and hijacks session restore | OS file permissions plus load only when launched under the same UID | Same OS-trust assumption as (1.1) |
| **T** | DoS by abusing a schema mismatch | Version validation (snapshots with `SNAPSHOT_VERSION` < 1 are rejected; v1/v2 → v3 migration) plus atomic write (added in Sprint 1) | A future-version snapshot opened by an old nexterm produces a clear error |
| **R** | Tampered session history goes undetected | The snapshot is not an audit log (only the last state is stored) | Use `RecordSession` for audit needs |
| **I** | Command history and path information are included | Protected via the OS home-directory permissions | Outbound transfer via backup software is an operational responsibility |
| **D** | Server startup is delayed by a huge snapshot | Parsing has size limits and timeouts | Snapshot bloat should be re-evaluated in operation |
| **E** | Snapshot restore induces arbitrary command execution | The snapshot contains only PTY startup arguments. Auto-restart depends on user configuration | The UI explicitly indicates that "always restore" will launch without further consent |

**Assessment**: Sprint 1 made snapshot writes atomic (write tempfile → rename). Symlink-based overwrite attacks are mitigated via the OS trust assumption.

---

### 2.9 Boundary 9: recording files

**Location**: user-specified path (default `$XDG_DATA_HOME/nexterm/recordings/`)
**Format**: custom frame format (timestamp + byte payload)

| Threat | Scenario | Existing mitigation | Residual risk |
|--------|----------|---------------------|---------------|
| **S** | Another user swaps the recording path | `dispatch_util::validate_recording_path()` permits writes only under the allowed directories (Sprint 2-2 Phase A) | Directory permissions themselves remain OS-dependent |
| **T** | Recording files are modified | OS permissions plus a file lock while writing (platform-dependent) | If post-archive tampering detection is needed, hash records should be kept separately |
| **R** | "I never recorded that" | Recording start/stop fire over IPC and are present in `tracing` logs | Detailed operation audit needs a separate aggregation log |
| **I** | A recording includes typed passwords | PTY echo is recorded as-is, so password keystrokes can appear in the file | The UI could warn that recording is on while a password prompt is focused |
| **D** | A huge recording fills the disk | Writes are limited to allowed directories; rotation is on the user | Auto-rotation is a future improvement |
| **E** | Parser bugs in a recording-playback tool yield host access | nexterm itself does not auto-play recordings (only `nexterm-ctl record` produces them) | Third-party players need separate review |

**Assessment**: path-traversal defence was completed in Sprint 2-2. From an info-disclosure angle, "warn when recording during a password prompt" is a UX improvement candidate.

---

## 3. Residual risks and remediation plan, by priority

### 3.1 Targeted by the upcoming sprints

| ID | Residual risk | Target sprint |
|----|---------------|---------------|
| RR-1 | Plugin API v1 has limited input sanitization | Sprint 4-2 (API v2, PaneId allow list, graceful degradation) |
| RR-2 | Property tests for the Sixel parser and BSP layout are sparse | Sprint 4-4 (introduce proptest) |

### 3.2 Medium-/long-term improvements

| ID | Residual risk | Plan | Priority |
|----|---------------|------|----------|
| RR-3 | Operationalise the minisign public key (key generation + Variables/Secrets) | Operations documented at the end of `project_sprint_progress.md` | High |
| RR-4 | OSC pass-through risk when consent is set to `allow` | Strengthen the "always allow" risk warning in the settings UI | Medium |
| RR-5 | Warn during password input while recording | Blink a recording indicator in both TUI and GPU clients | Medium |
| RR-6 | Monitor CPU usage of Lua scripts | Measure frame time inside `nexterm-lua-worker` and warn when the threshold is exceeded | Low |
| RR-7 | Audit log for local-IPC operations | Implement an optional audit log (extend `access_log`) | Low |
| RR-8 | Plugin signature verification | API v2 + plugin signature manifest (e.g. cosign) | Low |

### 3.3 Accepted (delegated to OS / external dependencies)

| ID | Risk | Reason accepted |
|----|------|-----------------|
| RA-1 | Processes under the same UID share one trust domain | Standard Unix / Windows model |
| RA-2 | OS root / SYSTEM can bypass every boundary | OS itself is in the trust base |
| RA-3 | Secrets leaking through core dumps / swap | Handled by OS settings (`prctl(PR_SET_DUMPABLE)` / `mlock`) |
| RA-4 | TLS MITM via a compromised trust store | The OS-provided trust store is trusted |
| RA-5 | Compromise of GitHub accounts | Mitigated through repository operations: 2FA, branch protection |

---

## 4. Continuous security operations

| Activity | Frequency | Tool | Sprint |
|----------|-----------|------|--------|
| License / vulnerability check on dependencies | Every PR / push | `cargo deny` (`deny.toml`) | 4-3 |
| Match against RustSec Advisory DB | Every PR / push | `cargo audit` | existing |
| Fuzz testing | Daily (UTC 03:00) | `cargo-fuzz`, 4 targets in parallel, 60 s | 3-5 |
| SBOM generation | On each release tag | `cargo-cyclonedx` (`.github/workflows/sbom.yml`) | 4-3 |
| SLSA Build Provenance | On each release tag | `actions/attest-build-provenance@v2` | 3-4 |
| minisign signing | On each release tag (after keys are set) | `minisign -S` | 3-4 |
| Property testing | Every PR / push (planned) | `proptest` | 4-4 (planned) |

---

## 5. Glossary

| Term | Description |
|------|-------------|
| **STRIDE** | Spoofing / Tampering / Repudiation / Information Disclosure / Denial of Service / Elevation of Privilege — Microsoft's threat-classification framework |
| **TOFU** | Trust On First Use — trust the first-seen key and warn on key changes (same as SSH known_hosts) |
| **SLSA** | Supply-chain Levels for Software Artifacts (levels L1 through L4) |
| **CycloneDX** | The OWASP-standardised SBOM format |
| **TOTP** | Time-based One-Time Password (RFC 6238) |
| **minisign** | A lightweight signing tool from the OpenBSD ecosystem (Ed25519-based) |

---

## 6. Revision history

| Date | Version | Changes | Sprint |
|------|---------|---------|--------|
| 2026-05-10 | 1.0 | Initial release (covers mitigations from Sprints 1 – 4-1) | Sprint 4-3 |
