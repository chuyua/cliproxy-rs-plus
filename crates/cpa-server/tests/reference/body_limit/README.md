# Request body limit goldens

`../../fixtures/body_limit_go.json` holds `server.max-request-body-bytes` answers from
CLIProxyAPI v8.0.35 (`ae3ee3be`), produced in-process by `main.go`: a complete
`api.NewServer` driven through `httptest`, with no credentials, so accepted bodies stop at
the model lookup. It opens no sockets and calls no provider. Each case records the
status, `Content-Type`, body and how many body bytes the server read. `tests/body_limit_go.rs`
replays it through the binary's middleware stack.

The fixture is recorded with Go 1.26.4, the toolchain of the v8.0.35 release build (`GO_VERSION` in the reference's `.github/workflows/release.yaml`); Go 1.26.4 quotes `textproto` MIME header diagnostics with `%q`, where 1.26.0 did not. Regenerate with a temporary module inside the reference module's internal-package
boundary, as for `../manage` (the reference checkout is not modified):

```sh
reference=/absolute/path/to/CLIProxyAPI   # at ae3ee3be (v8.0.35)
crate=/absolute/path/to/cliproxy-rs/crates/cpa-server
tmp=$(mktemp -d)
cp "$crate/tests/reference/body_limit/main.go" "$tmp/main.go"
(
  cd "$tmp"
  go mod init github.com/router-for-me/CLIProxyAPI/v8/fixture
  go mod edit -require=github.com/router-for-me/CLIProxyAPI/v8@v8.0.0
  go mod edit -replace=github.com/router-for-me/CLIProxyAPI/v8="$reference"
  cp "$reference/go.sum" .
  go mod tidy
  TZ=UTC GOFLAGS=-mod=mod GOTOOLCHAIN=local /path/to/go1.26.4/bin/go run . "$crate/tests/fixtures/body_limit_go.json"
)
rm -rf "$tmp"
```

`/openai/v1/videos` failures carry a random video ID, which the replay ignores. The
plugin auth cases add a provider that reads the body as Go's plugin host adapter does
(`readAndRestoreRequestBody`, then `NewRequestEntityTooLargeError`); the replay registers
the frontend auth provider of a plugin that is not loaded, which reads the body the same
way.
