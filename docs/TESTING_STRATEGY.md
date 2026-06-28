# Testing Strategy

> This document describes the test taxonomy used in Nexterm, the perspectives
> we deliberately cover, and the gaps we are currently aware of. It is the
> Markdown counterpart to the "requirement coverage matrix" pattern described
> in [nexta\_'s QA persona article][zenn-nexta] — we translate the same
> intent to a Rust workspace instead of a 25-column CSV.
>
> [zenn-nexta]: https://zenn.dev/nexta_/articles/be13a2395a5d2a

## 1. Test taxonomy

| Layer | Tool / location | Approx. count (2026-06-28) |
|---|---|---|
| Unit | `#[test]` / `#[tokio::test]` under each crate's `src/` | ~1,236 |
| Integration | `<crate>/tests/*.rs` | 3 files (server×2, vt×1) |
| Property | `proptest!` macros (image decoding, BSP / tiling) | ~3,500 generated cases |
| Fuzz | `cargo +nightly fuzz` (vt_parser / sixel / kitty / osc_url) | 4 targets × 60 s daily in CI |
| Coverage | `cargo llvm-cov` in `.github/workflows/coverage.yml` | workspace minus client-gpu / i18n |

The GPU client (`nexterm-client-gpu`) is excluded from coverage because wgpu /
winit require a display server. Its 600+ unit tests still run in the main CI.

## 2. QA persona × ISO/IEC 25010 matrix

The matrix below maps the seven QA personas (as enumerated in the referenced
article) to ISO/IEC 25010 quality characteristics, with the **concrete Rust
tests** that already cover the intersection. Empty cells are gaps; "MEDIUM"
and "HIGH" tags mark the priority for filling them.

Legend: ✅ covered · ⚠ partial · ✗ gap

### Functionality

| Persona | Status | Where |
|---|---|---|
| 新人ユーザー (Novice user) | ⚠ | `nexterm-client-tui/src` unit tests for keymap; no end-to-end TUI flow yet (MEDIUM) |
| ベテラン現場担当 (Power user) | ✅ | Lua hooks (`nexterm-server::hooks`), macros, palette ranking |
| 仕様懐疑者 (Spec skeptic) | ⚠ | `nexterm-vt` unit tests cover common CSI / OSC; DEC private modes partial (MEDIUM) |

### Reliability

| Persona | Status | Where |
|---|---|---|
| データ整合性監査役 | ✅ | proptest for BSP / tiling layout invariants; snapshot round-trip |
| 移行担当者 | ⚠ | snapshot v1→v3 migration tested; v3→v4 covered indirectly. Round-trip per version pending (covered in this strategy expansion) |
| 回帰デグレ番人 | ⚠ | No golden / snapshot library yet. `insta` adoption is staged for a follow-up sprint |

### Security

| Persona | Status | Where |
|---|---|---|
| 悪意ある操作者 | ⚠ | VT / Sixel / Kitty / OSC fuzz running; IPC postcard fuzz pending (HIGH — addressed in this expansion) |
| 悪意ある操作者 (Web auth) | ⚠ | OAuth / OTP / token unit tests exist; adversarial path (expired, replay, tampered) needs explicit cases (MEDIUM) |
| データ整合性監査役 (Sandboxing) | ✅ | WASM `consume_fuel(true)`, Lua sandbox, IPC `MAX_MSG_LEN` enforced and tested |

### Performance efficiency

| Persona | Status | Where |
|---|---|---|
| ベテラン現場担当 | ✗ | No committed `criterion` benches. Tracked as a follow-up (LOW) |

### Compatibility

| Persona | Status | Where |
|---|---|---|
| 仕様懐疑者 | ⚠ | vt100 / VT220 / xterm subset covered by unit tests; no formal vttest harness (LOW) |

### Maintainability

| Persona | Status | Where |
|---|---|---|
| 回帰デグレ番人 | ✅ | `cargo fmt --check`, `cargo clippy -- -D warnings`, cargo-deny in CI |

### Portability

| Persona | Status | Where |
|---|---|---|
| 移行担当者 | ✅ | 3-OS matrix (Linux / macOS / Windows) in `.github/workflows/ci.yml` |

### Usability / Accessibility

| Persona | Status | Where |
|---|---|---|
| 新人ユーザー | ⚠ | AccessKit integration tested at unit level; manual screen-reader verification pending |

## 3. Test Basis (一次情報の出典)

Following the article's "Test Basis must cite primary sources" rule, the
specifications below are the authoritative references for each subsystem.
When a test asserts behaviour, the test name or a doc comment should point
back to the relevant clause here.

- **VT escape sequences**: ECMA-48; xterm `ctlseqs.txt`; DEC VT220 Programmer
  Reference (Digital, EK-VT220-RM).
- **Sixel**: DEC STD 070, "Sixel Graphics".
- **Kitty graphics / keyboard protocol**: <https://sw.kovidgoyal.net/kitty/graphics-protocol/>
  and <https://sw.kovidgoyal.net/kitty/keyboard-protocol/>.
- **OSC 8 (hyperlinks)**: <https://gist.github.com/egmontkovacs/a487f133b1cd5c91d8eb>.
- **OSC 133 (semantic shell prompts)**: <https://gitlab.freedesktop.org/Per_Bothner/specifications/-/blob/master/proposals/semantic-prompts.md>.
- **postcard wire format**: <https://github.com/jamesmunns/postcard> (version pinned in `Cargo.lock`).
- **SSH**: RFC 4252 / 4253 / 4254; `russh` 0.60 API.

When a test description says "推測" (speculation), mark the test
`#[ignore = "spec unverified"]` and link the related issue.

## 4. Coverage operations

The `Coverage` workflow runs on every push / PR and posts an `llvm-cov`
summary as a PR comment. The current goal is to **make coverage visible**;
once we have ~3 PR cycles of data we will agree on a numeric floor in a
follow-up ADR (no hard threshold is enforced today to avoid optimising for
the metric over the code).

To reproduce locally:

```bash
cargo install cargo-llvm-cov
cargo llvm-cov --workspace \
  --exclude nexterm-client-gpu \
  --exclude nexterm-i18n \
  --html
# open target/llvm-cov/html/index.html
```

## 5. Reviewer checklist

Before merging a non-trivial PR, the reviewer confirms:

- [ ] At least one new test exercises the new behaviour or the bug being fixed.
- [ ] If a fuzz seed reproduced a panic, the regression test is added to the
  corresponding crate's `src/` (so it runs in normal CI, not only `cargo fuzz`).
- [ ] If the change crosses a protocol boundary (`nexterm-proto`,
  `nexterm-server::ipc`, snapshot schema), an integration test under
  `<crate>/tests/` covers the new wire shape.
- [ ] User-facing strings exist in all eight locale files (see `nexterm-i18n`).

## 6. Known gaps (snapshot 2026-06-28)

These are tracked so future contributors do not need to rediscover them:

1. **No `insta` (or equivalent) snapshot library.** Settings TOML
   write-back and palette rendering would benefit; deferred to keep this
   sprint's diff focused.
2. **No committed `criterion` benches.** `docs/benchmarks.md` exists as a
   manual record; promoting to CI is a separate decision.
3. **No vttest-based compliance run.** External tool; left as a manual
   release-checklist item.
4. **GPU client (`nexterm-client-gpu`) coverage is unreported.** Headless
   wgpu testing is fragile in CI; we keep its unit tests in `cargo test`
   but exclude it from `llvm-cov`.
