# gcl2api-plus → cliproxy-rs migration checkpoint

Status: **Phase 1 started — first Antigravity 403 vertical slice**
Last updated: 2026-10-19 (Windows developer node)

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
- `crates/cpa-server/src/lib.rs`: register new module.
- `crates/cpa-server/src/scheduler.rs`: use classified model/validation cooldowns on Antigravity HTTP 403; retain existing Go behavior for all other categories and all other providers. Added an integration-level scheduler regression test, including unknown 403 and unrelated-model guard.
- `docs/DIFFERENCES-FROM-GO.md`: document private-branch 403 behavior change.

## Verification

- Python source classifier tests: 8 passed.
- GitHub Actions Linux run 39181764675: **success** (Rust 403 regression test, Linux release build, smoke test, Artifact upload). URL: https://github.com/chuyua/gcl2api-rust-private-build/actions/runs/39181764675
- Standalone Rust classifier tests via `rustc --edition=2024 --test crates/cpa-server/src/antigravity_403.rs`: 2 groups passed.
- `cargo fmt --all`: ran; `git diff --check`: clean at previous checkpoint.
- Windows native `cargo test` failed during third-party `btls-sys` BoringSSL compilation, after working around non-ASCII MinGW toolchain paths. **GitHub Actions Ubuntu 24.04 now supplies the authoritative Linux compile/test gate.** Existing successful run: 39181764675. All subsequent feature additions must pass fresh CI before being called verified.
- No live provider requests / accounts. Linux production services untouched.

## Important partial scope

The Python classifier is ported, but account-health persistence, binding health probes, proxy-rebind retries, and automatic account disables **are not wired** in the Rust runtime yet. Until those exist, the new scheduler only uses the safe 300/600-second model cooling decisions. Other 403s keep Rust's existing Go-compatible behavior. Do not report full Python parity.

## Continue in next run

1. Check latest private GitHub Actions build; review `antigravity_health` and `gcl2api_antigravity` Rust tests and Linux release artifact.
2. Re-run `cargo fmt --all --check`, `git diff --check`, and focused Rust tests with private CI. Check `git status` for only intended changes; do not commit generated binaries.
3. Phase 2 health logic is now in Rust; verify its focused CI tests. Implement persistent Antigravity health `binding_status`/`eligibility_status` and safe rebind with mock upstream only. Review `src/antigravity_account_health.py` and `src/storage/sqlite_manager.py` in Python. Record schema and migration before touching credential files.
4. Port credential candidate ranking, cooldown/fallback and image account priority; then add streaming protocol and billing parity cases.
5. Before deployment, prove real Linux idle/peak RSS and protocol parity, and use a separate sandbox. Do not alter the 366MB production VPS without explicit request.

## Rules

Operate on the private local branch. Do not import real auth files or log tokens. Match behavior and test outcomes, not just Python syntax. Read `AGENTS.md` and the exact Python reference first; keep changes narrow, commit locally only after verified build/tests.
