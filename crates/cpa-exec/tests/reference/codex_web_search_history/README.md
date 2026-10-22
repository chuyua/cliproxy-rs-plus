# Codex web_search_call history goldens

`../../fixtures/codex_web_search_history_go.json` records what CLIProxyAPI v8.0.35
(`ae3ee3be`) does with replayed `web_search_call` input items (e7d4c731):

- `cases`: `ensureWebSearchToolForHistory` on its own. Inputs are the payloads of Go's
  `TestCodexExecutorReplayedWebSearchCallHistoryEnsuresWebSearchTool_Issue6770` plus the
  edges of the tool-type, Responses Lite and sjson paths. `out` is omitted when Go
  returned the input unchanged.
- `executor`: the Issue6770 subtests run through `CodexExecutor` against a loopback
  upstream (stream, non-stream, compact) and through `prepareCodexWebsocketStream`, with
  the body Go sent (`sent`). The payload-filter subtest is left out until 868e137 lands.

`src/codex_request.rs` replays `cases`; `src/codex_tests.rs` replays `executor`.

Regenerate (Go 1.26, no network beyond module download):

```sh
cp zz_rsfix_codex_web_search_history_test.go <CLIProxyAPI>/internal/runtime/executor/
cd <CLIProxyAPI>
RSFIX_OUT=/tmp/rsfix-wsh go test -count=1 -run TestRSFixCodexWebSearchHistory ./internal/runtime/executor/
cp /tmp/rsfix-wsh/codex_web_search_history_go.json <cliproxy-rs>/crates/cpa-exec/tests/fixtures/
```
