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
- `crates/cpa-server/src/lib.rs`: register new module.
- `crates/cpa-server/src/scheduler.rs`: use classified model/validation cooldowns on Antigravity HTTP 403; retain existing Go behavior for all other categories and all other providers. Added an integration-level scheduler regression test, including unknown 403 and unrelated-model guard.
- `docs/DIFFERENCES-FROM-GO.md`: document private-branch 403 behavior change.

## Verification

- Python source classifier tests: 8 passed.
- Standalone Rust classifier tests via `rustc --edition=2024 --test crates/cpa-server/src/antigravity_403.rs`: 2 groups passed.
- `cargo fmt --all`: ran; `git diff --check`: clean at previous checkpoint.
- Full `cargo test -p cpa-server gcl2api_antigravity_model_and_validation_cooldowns --lib`: **first complete Windows build in progress**, not yet verified. Windows GNU dlltool and linker fail when source/Cargo/rustup paths contain Chinese usernames. Isolated ASCII-only junctions: `C:\Users\Public\gcl2api-rust-src` (source), `C:\Users\Public\gcl2api-cargo` (Cargo cache), `C:\Users\Public\gcl2api-rustup` (Rust toolchain). Use `CARGO_HOME=C:\Users\Public\gcl2api-cargo`, `CARGO_TARGET_DIR=C:\Users\Public\gcl2api-rust-target`, `RUSTFLAGS=--sysroot C:\Users\Public\gcl2api-rustup\toolchains\stable-x86_64-pc-windows-gnu`, `CARGO_BUILD_JOBS=2` for Windows cargo commands. `btls-sys` CMake assumes Visual Studio `cl` despite MinGW; proved manual CMake with explicit GCC/G++, then primed the actual generated build cache with CMake `-DCMAKE_C_COMPILER=gcc -DCMAKE_CXX_COMPILER=g++` and an isolated toolchain file `C:\Users\Public\gcl2api-mingw-toolchain.cmake`. Build now passed past compiler configuration and reached cpa-server, but its long native BoringSSL compilation is not yet verified as complete; build session at the 2026-10-19 checkpoint: `session-ecc7cd91fc9fac410d794f5c`. Resume by checking session status (if still present), or rerun the cargo command with these isolated environment values. If btls-sys reruns in a fresh target, prime its CMake cache before running Cargo again. Re-check integration result before claiming success.
- No live provider requests / accounts. Linux production services untouched.

## Important partial scope

The Python classifier is ported, but account-health persistence, binding health probes, proxy-rebind retries, and automatic account disables **are not wired** in the Rust runtime yet. Until those exist, the new scheduler only uses the safe 300/600-second model cooling decisions. Other 403s keep Rust's existing Go-compatible behavior. Do not report full Python parity.

## Continue in next run

1. Check `cargo test -p cpa-server gcl2api_antigravity_model_and_validation_cooldowns --lib` (set `CARGO_TARGET_DIR=C:\Users\Public\gcl2api-rust-target`, `CARGO_BUILD_JOBS=2` in Windows); fix compile/test errors.
2. Re-run `cargo fmt --all --check`, `git diff --check`, and focused Rust tests. Check `git status` for only intended changes; do not commit generated binaries.
3. Build next Python feature module: persist Antigravity account-health `binding_status`/`eligibility_status` and safe rebind, using mocked upstream only. Review `src/antigravity_account_health.py` and `src/storage/sqlite_manager.py` in Python. Record schema and migration before touching credential files.
4. Port credential candidate ranking, cooldown/fallback and image account priority; then add streaming protocol and billing parity cases.
5. Before deployment, prove real Linux idle/peak RSS and protocol parity, and use a separate sandbox. Do not alter the 366MB production VPS without explicit request.

## Rules

Operate on the private local branch. Do not import real auth files or log tokens. Match behavior and test outcomes, not just Python syntax. Read `AGENTS.md` and the exact Python reference first; keep changes narrow, commit locally only after verified build/tests.
