# Image, video and speech goldens

Replayed by `tests/media_go.rs`. `main.go` starts the unmodified Go server binary
(`cmd/server` at CLIProxyAPI `6fecc6e`, run with `-local-model`) against a scripted local
capture server, sends each scenario from `scenarios.go` in order, and records the status,
selected response headers, the body and every raw upstream request. All keys are fake and
every base URL points at the capture server. Run it inside a loopback-only network
namespace anyway:

```sh
# in the CLIProxyAPI tree
go build -o /tmp/cpa-go-server ./cmd/server
# in a module that replaces github.com/router-for-me/CLIProxyAPI/v8 with that tree
cp *.go "$gen/"
cd "$gen" && go run . /tmp/cpa-go-server ../../fixtures/media_go.json
```

Scenarios share one server, so order matters: video IDs created early are polled later
(credential pinning), and the 429 cases run last because they cool their model down.
Replies with `delay_ms` outlast the 1s keep-alive intervals in the shared config.
The `models_grok_shell*` cases cover the Grok Shell `GET /v1/models` catalog, built from
the same registry (xAI catalog, xAI image and video builtins, compat models).

Normalized on both sides: the capture address (`UPSTREAM`), Go's random multipart
boundary (`BOUNDARY`), with each group of form parts Go writes in map order sorted,
timestamps from the clock (`"<now>"`), generated `video_<id>` IDs, and the two xAI keys
as `XAI-KEY-<n>` in order of first use. Credential IDs hash the base URL, which holds the
random capture port, so which key sorts first changes between runs. Request bodies that
are not valid UTF-8 are stored in `body_b64`.

## Speech goldens

`fixtures/speech_go.json` holds the `/v1/audio/speech` and `/v1/tts` scenarios from
`speech.go`, recorded from the unmodified Go server at CLIProxyAPI v8.0.35
(`ae3ee3be`) with the same driver and its own config (`SpeechConfig`: two xAI keys, a
payload rule for the voice model and `passthrough-headers: true`, so upstream audio types
and would-be error headers show). The zstd request bodies in `speech.go` were made with
the zstd CLI; `skip_prefix` puts a zstd skippable frame of that many zero bytes before a
body when it is sent, so a 3 MiB request stays out of the fixture. The replay compares the six kept response headers, not all of them, and the
audio bodies are short ASCII stand-ins:

```sh
# in the CLIProxyAPI tree at ae3ee3be
go build -o /tmp/cpa-go-server-v8.0.35 ./cmd/server
cp *.go "$gen/"
cd "$gen" && go run . /tmp/cpa-go-server-v8.0.35 ../../fixtures/speech_go.json speech
```

The driver only needs the standard library, so `$gen` can be any module. The
model-unavailable 404 runs last because it cools `grok-tts` on both keys. The run also
records the Grok Shell catalog; the 6fecc6e `models_grok_shell*` expectations take the two
speech builtins from it (v8.0.35 6a853dce adds them to the xAI catalog).


## Passthrough error-header goldens

`fixtures/passthrough_errors_go.json` holds the `errors.go` scenarios, recorded from the
unmodified Go server at v8.0.35 with its own config (`ErrorsConfig`: one
OpenAI-compatible provider and `passthrough-headers: true`). An upstream status error
hands the client no upstream header; a stream that ends before its first payload hands it
the stream's own headers (Go `streamBootstrapError.Headers()`). Cache-Control is the
probe header. In `compat_stream_empty` Go also copies the capture server's
`Content-Length: 0`, which empties its error body; the replay skips that length and body
(`ADDON_FRAMING`):

```sh
cp *.go "$gen/"
cd "$gen" && go run . /tmp/cpa-go-server-v8.0.35 ../../fixtures/passthrough_errors_go.json errors
```
