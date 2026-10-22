# CI quality policy for cliproxy-rs-plus

## Non-negotiable pull-request checks

- **Upstream provenance, not just a merge message.** The pinned Git SHA
  in `.github/upstream-base.sha` must be the official latest
  `vayungodara/cliproxy-rs` master fetched by CI and must be an actual
  ancestor of the submitted integration. This intentionally blocks a PR
  when upstream advances until new changes are integrated and tested.
  Six local positive/negative tests verify the guard *fails* on forged,
  stale, missing and malformed history.
- **Full backend regression.** The existing Linux full workspace test suite
  runs without outbound network access and without allowing skipped Go or
  PostgreSQL fixture checks. Linux and Windows Clippy use warnings-as-errors.
  Dedicated Antigravity 403, proxy fail-closed, health, SQLite, OAuth and
  version probes remain in the Linux binary build workflow.
- **Dashboard reproducibility.** `npm ci`, all Node tests, Svelte/TypeScript
  diagnostics and the size-capped production build run on Linux. CI rejects
  stale generated `dist`, self-contained panel or panel SHA256 files.
  The dashboard's translated strings do not alter API/provider identifiers.
- **Low-RAM deployment budgets.** Linux executable size, idle wakeups,
  CPU, thread count and RSS remain strictly capped in `bench/budgets.txt`.
  A failed one-account RSS measurement triggers two additional diagnostic
  samples from fresh isolated processes. **The job remains failed** even
  if diagnostics pass; diagnostic samples are not a waiver.
  Increasing a budget is a reviewed decision requiring reproducible
  same-toolchain measurements and a dated, quantified explanation.
- **Review gate.** Require the quality workflow and critical Rust CI
  contexts on `develop` and `main`. No bypass, direct commits to
  `main`, force-push, release or production deployment as a substitute
  for passing checks.

## Limits

No test suite can prove absence of bugs, correct live provider credentials
or complete Python gcl2api-plus parity. Tests use only mocks or isolated
environments. Proxy group SQLite and CLI-version sidecars are still not
automatically connected to the live scheduler. The protected stable branch
must advance only after explicit review of those limitations.
