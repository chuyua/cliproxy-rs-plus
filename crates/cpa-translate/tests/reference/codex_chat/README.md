# Chat Completions <-> Codex goldens at v8.0.35

`../../fixtures/codex_chat_go.json` records what CLIProxyAPI v8.0.35 (`ae3ee3be`) returns
from the `internal/translator/codex/openai/chat-completions` converters for two commits
the 6fecc6e pair fixtures predate:

- `requests`: `ConvertOpenAIRequestToCodex` with over-length tool call IDs (b0debb35).
- `streams` and `non_stream`: `ConvertCodexResponseToOpenAI` (one output list per input
  line, one `param` per stream) and `ConvertCodexResponseToOpenAINonStream` with URL
  citations (16d98881). `pair_codex_events_non_stream_0` re-records the pair fixture
  `codex-events/non-stream/0`, which 16d98881 changes; `tests/golden.rs` lists it as
  superseded.

Inputs follow Go's tests for both commits plus edge cases. `src/codex_chat_request.rs`
and `src/codex_chat_response.rs` replay the file.

Regenerate (Go 1.26, no network beyond module download):

```sh
cp zz_rsfix_codex_chat_test.go <CLIProxyAPI>/internal/translator/codex/openai/chat-completions/
cd <CLIProxyAPI>
RSFIX_OUT=/tmp/rsfix-cc go test -count=1 -run TestRSFixCodexChat ./internal/translator/codex/openai/chat-completions/
cp /tmp/rsfix-cc/codex_chat_go.json <cliproxy-rs>/crates/cpa-translate/tests/fixtures/
```
