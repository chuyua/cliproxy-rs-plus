package main

// ErrorsConfig is the passthrough error-header run's config (CLIProxyAPI v8.0.35): one
// OpenAI-compatible provider on the capture server and `passthrough-headers: true`, so
// any header an error hands the client (Go `ErrorMessage.Addon`) shows. Cache-Control is
// the probe: the replay keeps it, and nothing else sets it on an error.
const ErrorsConfig = `config-version: 8
server:
  host: 127.0.0.1
  port: PORT
management:
  disable-control-panel: true
requests:
  passthrough-headers: true
access:
  api-keys: [client-key-1]
oauth:
  auth-dir: AUTHDIR
api-keys:
  openai-compatibility:
    - name: Acme
      base-url: http://UPSTREAM/v1
      models:
        - name: acme-chat
          alias: chat
      keys:
        - api-key: sk-fake-acme
`

func errorScenarios() []scenario {
	const chat = "/v1/chat/completions"
	return []scenario{
		// A plain upstream status error has no Headers(): no upstream header reaches the
		// client, and the error is JSON whatever the upstream type was.
		post("compat_status_error", chat, jsonType,
			`{"model":"chat","messages":[{"role":"user","content":"hi"}]}`,
			upstream{Status: 400, Headers: [][2]string{{"Content-Type", "text/plain"}, {"Cache-Control", "no-store"}},
				Body: `{"error":{"message":"bad input","type":"invalid_request_error"}}`}),
		post("compat_stream_status_error", chat, jsonType,
			`{"model":"chat","stream":true,"messages":[{"role":"user","content":"hi"}]}`,
			upstream{Status: 400, Headers: [][2]string{{"Content-Type", "text/plain"}, {"Cache-Control", "no-store"}},
				Body: `{"error":{"message":"bad input","type":"invalid_request_error"}}`}),
		// A stream that ends before its first payload fails as a streamBootstrapError,
		// whose Headers() are the stream's own response headers.
		post("compat_stream_empty", chat, jsonType,
			`{"model":"chat","stream":true,"messages":[{"role":"user","content":"hi"}]}`,
			upstream{Status: 200, Headers: [][2]string{{"Content-Type", "text/event-stream"}, {"Cache-Control", "no-cache"}}}),
	}
}
