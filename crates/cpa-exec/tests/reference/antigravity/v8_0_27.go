//go:build v8027

// Scenarios for the Antigravity behaviour ported from CLIProxyAPI v8.0.27. They need the
// v8.0.27 tree, so they build only with `-tags v8027` and run with
// ANTIGRAVITY_GO_SET=v8.0.27 (README.md).
package main

import (
	"github.com/router-for-me/CLIProxyAPI/v8/internal/signature/signaturetest"
)

func init() {
	scenarioSets["v8.0.27"] = v8027Scenarios
}

func v8027Scenarios() []execScenario {
	answer := func(model, trace string) string {
		return `{"response":{"candidates":[{"content":{"role":"model","parts":[{"thought":true,"text":"checked limits"},{"text":"34113"}]},"finishReason":"STOP"}],"modelVersion":"` + model + `","usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":2,"totalTokenCount":12}},"traceId":"` + trace + `"}`
	}
	// Claude 5.5's double-base64 CAQS thinking signature (Go 9b9c2bdf and 174248ca), as
	// Go's own tests build it: version 4, channel 18, Google infrastructure, signature
	// bytes in the container.
	caqs := signaturetest.AntigravityCAQS()
	session := map[string]string{"X-Claude-Code-Session-Id": "caqs-replay-session"}
	thought := func(model, trace string) string {
		return `{"response":{"candidates":[{"content":{"role":"model","parts":[{"thought":true,"text":"checked calculation","thoughtSignature":"` + caqs + `"},{"text":"34113"}]},"finishReason":"STOP"}],"modelVersion":"` + model + `","usageMetadata":{"promptTokenCount":10,"candidatesTokenCount":2,"totalTokenCount":12}},"traceId":"` + trace + `"}`
	}
	history := func(model, signature string) string {
		return `{"model":"` + model + `","max_tokens":4096,"thinking":{"type":"enabled","budget_tokens":1024},"messages":[{"role":"user","content":"calculate"},{"role":"assistant","content":[{"type":"thinking","thinking":"checked calculation","signature":"` + signature + `"},{"type":"text","text":"34113"}]},{"role":"user","content":"plus 17"}]}`
	}
	// The envelope with its last outer characters replaced: still base64, no longer a
	// valid inner encoding.
	broken := caqs[:len(caqs)-6] + "AAAAAA"
	// Go 4e4dede4: a payload split over lines (whose served model is reported), and a
	// backend error object mid-stream.
	stream := func(body string) *upstream {
		return &upstream{Status: 200, Headers: [][2]string{{"Content-Type", "text/event-stream"}}, Body: body}
	}
	partial := `data: {"response":{"candidates":[{"content":{"parts":[{"text":"partial"}]}}],"modelVersion":"gemini-3.7-flash"}}` + "\n\n"
	backendError := func(code string) string {
		return partial + "{\n  \"error\": {\n    \"code\": " + code + ",\n    \"message\": \"backend capacity exhausted\",\n    \"status\": \"UNAVAILABLE\"\n  }\n}\n"
	}
	split := partial + "data: {\"response\":{\"candidates\":[{\"content\":{\"role\":\"model\",\n" +
		"data: \"parts\":[{\"text\":\" joined\"}]},\n\n" +
		"  \"finishReason\":\"STOP\"}],\"usageMetadata\":{\"promptTokenCount\":3,\"candidatesTokenCount\":2,\"totalTokenCount\":5},\n" +
		"\"modelVersion\":\"gemini-3.7-flash-joined\"}}\n\n"
	// OpenAI chat clients are left out: v8.0.27's stream chunks also differ by dfe0a0ef,
	// which is not ported.
	geminiHello := `{"contents":[{"role":"user","parts":[{"text":"hello"}]}]}`
	claudeHello := `{"model":"gemini-3.7-flash","max_tokens":256,"stream":true,"messages":[{"role":"user","content":"hello"}]}`
	quota := `{"error":{"code":429,"message":"You have exhausted your capacity on this model.","status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"12s"}]}}`
	return []execScenario{
		{Name: "stream_backend_error_claude", Op: "stream", Source: "claude", Model: "gemini-3.7-flash",
			GoTests: []string{"TestAntigravityStreamSurfacesTrailingBackendError"},
			Payload: claudeHello, Upstream: stream(backendError("503"))},
		{Name: "stream_backend_error_code_out_of_range", Op: "stream", Source: "gemini", Model: "gemini-3.7-flash",
			Payload: geminiHello, Upstream: stream(backendError("200"))},
		{Name: "stream_backend_error_quota", Op: "stream", Source: "claude", Model: "gemini-3.7-flash",
			Payload: claudeHello, Upstream: stream(partial + "data: " + quota + "\n")},
		{Name: "stream_split_payload_gemini", Op: "stream", Source: "gemini", Model: "gemini-3.7-flash",
			Payload: geminiHello, Upstream: stream(split)},
		{Name: "stream_split_payload_claude", Op: "stream", Source: "claude", Model: "gemini-3.7-flash",
			Payload: claudeHello, Upstream: stream(split)},
		// Go's TestAntigravityExecutorCAQSTwoTurnReplay, streamed and not: the first turn
		// hands the client the envelope unchanged, the second sends it back upstream as is.
		{Name: "caqs_turn1_stream", Op: "stream", Source: "claude", Model: "claude-opus-5-5-high", Headers: session,
			GoTests:  []string{"TestAntigravityExecutorCAQSTwoTurnReplay"},
			Payload:  `{"model":"claude-opus-5-5-high","max_tokens":4096,"stream":true,"thinking":{"type":"enabled","budget_tokens":1024},"messages":[{"role":"user","content":"calculate"}]}`,
			Upstream: sseOK(thought("claude-opus-5-5-high", "t-caqs-1"))},
		{Name: "caqs_turn2_replays_signature", Op: "execute", Source: "claude", Model: "claude-opus-5-5-high", Headers: session,
			GoTests:  []string{"TestAntigravityExecutorCAQSTwoTurnReplay"},
			Payload:  history("claude-opus-5-5-high", caqs),
			Upstream: sseOK(thought("claude-opus-5-5-high", "t-caqs-2"))},
		{Name: "caqs_turn1_execute", Op: "execute", Source: "claude", Model: "claude-sonnet-5-5-high",
			GoTests:  []string{"TestAntigravityExecutorCAQSTwoTurnReplay"},
			Payload:  `{"model":"claude-sonnet-5-5-high","max_tokens":4096,"thinking":{"type":"enabled","budget_tokens":1024},"messages":[{"role":"user","content":"calculate"}]}`,
			Upstream: sseOK(thought("claude-sonnet-5-5-high", "t-caqs-3"))},
		// A cache prefix is stripped; an envelope that no longer decodes drops its block.
		{Name: "caqs_turn2_cache_prefixed_signature", Op: "execute", Source: "claude", Model: "claude-sonnet-5-5-high",
			Payload:  history("claude-sonnet-5-5-high", "claude#"+caqs),
			Upstream: sseOK(thought("claude-sonnet-5-5-high", "t-caqs-4"))},
		{Name: "caqs_turn2_broken_signature", Op: "execute", Source: "claude", Model: "claude-opus-5-5-high",
			Payload:  history("claude-opus-5-5-high", broken),
			Upstream: sseOK(thought("claude-opus-5-5-high", "t-caqs-5"))},
		// The Claude 5.5 models of the static catalog (Go e3abd9ae, c575291a, 6b037f61,
		// 8a945b3f): a thinking budget above their 64,000 maximum and an output limit
		// above their 128,000 tokens.
		{Name: "claude55_thinking_budget_above_max", Op: "execute", Source: "claude", Model: "claude-opus-5-5-high",
			Payload:  `{"model":"claude-opus-5-5-high","max_tokens":200000,"thinking":{"type":"enabled","budget_tokens":100000},"messages":[{"role":"user","content":"limits"}]}`,
			Upstream: sseOK(answer("claude-opus-5-5-high", "t-limits-1"))},
		{Name: "claude55_openai_reasoning_effort", Op: "execute", Source: "openai", Model: "claude-sonnet-5-5-high",
			Payload:  `{"model":"claude-sonnet-5-5-high","reasoning_effort":"high","messages":[{"role":"user","content":"effort"}]}`,
			Upstream: sseOK(answer("claude-sonnet-5-5-high", "t-limits-2"))},
		{Name: "claude55_gemini_stream", Op: "stream", Source: "gemini", Model: "claude-sonnet-5-5-high",
			Payload:  `{"contents":[{"role":"user","parts":[{"text":"limits"}]}],"generationConfig":{"maxOutputTokens":500000,"thinkingConfig":{"thinkingBudget":-1}}}`,
			Upstream: sseOK(answer("claude-sonnet-5-5-high", "t-limits-3"))},
	}
}
