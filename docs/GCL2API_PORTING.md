# gcl2api-plus → cliproxy-rs migration checkpoint

Status: **Phase 2 — proxy-change invalidation CI-verified; guarded eligibility POST mock path under test**
Last updated: 2026-10-20 (Windows developer node)

## Ground truth

- Python reference: `C:\Users\李小龙\AgentDock\gcl2api-plus-memory-test`
  - Branch `custom`, commit `776430a`.
  - `src/antigravity_error_classifier.py`
  - `src/api/antigravity.py` (`MODEL_FORBIDDEN_COOLDOWN_SECONDS=300`, `VALIDATION_REQUIRED_COOLDOWN_SECONDS=600`)
  - `tests/test_antigravity_account_health.py::Antigravity403ClassifierTests` (8/8 Python tests passed).
- Rust base: `C:\Users\李小龙\AgentDock\gcl2api-rust-port`
  - `cliproxy-rs` v0.2.6, based on `d1261f2`.
  - Local development branch: `feature/gcl2api-antigravity-403-20261019`.
  - Do not push, publish, or copy upstream credentials into tests.

## Phase 1 changes

- `crates/cpa-server/src/antigravity_403.rs`: Python 403 classification priority, decision categories, action flags and 300/600-second durations. Standalone Rust test binary built, 2/2 groups passed.
- `crates/cpa-server/src/antigravity_health.rs`: phase 2 pure-Rust port of Python egress IP echo parsing, TTL freshness, conservative negative-evidence gating, proxy-drift transition, and eligibility classification. Exposed as `cpa_server::antigravity_health`; scheduler reads optional nested `gcl2api_account_health` credential metadata to apply 403 classification precedence. Persistence, proxy-bound HTTP probes, and rebinding still require integration.
- Phase 4 persistence bridge: `antigravity_health::persist_health_update` reuses the Rust `CredentialStore::apply_patch` CAS revision guard plus atomic credential-file persistence for nested health fields, merges only allowlisted health fields, and retains OAuth tokens and unrelated metadata. Mock credential-file tests cover merge, invalid field rejection, and non-Antigravity isolation. This is the persistence primitive, NOT a live network probe, rebind, or Python SQLite schema migration.
- `crates/cpa-exec/src/antigravity_cli_ua.rs`: phase 3 Python CLI User-Agent identity and strict semantic-version floor versus adopted-version comparison. Opt-in by per-credential `gcl2api_cli_version` and/or `gcl2api_cli_adopted_version`; optional `gcl2api_cli_os_type`, `gcl2api_cli_arch`, `gcl2api_cli_auth_method`. Integrated with `antigravity_request::user_agent`. The periodic official manifest fetch, successful credential-backed version probe, and persistent global adopted-version store remain future work; normal Go-compatible User-Agent is preserved without explicit opt-in.
- `crates/cpa-server/src/lib.rs`: register new module.
- `crates/cpa-server/src/scheduler.rs`: use classified model/validation cooldowns on Antigravity HTTP 403; retain existing Go behavior for all other categories and all other providers. Added an integration-level scheduler regression test, including unknown 403 and unrelated-model guard.
- `docs/DIFFERENCES-FROM-GO.md`: document private-branch 403 behavior change.

## Verification

- Python source classifier tests: 8 passed.
- GitHub Actions Linux run 39181764675: **success** (Rust 403 regression test, Linux release build, smoke test, Artifact upload). URL: https://github.com/chuyua/gcl2api-rust-private-build/actions/runs/39181764675
- GitHub Actions Linux run 39182930168: failed on Rust 1.99 compile due to unavailable `Result::is_err_or`; replaced with stable `map_or`. Subsequent run 39183209799 **passed Rust 403 and health/egress tests**, with release compilation in progress when this checkpoint was written.
- GitHub Actions Linux run 39183209799: **Success**, including release build, startup smoke test, and uploaded Linux x86_64 artifact.
- Run 39183679801: **Success**, including CLI UA focused tests and Linux release.
- Run 39184467027: **Success**, private remote branch SHA `d720fd20930c05f7fdbdcb80a4920dae81ff3c3a`, including atomic health persistence tests, Ubuntu 24.04 release, smoke test, and `gcl2api-rust-linux-x86_64` artifact (17,792,986 bytes). Fourth-batch Windows local source commit was `c870b46`; its GitHub snapshot was successfully pushed before the Windows session disappeared.
- Run 39213709142: **Success**, SHA `67744a5ce1c3eacde82f962b9f971a8db12edd42`: Ubuntu 24.04 focused tests and Linux release/smoke after switching the bounded IP echo reader from unsupported `chunk()` to `bytes_stream()`. Prior run 39209666893 failed on the missing method and was superseded.
- Run 39222627510: **Success**, SHA `8c3c43e789f89633ba8dc5207cd937377aed0c58`: Ubuntu 24.04 403/health/CLI UA tests, Release, smoke and artifact upload. A later run 39234892913 of the same SHA also **succeeded**.
- Current in-flight network probe CAS guard: `check_egress_and_persist` captures the credential revision before HTTP I/O and rejects stale results after a concurrent proxy edit/token change. Isolated file/mock regressions cover old-IP success, HTTP 429 and transport error; **not yet Linux CI verified**. This must not be reported as complete parity.
- Next optional API: `check_account_health_and_persist` checks IP echo first and calls `v1internal:loadCodeAssist` with a dummy-test credential through the same effective proxy only if IP is stable. The actual code accepts an explicitly supplied access token; the loopback mock verifies POST body, Authorization placement, atomic health persistence, and no POST after drift. Response bytes never enter logs or credential metadata (only a generic HTTP status reason). No automatic scheduler, real-account check, `fetchUserInfo`/`listExperiments` warmup, group node rebind, or production opt-in yet. **Linux CI pending**.
- Follow-up safety check (not in snapshot 297457c): the optional full probe and direct eligibility helper reject an access token that differs from the credential's own `access_token`, before sending requests. An additional mock assertion rejects an intentionally mismatched dummy token. Requires its own CI validation.
- Standalone Rust classifier tests via `rustc --edition=2024 --test crates/cpa-server/src/antigravity_403.rs`: 2 groups passed.
- `cargo fmt --all`: ran; `git diff --check`: clean at previous checkpoint.
- Windows native `cargo test` failed during third-party `btls-sys` BoringSSL compilation, after working around non-ASCII MinGW toolchain paths. **GitHub Actions Ubuntu 24.04 now supplies the authoritative Linux compile/test gate.** Existing successful run: 39181764675. All subsequent feature additions must pass fresh CI before being called verified.
- No live provider requests / accounts. Linux production services untouched.

## Important partial scope

The Python classifier and CAS-backed account-health persistence are ported. This iteration adds `record_egress`, `fetch_egress` (effective-proxy HTTP echo, 15s timeout, 64 KiB response cap), and `check_egress_and_persist` (blocking-worker persistence), with local mock coverage. Subsequent safety fix: an observed IP drift keeps the last accepted `egress_ip` and records `observed_egress_ip` separately, preventing a second identical echo from silently clearing `proxy_drift`. A separately verified eligibility check and explicit rebind acceptance are still required before moving to the new IP. This is an intentional safety divergence from the Python helper, which overwrites `egress_ip` immediately; verify end-to-end parity before adoption. This is **an opt-in callable API, not yet an automatic runtime scheduler**. Eligibility POST, proxy-rebind retries, dynamic CLI manifest, and account disabling remain unwired. Other 403s keep the existing Go-compatible fallback. Do not report full Python parity.

## Continue in next run

1. Verify the new per-credential proxy edit invalidation: `CredentialStore::apply_patch` atomically resets accepted/observed IP and cached eligibility to `unchecked` when a file-backed Antigravity `proxy_url` changes; identical edits retain the binding. This follows Python `update_credential_network` reset semantics. It does NOT yet support Python proxy-group node cooldown/reselection, inherited configuration changes, or automatically run a health check. Build a focused mock CI regression before extending the path. Verify no credentials enter CI.
2. Re-run `cargo fmt --all --check`, `git diff --check`, and focused Rust tests with private CI. Check `git status` for only intended changes; do not commit generated binaries.
3. Phase 2 health logic is now in Rust; verify its focused CI tests. Implement persistent Antigravity health `binding_status`/`eligibility_status` and safe rebind with mock upstream only. Review `src/antigravity_account_health.py` and `src/storage/sqlite_manager.py` in Python. Record schema and migration before touching credential files.
4. Port credential candidate ranking, cooldown/fallback and image account priority; then add streaming protocol and billing parity cases.
5. Before deployment, prove real Linux idle/peak RSS and protocol parity, and use a separate sandbox. Do not alter the 366MB production VPS without explicit request.

## Rules

Operate on the private local branch. Do not import real auth files or log tokens. Match behavior and test outcomes, not just Python syntax. Read `AGENTS.md` and the exact Python reference first; keep changes narrow, commit locally only after verified build/tests.
- 2026-10-19 follow-up: transient echo transport errors preserve existing `proxy_drift` evidence and `observed_egress_ip`; validated in Ubuntu 24.04 run 39213709142.
- CI run 39209666893 failed compilation: `wreq::Response` has no `chunk()`; switched bounded echo reader to `bytes_stream()` + `futures_util::StreamExt` (already a dependency), preserving the 64 KiB cap. Fixed and verified in run 39213709142.
