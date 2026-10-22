# Updating cliproxy-rs in the gcl2api Rust branch

## One-time history bridge (2026-10-23)

The original private-build branch started as an independent source snapshot
(`a941f9a`), not a Git fork sharing an ancestor with `vayungodara/cliproxy-rs`.
A plain `git merge` therefore cannot be the first sync.

The integration branch `sync/cliproxy-20261023` starts at upstream
`0805c43`. It ports the published private changes relative to the upstream
snapshot `d1261f2` (private head `a17dc19`) and resolves conflicts in
`Cargo.lock`, `cpa-server/src/lib.rs`, `runtime.rs`, and `scheduler.rs`.
The resulting history-bridge merge records the prior private branch as a
second parent **without replacing** the integrated upstream-first tree.

Do not use `git merge --allow-unrelated-histories` again after the bridge.
After this integration becomes the main private branch, subsequent upstream
syncs can be normal merges.

## Procedure for later updates

1. Fetch `origin/master` (the upstream repository) and the current private
   branch. Inspect `git status` first; use a separate worktree whenever the
   development directory has local or staged changes.
2. Create a temporary branch from the current integrated private branch.
   Merge the fetched upstream tip (no force push, no unrelated-histories flag).
3. Resolve conflicts by keeping upstream's OAuth/Management API, credential
   persistence and protocol behavior, while preserving the opt-in gcl2api
   Antigravity 403/health/proxy-group/CLI-identity hooks.
4. Run `cargo fmt --all --check`, `cargo clippy --workspace --all-targets
   --locked -- -D warnings`, and the private Antigravity regression tests
   (see `private-linux-build.yml`) on Linux, then the upstream CI suite.
   Use only dummy/local mock accounts and avoid real credentials.
5. Review the PR diff, regressions and memory behavior before accepting it.
   Nothing in a source sync authorizes deployment or release.

## Current feature ceilings

- The Management API Antigravity login is inherited from upstream.
- Antigravity 403 model-specific cooldowns and explicit group-mode
  fail-closed transport guards are live.
- The companion SQLite proxy-group scheduler/lease and CLI version stores
  have tested modules but are **not** connected to automatic live dispatch.
- Local Windows GNU toolchains may fail when the compilation target path
  contains non-ASCII characters; prefer the Ubuntu CI verification gate.

The local staged work in `gcl2api-rust-port` was not imported into this
sync. It is separate from the GitHub-published private tip and must be
reviewed independently before any later port.
