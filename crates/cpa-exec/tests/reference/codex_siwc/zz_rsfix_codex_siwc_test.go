package executor

// Records what CLIProxyAPI v8.0.35 (ae3ee3be) sends upstream for a Codex credential with
// Sign in with ChatGPT enabled (use_siwc, 7b0319fb): the HTTP stream, non-stream and
// compact requests, and the WebSocket turn prepareCodexWebsocketStream builds. See
// README.md.

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/router-for-me/CLIProxyAPI/v8/internal/config"
	cliproxyauth "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/auth"
	cliproxyexecutor "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/executor"
	sdktranslator "github.com/router-for-me/CLIProxyAPI/v8/sdk/translator"
)

type rsfixSIWCCase struct {
	Name    string            `json:"name"`
	Call    string            `json:"call"`
	Model   string            `json:"model"`
	Config  string            `json:"config,omitempty"`
	Headers map[string]string `json:"headers,omitempty"`
	Attrs   map[string]string `json:"attributes,omitempty"`
	// Metadata is the credential file; base_url points at the loopback upstream.
	Metadata map[string]any `json:"metadata"`
	Payload  string         `json:"payload"`
	// Path, Body and SentHeaders are what reached the upstream (WebSocket:
	// prepared.clientBody and prepared.wsHeaders).
	Path        string              `json:"path,omitempty"`
	Body        string              `json:"body,omitempty"`
	SentHeaders map[string][]string `json:"sent_headers,omitempty"`
	Status      int                 `json:"status,omitempty"`
	Error       string              `json:"error,omitempty"`
}

func rsfixSIWCRun(t *testing.T, c rsfixSIWCCase) rsfixSIWCCase {
	var got *http.Request
	var body []byte
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		got = r
		body, _ = io.ReadAll(r.Body)
		if c.Call == "image" {
			w.Header().Set("Content-Type", "text/event-stream")
			_, _ = w.Write([]byte("data: {\"type\":\"response.output_item.done\",\"output_index\":0,\"item\":{\"type\":\"image_generation_call\",\"id\":\"ig_0\",\"status\":\"completed\",\"result\":\"SU1HMQ==\",\"output_format\":\"png\"}}\n\n" +
				"data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_img\",\"created_at\":1700000000,\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":1,\"output_tokens\":2,\"total_tokens\":3}}}\n\n"))
			return
		}
		if c.Call == "compact" {
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"id":"resp_compact_1","object":"response.compaction","usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}`))
			return
		}
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = w.Write([]byte("data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"object\":\"response\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":10,\"output_tokens\":5,\"total_tokens\":15}}}\n\n"))
	}))
	defer server.Close()
	cfg, err := config.ParseConfigBytes([]byte(c.Config + "\nport: 8317\n"))
	if err != nil {
		t.Fatal(err)
	}
	attrs := map[string]string{"base_url": server.URL}
	for k, v := range c.Attrs {
		attrs[k] = v
	}
	metadata := map[string]any{}
	raw, _ := json.Marshal(c.Metadata)
	_ = json.Unmarshal(raw, &metadata)
	auth := &cliproxyauth.Auth{ID: "codex-siwc.json", Provider: "codex", Attributes: attrs, Metadata: metadata}
	headers := make(http.Header)
	for k, v := range c.Headers {
		headers.Set(k, v)
	}
	req := cliproxyexecutor.Request{Model: c.Model, Payload: []byte(c.Payload)}
	opts := cliproxyexecutor.Options{SourceFormat: sdktranslator.FromString("openai-response"), Headers: headers}
	ctx := context.Background()
	var errExec error
	switch c.Call {
	case "stream":
		opts.Stream = true
		res, errStream := NewCodexExecutor(cfg).ExecuteStream(ctx, auth, req, opts)
		errExec = errStream
		if errStream == nil {
			for range res.Chunks {
			}
		}
	case "non_stream":
		_, errExec = NewCodexExecutor(cfg).Execute(ctx, auth, req, opts)
	case "compact":
		opts.Alt = "responses/compact"
		_, errExec = NewCodexExecutor(cfg).Execute(ctx, auth, req, opts)
	case "image":
		// The Images API: a direct image model, which SIWC sends as a tool call.
		meta := map[string]any{cliproxyexecutor.RequestPathMetadataKey: "/v1/images/generations"}
		opts = cliproxyexecutor.Options{Headers: headers, OriginalRequest: req.Payload,
			SourceFormat: sdktranslator.FromString("openai-image"), Metadata: meta}
		req.Metadata = meta
		_, errExec = NewCodexExecutor(cfg).Execute(ctx, auth, req, opts)
	case "websocket":
		opts.Stream = true
		prepared, errPrepare := NewCodexWebsocketsExecutor(cfg).prepareCodexWebsocketStream(ctx, auth, req, opts)
		errExec = errPrepare
		if errPrepare == nil {
			c.Body = string(prepared.clientBody)
			c.SentHeaders = lowerHeaders(prepared.wsHeaders)
		}
	}
	if errExec != nil {
		c.Error = errExec.Error()
		if se, ok := errExec.(interface{ StatusCode() int }); ok {
			c.Status = se.StatusCode()
		}
		return c
	}
	if got != nil {
		c.Path = got.URL.Path
		c.Body = string(body)
		c.SentHeaders = lowerHeaders(got.Header)
	}
	return c
}

func lowerHeaders(h http.Header) map[string][]string {
	out := map[string][]string{}
	for k, v := range h {
		out[strings.ToLower(k)] = v
	}
	return out
}

func TestRSFixCodexSIWC(t *testing.T) {
	out := os.Getenv("RSFIX_OUT")
	if out == "" {
		t.Skip("RSFIX_OUT not set")
	}
	siwc := map[string]any{
		"use_siwc":      true,
		"access_token":  "outer-at",
		"refresh_token": "outer-rt",
		"account_id":    "acct-1",
		"siwc": map[string]any{
			"access_token":  "  siwc-at  ",
			"refresh_token": "siwc-rt",
			"client_id":     "client-1",
		},
	}
	user := `{"type":"message","role":"user","content":[{"type":"input_text","text":"hi"}]}`
	update := func(effort string) string {
		return `{"type":"configuration_update","reasoning":{"effort":"` + effort + `"}}`
	}
	clientHeaders := map[string]string{
		"Originator":         "client-originator",
		"X-Codex-Turn-State": "turn-state",
		"Version":            "0.99.0",
		"User-Agent":         "client-agent/1.0",
		"Session_id":         "client-session",
	}
	var cases []rsfixSIWCCase
	for _, c := range []rsfixSIWCCase{
		{Name: "stream_strips_fields", Call: "stream", Model: "gpt-6.1-sol", Headers: clientHeaders, Metadata: siwc,
			Payload: `{"model":"gpt-6.1-sol","input":[` + user + `],"instructions":"","store":true,"stream":false,"temperature":0.2,"max_output_tokens":100,"max_completion_tokens":50,"prompt_cache_retention":"24h","prompt_cache_options":{"mode":"x"},"prompt_cache_key":"pck-1"}`},
		{Name: "non_stream_collapses_updates", Call: "non_stream", Model: "gpt-6.1-sol", Metadata: siwc,
			Attrs:   map[string]string{"header:X-Operator": "op-1"},
			Payload: `{"model":"gpt-6.1-sol","input":[` + update("low") + `,` + user + `,` + update("high") + `],"prompt_cache_key":"pck-2"}`},
		{Name: "stream_suffix_keeps_effort", Call: "stream", Model: "gpt-6.1-sol(low)", Metadata: siwc,
			Payload: `{"model":"gpt-6.1-sol","input":[` + update("high") + `,` + user + `],"prompt_cache_key":"pck-3"}`},
		{Name: "stream_explicit_instructions", Call: "stream", Model: "gpt-6.1-sol", Metadata: siwc,
			Config:  "oauth:\n  providers:\n    codex:\n      signin-with-chatgpt-identifier: \"  MyAgent/2.0  \"\n",
			Payload: `{"model":"gpt-6.1-sol","input":[` + user + `],"instructions":"be brief","tools":[],"prompt_cache_key":"pck-4"}`},
		{Name: "compact_explicit_instructions", Call: "compact", Model: "gpt-6.1-sol", Metadata: siwc,
			Payload: `{"model":"gpt-6.1-sol","input":[` + user + `,` + update("medium") + `],"instructions":"be brief","store":true,"stream":true,"temperature":1,"prompt_cache_key":"pck-5"}`},
		{Name: "compact_blank_instructions", Call: "compact", Model: "gpt-6.1-sol", Metadata: siwc,
			Payload: `{"model":"gpt-6.1-sol","input":[` + user + `],"instructions":"  ","prompt_cache_key":"pck-6"}`},
		{Name: "websocket_turn", Call: "websocket", Model: "gpt-6.1-sol", Metadata: siwc,
			Headers: map[string]string{"OpenAI-Beta": "responses_websockets=2026-09-01", "X-Client-Request-Id": "client-req", "Originator": "client-originator", "X-Codex-Turn-State": "turn-state"},
			Payload: `{"model":"gpt-6.1-sol","input":[` + update("low") + `,` + user + `],"instructions":"","store":true,"stream":true,"background":true,"temperature":0.5,"prompt_cache_key":"pck-7"}`},
		{Name: "websocket_default_beta", Call: "websocket", Model: "gpt-6.1-sol(high)", Metadata: siwc,
			Headers: map[string]string{"OpenAI-Beta": "assistants=v2"},
			Payload: `{"model":"gpt-6.1-sol","input":[` + user + `,` + update("low") + `],"prompt_cache_key":"pck-8"}`},
		{Name: "image_direct_model_uses_the_tool", Call: "image", Model: "gpt-image-2", Metadata: siwc,
			Headers: map[string]string{"User-Agent": "client-agent/1.0"},
			Payload: `{"model":"gpt-image-2","prompt":"a cat"}`},
		{Name: "missing_token", Call: "stream", Model: "gpt-6.1-sol",
			Metadata: map[string]any{"use_siwc": "true", "access_token": "outer-at", "siwc": map[string]any{"refresh_token": "siwc-rt", "client_id": "client-1"}},
			Payload:  `{"model":"gpt-6.1-sol","input":[` + user + `]}`},
		{Name: "attribute_disables_nothing", Call: "non_stream", Model: "gpt-6.1-sol",
			Attrs:    map[string]string{"use_siwc": "false"},
			Metadata: siwc,
			Payload:  `{"model":"gpt-6.1-sol","input":[` + user + `],"prompt_cache_key":"pck-9"}`},
	} {
		cases = append(cases, rsfixSIWCRun(t, c))
	}
	data, err := json.MarshalIndent(map[string]any{"cases": cases}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(out, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(out, "codex_siwc_go.json"), append(data, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}
}
