# gcl2api-plus → cliproxy-rs migration checkpoint

Status: **Phase 2 — health/explicit rebind and proxy-group policy CI-verified; companion SQLite transactions under test**
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
- Run 39363746266: **Success**, SHA `5262b1371139baa115f46df4bbddf7f67b6d3f54`: credential-revision CAS guard for in-flight echo results, Ubuntu 24.04 tests, Release, smoke and artifact upload.
- Run 39364271678: **Success**, SHA `297457c9f996f5e01b6a70ec6dee9e37f9967356`: optional same-proxy egress echo then bounded `v1internal:loadCodeAssist` eligibility POST; mock checks no OAuth on echo and no credential-backed POST after drift.
- Run 39365041772: **Success**, SHA `ce6cd3caefa326ec7a0c6046854dcb72322b604b`: reject mismatched account access token before health network I/O; Ubuntu 24.04 tests, Release, smoke and artifact upload.
- Run 39366251127, SHA `2e6f12a71fd5579a0223f5e7d5dc704dfaae8d59`: optional `confirm_egress_rebind_and_persist` accepts a drifted binding only if the explicitly approved observed IP is echoed again, eligibility succeeds, and the pre-network credential revision still matches. Invalid approval, echo mismatch, and repeat approval without drift are rejected. Private Ubuntu CI was running at this checkpoint; do not mark verified before final success. This is *not* Python proxy-group cooldown/reselection.
- Run 39366251127 is now **Success**, with Ubuntu 24.04 focused tests, Release, smoke and artifact upload.
- Current unverified change: after a successful `loadCodeAssist` 200 with project ID, an opt-in account-health check also calls `fetchUserInfo` then `listExperiments` with the Python stable-channel request shape. Both share the account's effective proxy and token; each has a 15-second cap, errors are ignored without logging response bodies or tokens, and the feature is not a hot-path scheduler. Loopback Mock asserts warmup headers/project/body, exact call counts, and that IP drift skips the entire credential-backed path. Project ID must be a nonempty bounded string or `{"id":"..."}` object. **Linux CI pending.**
- Run 39366671233, private SHA `b76022bbd9d57da72a19c6f1c2c15bd1c3d47bb1`: warmup and associated loopback test, Ubuntu CI running (not yet fully verified).
- New pure-Rust `antigravity_proxy_group` selector ports Python's round-robin, random and failover node-choice rules, enabled/cooldown filters, 300-second failed-node cooldown and no-direct-fallback behavior. All operations are parameterized and tested without real proxies; private workflow gains a dedicated test step. **This is not a SQLite adapter, management API or atomic persistent rebind.** Group/cursor/node/binding updates must be implemented together in a transaction before enabling live rebind. Linux CI pending.
- Run 39366671233 for warmup `b76022b` and run 39367114906 for pure proxy-group policy `179cc09` are now **Success** (focused tests, Release build and smoke).
- New isolated companion DB `antigravity_proxy_sqlite`: explicit create-new only, marker-protected reopen, private permissions on Unix, SQLite `BEGIN IMMEDIATE` transactions over `rust_ag_groups`, `rust_ag_nodes` and `rust_ag_accounts`. Selection persists cursor/node last-used/account binding together. Failure persists node cooldown and failure count, releases the old binding, clears cached health and selects any eligible replacement in one transaction; if no node is available it leaves the account unbound, **never direct**. Includes restart persistence, concurrent bind, rollback injection, cooldown, isolation and unrelated-DB refusal tests. Added bundled SQLite dependency, locked version, and Ubuntu CI focused-test step. This is a **companion isolated schema**, NOT Python's live SQLite schema or Rust credential JSON state. It does not yet make the runtime use the new proxy, and transaction guarantees do not cross the auth JSON boundary. CI validation pending.
- GitHub Actions 39451136272 for SQLite companion commit `5e7f6e2e063ccb43c68759b0c6e62f6e29e24296` finished **Success**. Follow-up guard requires the failed request's `expected_node_id` in `invalidate_and_rebind` and rejects stale failures after a different node has been bound, within the same immediate transaction. Regression checks no cooldown increment or health change on the replacement node. This guard still needs its own Linux CI validation. A node ID may be reused after cooldown; a future production interface must additionally guard against same-node ABA with a binding generation or credential revision.
- Snapshot `8729a9278a4f17b09a60fe478503f7bcf0916f27` contains the node-ID guard, with Actions 39530899397 as the validation gate. Further work in the Windows tree upgrades the isolated companion schema to v2, supplies a durable binding generation with each selected proxy, and checks both node ID and generation inside the failure transaction. Reopen migrates old schema v1 atomically, with regression tests for old-data upgrade and same-node ABA. This next batch remains unverified by Linux CI and must not be wired into the live dispatcher before successful CI and cross-store coordination.
- Standalone Rust classifier tests via `rustc --edition=2024 --test crates/cpa-server/src/antigravity_403.rs`: 2 groups passed.
- `cargo fmt --all`: ran; `git diff --check`: clean at previous checkpoint.
- Windows native `cargo test` failed during third-party `btls-sys` BoringSSL compilation, after working around non-ASCII MinGW toolchain paths. **GitHub Actions Ubuntu 24.04 now supplies the authoritative Linux compile/test gate.** Existing successful run: 39181764675. All subsequent feature additions must pass fresh CI before being called verified.
- No live provider requests / accounts. Linux production services untouched.

## Important partial scope

The Python classifier and CAS-backed account-health persistence are ported. `record_egress`, `fetch_egress` (effective-proxy HTTP echo, 15s timeout, 64 KiB response cap), and `check_egress_and_persist` have local mock coverage. An observed IP drift keeps the last accepted `egress_ip` and records `observed_egress_ip` separately so a second identical echo cannot silently clear it. The opt-in eligibility POST and explicit fresh-echo + eligible rebind confirmation now exist, but **none is an automatic request-time scheduler**. The Python global proxy-group SQLite/node management and 300-second node cooldown, dynamic CLI manifest adoption, automatic credential rotation and account disablement remain unwired; some 403s retain Go-compatible fallback. Do not report full Python parity.

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
