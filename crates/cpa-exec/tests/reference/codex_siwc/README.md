# Codex Sign in with ChatGPT request goldens

`../../fixtures/codex_siwc_go.json` records what CLIProxyAPI v8.0.35 (`ae3ee3be`) sends
upstream for a Codex credential with Sign in with ChatGPT enabled (`use_siwc`, 7b0319fb):

- `stream`, `non_stream`, `compact` and `image` cases ran through `CodexExecutor`
  against a loopback upstream; `path`, `body` and `sent_headers` are what reached it.
- `websocket` cases ran through `prepareCodexWebsocketStream`; `body` and `sent_headers`
  are `prepared.clientBody` and `prepared.wsHeaders`.
- `missing_token` records the error returned before any request (`status`, `error`).

The payloads cover the dropped fields, `configuration_update` collapsing with and
without a model thinking suffix, explicit and blank instructions, the WebSocket beta
header, an operator header and a custom `signin-with-chatgpt-identifier`. The
User-Agent carries Go's `runtime.GOOS` and `runtime.GOARCH`; the fixture was recorded on
linux/amd64, and the replay substitutes the test host's Go platform names.

`src/codex_tests.rs` `siwc_requests_match_go` replays every case.

Regenerate with Go 1.26.4 (no network beyond module download):

```sh
cp zz_rsfix_codex_siwc_test.go <CLIProxyAPI>/internal/runtime/executor/
cd <CLIProxyAPI>
RSFIX_OUT=/tmp/rsfix-siwc go test -count=1 -run TestRSFixCodexSIWC ./internal/runtime/executor/
cp /tmp/rsfix-siwc/codex_siwc_go.json <cliproxy-rs>/crates/cpa-exec/tests/fixtures/
```
