# Codex tool schema goldens

`../../fixtures/codex_tool_schema_go.json` records what CLIProxyAPI v8.0.35 (`ae3ee3be`)
returns from two request passes:

- `schemas`: `helps.NormalizeCodexToolSchemas`, the union-to-enum collapse plus the
  pattern and out-of-range int64 bound strip (57d005fe).
- `integers`: `helps.NormalizeCodexToolIntegerTypes`, which wraps
  `client/codex/tool-schema` (d7914afd's field paths and namespaces). The history and
  notes cases use Go's `testdata/history_notes_tools.json`, compacted.

Inputs follow Go's tests for both commits. `out` is omitted when Go returned the input
unchanged. `src/codex_request.rs` replays the file.

Regenerate (Go 1.26, no network beyond module download):

```sh
cp zz_rsfix_codex_tool_schema_test.go <CLIProxyAPI>/internal/runtime/executor/helps/
cd <CLIProxyAPI>
RSFIX_OUT=/tmp/rsfix-ts go test -count=1 -run TestRSFixCodexToolSchema ./internal/runtime/executor/helps/
cp /tmp/rsfix-ts/codex_tool_schema_go.json <cliproxy-rs>/crates/cpa-exec/tests/fixtures/
```
