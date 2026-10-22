# Claude 429 cooldown vectors

`../../fixtures/claude_ratelimit_go.json` holds outputs of CLIProxyAPI v8.0.35
(`ae3ee3be`) for the inputs in `zz_rsfix_claude_ratelimit_test.go`. For each case it
records:

- `reset_ms`: `helps.ParseClaudeRateLimitResetForError` at a fixed `now`
  (2026-10-17T14:00:00Z), without the 1–30 s fuzz;
- `header_only_ms`: `helps.ParseClaudeRateLimitReset`, the result before db1bab01;
- `shared`: `helps.ClaudeHeadersIndicateUnifiedRateLimitRejection`;
- `scope`/`classify_retry` and `fast_scope`/`fast_retry`: the scope and whether a
  cooldown is set for `classifyClaudeUpstreamErrorWithCooling(429, …, false)` and
  `newClaudeFastDirectResponseError` on a 429, which read `time.Now()`.

Header values may contain `{now+N}` (the Unix second N seconds after now) and
`{date+N}` (that instant as an HTTP date). Go's fuzz is a whole number of seconds, so
the recorder takes the minimum over 600 calls and subtracts one second.

The Rust replay is `claude::tests::claude_rate_limit_cooldown_and_scope_match_go`.

Regenerate in a scratch copy of the reference (the checkout is not modified), without
network access:

```sh
reference=/absolute/path/to/CLIProxyAPI   # at v8.0.35
crate=/absolute/path/to/cliproxy-rs/crates/cpa-exec
tmp=$(mktemp -d)
tar -C "$reference" --exclude=.git -cf - . | tar -C "$tmp" -xf -
cp "$crate/tests/reference/claude_ratelimit/zz_rsfix_claude_ratelimit_test.go" "$tmp/internal/runtime/executor/"
(cd "$tmp" && RSFIX_OUT="$crate/tests/fixtures/claude_ratelimit_go.json" \
  go test ./internal/runtime/executor/ -run TestRSFixClaudeRateLimit -count=1)
```
