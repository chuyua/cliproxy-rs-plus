# Responses WebSocket fixtures

Both files were produced by CLIProxyAPI itself; the Rust tests never derive expected
values from Rust code. Every credential is fake and the upstream is a loopback mock, so
nothing reaches OpenAI.

- `ws_vectors.json`: the handler's pure helpers (request normalization and incremental
  merge, passthrough, prewarm, SSE payload extraction, error payloads, request-fault
  exposure, completion output restoration, pending tool calls, tool-call repair against
  the shared caches, close-reason truncation). Recorded from `6fecc6e`. Replayed by
  `src/websocket_requests_tests.rs`.
- `ws_e2e.json`: forty-seven scenarios through Go's real `ResponsesWebsocket` handler, auth
  manager, built-in translators and `CodexAutoExecutor`, against a scripted upstream that
  speaks both WebSocket and HTTP SSE. It records the frames and close codes the client
  saw, what upstream received (WebSocket dials and frames, HTTP requests) and the upgrade
  response's `x-codex-turn-state`. Replayed by `tests/ws_e2e.rs`, which masks UUIDs and
  the prewarm timestamp. A full v8.0.35 (`ae3ee3be`) run reproduces every scenario
  apart from random keys, IDs and timestamps. `ws_request_fault_headers` and the shared
  `upstream.codex` steering scenarios (`steer_historical_oauth_path_steers_api_key`,
  `steer_idle_upstream_close_upstream_*`, Go 52d5507d) come from that run; the others keep
  their `6fecc6e` bytes.

The upstream mock reads WebSocket frames itself, as gorilla does, so a text frame need not be UTF-8; a frame that is not UTF-8 is recorded with its exact bytes as `body_hex`.

Scripted upstream closes wait 300 ms after their events. Without that pause Go's
disconnect notifier races the turn and can close the client before the events are
written; the fixtures record the intended order.

## Regenerating

```sh
git -C <CLIProxyAPI> worktree add /tmp/cpa-ws ae3ee3be
cp go/zz_rsfix_ws_test.go /tmp/cpa-ws/sdk/api/handlers/openai/
(cd /tmp/cpa-ws && RSFIX_OUT=/tmp/rsfix-ws go test -count=1 -run TestRSFixWS ./sdk/api/handlers/openai/)
cp /tmp/rsfix-ws/*.json .
```

To record one scenario, make `TestRSFixWSE2E` in the copy keep only the scenario named
by `RSFIX_ONLY`, run with `RSFIX_ONLY=<name>`, and splice that element into `ws_e2e.json`.
