# Codex Sign in with ChatGPT expiry golden

`../../fixtures/codex_siwc_expiry_go.json` records CLIProxyAPI v8.0.35 (`ae3ee3be`)
`CodexSIWCNeedsRefresh` for Sign in with ChatGPT partitions at two fixed clocks. The
cases cover Go's `time.Parse` grammar for the layouts `parseTimeValue` tries,
`normaliseUnix` (milliseconds, far-future values, an out-of-range float, which the recording linux/amd64 converts to `math.MinInt64`),
`expires_in` after an issue time with Go's wrapping `Duration` product, nested token
maps, the JWT `exp` fallback (Go's base64 decoding and its decode into `struct{ Exp any }`)
and Go's zero time. Each `siwc` is the partition as JSON,
decoded like a credential file.

`src/codex_siwc_tests.rs` `refresh_due_matches_go_expiry_parsing` replays every case; `float at the unit threshold` expects the
result listed in docs/DIFFERENCES-FROM-GO.md.

Regenerate with Go 1.26.4:

```sh
cp zz_rsfix_siwc_expiry_test.go <CLIProxyAPI>/sdk/cliproxy/auth/
cd <CLIProxyAPI>
RSFIX_OUT=/tmp/rsfix-siwc-expiry go test -count=1 -run TestRSFixSIWCExpiry ./sdk/cliproxy/auth/
cp /tmp/rsfix-siwc-expiry/codex_siwc_expiry_go.json <cliproxy-rs>/crates/cpa-exec/tests/fixtures/
```
