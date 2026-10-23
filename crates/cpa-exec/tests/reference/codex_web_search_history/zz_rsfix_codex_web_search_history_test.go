package executor

// Records what CLIProxyAPI v8.0.35 (ae3ee3be) returns from
// ensureWebSearchToolForHistory (codex_executor_request.go, e7d4c731). Inputs follow
// Go's TestCodexExecutorReplayedWebSearchCallHistoryEnsuresWebSearchTool_Issue6770 plus
// the edges of the tool-type, Lite and sjson paths. See README.md.

import (
	"context"
	"encoding/json"
	"io"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"testing"

	"github.com/router-for-me/CLIProxyAPI/v8/internal/config"
	cliproxyauth "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/auth"
	cliproxyexecutor "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/executor"
	sdktranslator "github.com/router-for-me/CLIProxyAPI/v8/sdk/translator"
)

type rsfixWebSearchExec struct {
	Name    string `json:"name"`
	Call    string `json:"call"`
	Plan    string `json:"plan,omitempty"`
	NoImage bool   `json:"no_image,omitempty"`
	Lite    bool   `json:"lite_header,omitempty"`
	Payload string `json:"payload"`
	// Sent is the body Go sent upstream (WebSocket: prepared.clientBody).
	Sent string `json:"sent"`
}

// rsfixWebSearchRun replays one Issue6770 subtest and records the upstream body.
func rsfixWebSearchRun(t *testing.T, c rsfixWebSearchExec) rsfixWebSearchExec {
	var got []byte
	server := httptest.NewServer(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		got, _ = io.ReadAll(r.Body)
		if c.Call == "compact" {
			w.Header().Set("Content-Type", "application/json")
			_, _ = w.Write([]byte(`{"id":"resp_compact_1","object":"response.compaction","usage":{"input_tokens":10,"output_tokens":2,"total_tokens":12}}`))
			return
		}
		w.Header().Set("Content-Type", "text/event-stream")
		_, _ = w.Write([]byte("data: {\"type\":\"response.completed\",\"response\":{\"id\":\"resp_1\",\"object\":\"response\",\"status\":\"completed\",\"output\":[],\"usage\":{\"input_tokens\":10,\"output_tokens\":5,\"total_tokens\":15}}}\n\n"))
	}))
	defer server.Close()
	cfg := &config.Config{}
	if c.NoImage {
		cfg.SDKConfig.DisableImageGeneration = config.DisableImageGenerationAll
	}
	attrs := map[string]string{"base_url": server.URL, "api_key": "test"}
	if c.Plan != "" {
		attrs["plan_type"] = c.Plan
	}
	auth := &cliproxyauth.Auth{Provider: "codex", Attributes: attrs}
	headers := make(http.Header)
	if c.Lite {
		headers.Set(codexResponsesLiteHeader, "true")
	}
	req := cliproxyexecutor.Request{Model: "gpt-6.1-sol", Payload: []byte(c.Payload)}
	opts := cliproxyexecutor.Options{SourceFormat: sdktranslator.FromString("openai-response"), Headers: headers}
	ctx := context.Background()
	switch c.Call {
	case "stream":
		opts.Stream = true
		res, err := NewCodexExecutor(cfg).ExecuteStream(ctx, auth, req, opts)
		if err != nil {
			t.Fatal(err)
		}
		for range res.Chunks {
		}
	case "non_stream":
		if _, err := NewCodexExecutor(cfg).Execute(ctx, auth, req, opts); err != nil {
			t.Fatal(err)
		}
	case "compact":
		opts.Alt = "responses/compact"
		if _, err := NewCodexExecutor(cfg).Execute(ctx, auth, req, opts); err != nil {
			t.Fatal(err)
		}
	case "websocket":
		opts.Stream = true
		prepared, err := NewCodexWebsocketsExecutor(cfg).prepareCodexWebsocketStream(ctx, auth, req, opts)
		if err != nil {
			t.Fatal(err)
		}
		got = prepared.clientBody
	}
	c.Sent = string(got)
	return c
}

type rsfixWebSearchCase struct {
	Name string `json:"name"`
	// Lite is the X-OpenAI-Internal-Codex-Responses-Lite header value, when set.
	Lite string `json:"lite_header,omitempty"`
	In   string `json:"in"`
	// Out is omitted when Go returned the input unchanged.
	Out string `json:"out,omitempty"`
}

func TestRSFixCodexWebSearchHistory(t *testing.T) {
	out := os.Getenv("RSFIX_OUT")
	if out == "" {
		t.Skip("RSFIX_OUT not set")
	}
	var cases []rsfixWebSearchCase
	addRaw := func(name, lite, in string) {
		headers := make(http.Header)
		if lite != "" {
			headers.Set(codexResponsesLiteHeader, lite)
		}
		c := rsfixWebSearchCase{Name: name, Lite: lite, In: in}
		if got := string(ensureWebSearchToolForHistory([]byte(in), headers)); got != in {
			c.Out = got
		}
		cases = append(cases, c)
	}
	add := func(name string, lite bool, in string) {
		value := ""
		if lite {
			value = "true"
		}
		addRaw(name, value, in)
	}
	call := `{"id":"ws_1","type":"web_search_call","status":"completed","action":{"type":"search","query":"q"}}`
	user := `{"type":"message","role":"user","content":[{"type":"input_text","text":"search docs"}]}`

	// Go's Issue6770 payloads.
	add("issue6770_empty_tools", false, `{"model":"gpt-6.1-sol","input":[{"type":"message","role":"user","content":[{"type":"input_text","text":"search docs"}]},{"id":"ws_1","type":"web_search_call","status":"completed","action":{"type":"search","query":"openai responses web search"}},{"id":"ws_2","type":"web_search_call","status":"completed","action":{"type":"open_page","url":"https://platform.openai.com/docs"}},{"type":"message","role":"assistant","content":[{"type":"output_text","text":"Found docs."}]},{"type":"message","role":"user","content":[{"type":"input_text","text":"Summarize conversation"}]}],"tools":[],"tool_choice":"auto"}`)
	add("issue6770_compact_no_tools", false, `{"model":"gpt-6.1-sol","input":[{"type":"message","role":"user","content":"history"},`+call+`,{"type":"compaction_trigger"}]}`)
	add("issue6770_existing_web_search", false, `{"model":"gpt-6.1-sol","input":[`+call+`],"tools":[{"type":"web_search","external_web_access":true}]}`)
	add("issue6770_lite", true, `{"model":"gpt-6.1-sol","input":[{"type":"additional_tools","role":"developer","tools":[{"type":"namespace","name":"functions","description":"","tools":[{"type":"custom","name":"exec"}]}]},{"type":"message","role":"user","content":[{"type":"input_text","text":"search"}]},`+call+`,{"id":"rs_1","type":"reasoning","summary":[]},{"type":"message","role":"assistant","content":[{"type":"output_text","text":"answer"}]},{"type":"compaction_trigger"}],"client_metadata":{"ws_request_header_x_openai_internal_codex_responses_lite":"true"}}`)

	// Tool declarations.
	add("function_tools_append", false, `{"input":[`+user+`,`+call+`],"tools":[{"type":"function","name":"f"}]}`)
	add("tools_null", false, `{"input":[`+call+`],"tools":null}`)
	add("tools_object", false, `{"input":[`+call+`],"tools":{"type":"web_search"}}`)
	add("tools_pretty", false, "{\"input\":["+call+"],\"tools\": [\n  {\"type\": \"function\", \"name\": \"f\"}\n]\n}")
	add("tools_pretty_empty", false, "{\"input\":["+call+"],\"tools\": [ ]}")
	add("preview_tool", false, `{"input":[`+call+`],"tools":[{"type":"web_search_preview"}]}`)
	add("preview_dated_tool", false, `{"input":[`+call+`],"tools":[{"type":"function","name":"f"},{"type":"web_search_preview_2025_03_11"}]}`)
	add("spaced_tool_type", false, `{"input":[`+call+`],"tools":[{"type":" web_search "}]}`)
	add("other_dated_tool", false, `{"input":[`+call+`],"tools":[{"type":"web_search_2025_08_26"}]}`)
	add("uppercase_tool_type", false, `{"input":[`+call+`],"tools":[{"type":"WEB_SEARCH"}]}`)

	// Which inputs count.
	add("mention_only", false, `{"input":[{"type":"message","role":"user","content":"web_search_call"}],"tools":[]}`)
	add("spaced_item_type", false, `{"input":[{"type":" web_search_call ","id":"ws_1"}]}`)
	add("uppercase_item_type", false, `{"input":[{"type":"WEB_SEARCH_CALL","id":"ws_1"}]}`)
	add("input_string", false, `{"input":"web_search_call","tools":[]}`)
	add("input_object", false, `{"input":{"type":"web_search_call"}}`)
	add("no_input", false, `{"tools":[{"type":"web_search_call"}]}`)

	// Responses Lite.
	add("lite_metadata_spaced", false, `{"input":[`+user+`,`+call+`],"client_metadata":{"ws_request_header_x_openai_internal_codex_responses_lite":" TRUE "}}`)
	add("lite_metadata_bool", false, `{"input":[`+call+`,`+user+`],"client_metadata":{"ws_request_header_x_openai_internal_codex_responses_lite":true}}`)
	add("lite_metadata_false", false, `{"input":[`+call+`],"client_metadata":{"ws_request_header_x_openai_internal_codex_responses_lite":"false"}}`)
	add("lite_existing_tool", true, `{"input":[`+user+`,`+call+`],"tools":[{"type":"web_search"}]}`)
	add("lite_only_calls", true, `{"input":[`+call+`,`+call+`]}`)
	// Malformed arrays: ForEach stops after {} where Array() does not.
	add("malformed_input_standard", false, `{"model":"gpt-6.1-sol","instructions":"","input":[{}{"type":"web_search_call"}]}`)
	add("malformed_input_lite", true, `{"model":"gpt-6.1-sol","instructions":"","input":[{}{"type":"web_search_call"}]}`)
	add("malformed_after_call_lite", true, `{"input":[{"type":"web_search_call"},{}{"type":"web_search_call"},{"type":"message"}]}`)
	add("malformed_tools", false, `{"input":[{"type":"web_search_call"}],"tools":[{}{"type":"web_search"}]}`)
	// Go's TrimSpace strips U+00A0 around the header value.
	addRaw("lite_header_nbsp", "\u00a0true\u00a0", `{"input":[{"type":"web_search_call"}]}`)
	addRaw("lite_header_upper", " TRUE ", `{"input":[{"type":"web_search_call"}]}`)
	add("lite_pretty_input", true, "{\"input\": [\n  "+user+",\n  "+call+",\n  {\"type\": \"message\",  \"role\": \"assistant\", \"content\": []}\n],\"tools\":[]}")

	emptyTools := cases[0].In
	compact := cases[1].In
	existing := cases[2].In
	lite := cases[3].In
	var execs []rsfixWebSearchExec
	for _, c := range []rsfixWebSearchExec{
		{Name: "stream_empty_tools", Call: "stream", Plan: "plus", Payload: emptyTools},
		{Name: "non_stream_no_image", Call: "non_stream", NoImage: true, Payload: emptyTools},
		{Name: "compact", Call: "compact", Payload: compact},
		{Name: "websocket_empty_tools", Call: "websocket", Plan: "plus", Payload: emptyTools},
		{Name: "websocket_existing_tool", Call: "websocket", NoImage: true, Payload: existing},
		{Name: "websocket_lite", Call: "websocket", Lite: true, Payload: lite},
		{Name: "stream_no_tools_no_instructions", Call: "stream", Plan: "plus", Payload: compact},
	} {
		execs = append(execs, rsfixWebSearchRun(t, c))
	}

	data, err := json.MarshalIndent(map[string]any{"cases": cases, "executor": execs}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(out, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(out, "codex_web_search_history_go.json"), append(data, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}
}
