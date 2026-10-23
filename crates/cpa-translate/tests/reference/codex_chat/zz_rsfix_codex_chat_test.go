package chat_completions

// Records what CLIProxyAPI v8.0.35 (ae3ee3be) returns from the OpenAI Chat Completions
// <-> Codex converters for b0debb35 (over-length tool call IDs) and 16d98881 (URL
// citations). Inputs follow Go's tests for both commits plus edge cases. See README.md.

import (
	"context"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
)

type rsfixChatRequest struct {
	Name   string `json:"name"`
	Model  string `json:"model"`
	Stream bool   `json:"stream"`
	In     string `json:"in"`
	Out    string `json:"out"`
}

type rsfixChatStream struct {
	Name  string     `json:"name"`
	Lines []string   `json:"lines"`
	Out   [][]string `json:"out"`
}

type rsfixChatNonStream struct {
	Name     string `json:"name"`
	Original string `json:"original,omitempty"`
	In       string `json:"in"`
	Out      string `json:"out"`
}

func TestRSFixCodexChat(t *testing.T) {
	dir := os.Getenv("RSFIX_OUT")
	if dir == "" {
		t.Skip("RSFIX_OUT not set")
	}
	var requests []rsfixChatRequest
	var streams []rsfixChatStream
	var nonStreams []rsfixChatNonStream

	request := func(name, in string) {
		out, err := ConvertOpenAIRequestToCodex("gpt-6.1-sol", []byte(in), true)
		if err != nil {
			t.Fatalf("%s: %v", name, err)
		}
		requests = append(requests, rsfixChatRequest{Name: name, Model: "gpt-6.1-sol", Stream: true, In: in, Out: string(out)})
	}
	calls := func(ids ...string) string {
		var tc, results []string
		for i, id := range ids {
			name := string(rune('a' + i))
			tc = append(tc, `{"id":`+quote(id)+`,"type":"function","function":{"name":"tool_`+name+`","arguments":"{}"}}`)
			results = append(results, `{"role":"tool","tool_call_id":`+quote(id)+`,"content":"res_`+name+`"}`)
		}
		return `{"model":"gpt-6.1-sol","messages":[{"role":"user","content":"Multi-tool"},{"role":"assistant","content":null,"tool_calls":[` +
			strings.Join(tc, ",") + `]},` + strings.Join(results, ",") + `]}`
	}
	long := strings.Repeat("a", 70)
	request("over_length_ids", calls(long+"_A", long+"_B", "short_id"))
	request("shortened_collides_with_legal_id", calls(long+"_A", shortenCodexCallIDIfNeeded(long+"_A")))
	request("collision_chain", calls(long+"_A", shortenCodexCallIDIfNeeded(long+"_A"), shortenCodexCallIDIfNeeded(long+"_A_1")))
	request("exactly_64", calls(strings.Repeat("b", 64), strings.Repeat("c", 65)))
	request("multibyte_cut", calls(strings.Repeat("é", 40)))
	request("missing_id_kept", `{"model":"m","messages":[{"role":"assistant","tool_calls":[{"type":"function","function":{"name":"t","arguments":"{}"}},{"id":"`+long+`","type":"custom","custom":{"name":"c","input":"x"}}]},{"role":"tool","tool_call_id":"`+long+`","content":"r"}]}`)
	request("duplicate_long_ids_dropped", calls(long+"_D", long+"_D"))

	stream := func(name string, lines ...string) {
		var param any
		s := rsfixChatStream{Name: name, Lines: lines}
		for _, line := range lines {
			out := ConvertCodexResponseToOpenAI(context.Background(), "gpt-5.5", nil, nil, []byte(line), &param)
			chunk := []string{}
			for _, b := range out {
				chunk = append(chunk, string(b))
			}
			s.Out = append(s.Out, chunk)
		}
		streams = append(streams, s)
	}
	delta := func(text string) string {
		return `data: {"type":"response.output_text.delta","delta":` + quote(text) + `}`
	}
	created := `data: {"type":"response.created","response":{"id":"resp_1","created_at":1700000000,"model":"gpt-5.5"}}`
	stream("annotation_added", created, delta("前🙂"),
		`data: {"type":"response.output_text.annotation.added","annotation_index":0,"annotation":{"type":"url_citation","url":"https://example.com","title":"Example","start_index":0,"end_index":1}}`,
		delta("引用"),
		`data: {"type":"response.output_text.annotation.added","annotation_index":0,"annotation":{"type":"url_citation","url":"https://example.com","title":"Example","start_index":0,"end_index":2}}`)
	stream("completion_annotations", created, delta("前🙂"), delta("引用"),
		`data: {"type":"response.output_text.done","text":"前🙂引用","annotations":[{"type":"url_citation","url":"https://example.com","title":"Example","start_index":0,"end_index":2}]}`,
		`data: {"type":"response.content_part.done","part":{"type":"output_text","text":"前🙂引用","annotations":[{"type":"url_citation","url":"https://other.example","title":"Other","start_index":0,"end_index":1}]}}`,
		`data: {"type":"response.output_item.done","item":{"type":"message","content":[{"type":"output_text","text":"前🙂引用","annotations":[{"type":"url_citation","url":"https://example.com","title":"Example","start_index":0,"end_index":2}]}]}}`)
	stream("annotation_edges", created, delta("ab\xff"),
		`data: {"type":"response.output_text.annotation.added","annotation":{"type":"file_citation","file_id":"f","start_index":0,"end_index":1}}`,
		`data: {"type":"response.output_text.annotation.added","annotation":{"type":"url_citation","start_index":"1","end_index":2.9,"id":"cite_1"}}`,
		`data: {"type":"response.output_text.annotation.added","annotation":{"type":"url_citation","url":"https://x","start_index":5,"end_index":1}}`,
		`data: {"type":"response.output_text.annotation.added","annotation":{"type":"url_citation","url":"https://x","start_index":-9,"end_index":1}}`,
		`data: {"type":"response.output_text.annotation.added","annotation":{"type":"url_citation","url":"https://y","id":"cite_1","start_index":7,"end_index":8}}`,
		`data: {"type":"response.output_text.annotation.added","annotations":[{"type":"url_citation","url":"https://z","title":"<z&>","start_index":0,"end_index":0},{"type":"url_citation","url":"https://z","start_index":0,"end_index":3}]}`,
		`data: {"type":"response.output_item.done","item":{"type":"message","annotations":{"type":"url_citation","url":"https://w","start_index":1,"end_index":1},"content":[]}}`,
		// A missing comma: ForEach stops after the first element, Array() would not.
		`data: {"type":"response.output_text.done","annotations":[{}{"type":"url_citation","url":"https://m","start_index":0,"end_index":1}]}`,
		`data: {"type":"response.output_item.done","item":{"type":"message","content":[{"type":"output_text","text":"x"}]}}`,
		`data: {"type":"response.content_part.done","part":{"type":"output_text","text":"x"}}`,
		`data: {"type":"response.output_text.done","text":"x"}`)

	nonStreamWith := func(name, original, in string) {
		var originalJSON []byte
		if original != "" {
			originalJSON = []byte(original)
		}
		out := ConvertCodexResponseToOpenAINonStream(context.Background(), "gpt-5.5", originalJSON, nil, []byte(in), nil)
		nonStreams = append(nonStreams, rsfixChatNonStream{Name: name, Original: original, In: in, Out: string(out)})
	}
	nonStream := func(name, in string) { nonStreamWith(name, "", in) }
	nonStream("citations", `{"type":"response.completed","response":{"id":"resp_citation","created_at":1700000000,"model":"gpt-5.5","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":"前🙂"},{"type":"output_text","text":"引用","annotations":[{"type":"url_citation","url":"https://example.com","title":"Example","start_index":0,"end_index":2},{"type":"file_citation","file_id":"file_1","index":0}]}]}]}}`)
	nonStream("text_across_messages", `{"type":"response.completed","response":{"id":"r","created_at":1700000000,"model":"gpt-5.5","status":"completed","output":[{"type":"message","content":[{"type":"output_text","text":""},{"type":"refusal","refusal":"no"},{"type":"output_text","text":"one","annotations":[{"type":"url_citation","url":"https://a","start_index":0,"end_index":3},{"type":"url_citation","url":"https://a","start_index":0,"end_index":3}]}]},{"type":"message","content":[{"type":"output_text","text":"two","annotations":{"type":"url_citation","url":"https://b","title":"B","start_index":1,"end_index":2}}]}]}}`)
	nonStream("no_text_only_annotations", `{"type":"response.completed","response":{"id":"r","created_at":1700000000,"status":"completed","output":[{"type":"message","content":[{"type":"output_text","annotations":[{"type":"url_citation","id":"c","start_index":0,"end_index":0}]}]}]}}`)

	// The pair golden's codex-events/non-stream/0 (recorded at 6fecc6e), which 16d98881
	// changes: every output_text part now joins the content.
	nonStreamWith("pair_codex_events_non_stream_0", `{"tools":[{"type":"custom","name":"apply_patch"},{"type":"custom","name":"freeform"},{"type":"function","function":{"name":"mcp__srv__read"}},{"type":"function","function":{"name":"lookup"}}]}`, `{"type":"response.completed","response":{"id":"resp_1","created_at":1700000000,"model":"gpt-5.3-codex","status":"completed","service_tier":"priority","usage":{"input_tokens":10,"output_tokens":5,"total_tokens":15,"input_tokens_details":{"cached_tokens":3,"cache_write_tokens":2},"output_tokens_details":{"reasoning_tokens":4}},"output":[{"type":"reasoning","summary":[{"type":"summary_text","text":"s1"},{"type":"summary_text","text":"s2"}],"content":[{"type":"reasoning_text","text":"r1"},{"type":"reasoning_text","text":"r2"}]},{"type":"message","content":[{"type":"refusal"},{"type":"output_text","text":"t1"},{"type":"output_text","text":"t2"}]},{"type":"function_call","call_id":"c1","name":"lookup","arguments":"{}"},{"type":"custom_tool_call","call_id":"c2","name":"apply_patch","input":"patch <x>"},{"type":"custom_tool_call","name":"freeform","input":"raw"},{"type":"image_generation_call","result":"AAA","output_format":"gif"},{"type":"image_generation_call","result":""}]}}`)

	data, err := json.MarshalIndent(map[string]any{"requests": requests, "streams": streams, "non_stream": nonStreams}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(dir, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(dir, "codex_chat_go.json"), append(data, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}
}

func quote(s string) string {
	b, _ := json.Marshal(s)
	return string(b)
}
