package helps

// Records what CLIProxyAPI v8.0.35 (ae3ee3be) returns from NormalizeCodexToolSchemas
// (helps/codex_tool_schema.go, with 57d005fe's int64 bound stripping) and
// NormalizeCodexToolIntegerTypes (client/codex/tool-schema, with d7914afd's field
// paths and namespaces). Inputs follow Go's own tests for both commits. See README.md.

import (
	"bytes"
	"encoding/json"
	"fmt"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"

	"github.com/tidwall/gjson"
)

type rsfixToolSchemaCase struct {
	Name string `json:"name"`
	UA   string `json:"ua,omitempty"`
	In   string `json:"in"`
	// Out is omitted when Go returned the input unchanged.
	Out string `json:"out,omitempty"`
}

func rsfixCase(name, ua, in string, out []byte) rsfixToolSchemaCase {
	c := rsfixToolSchemaCase{Name: name, UA: ua, In: in}
	if string(out) != in {
		c.Out = string(out)
	}
	return c
}

func TestRSFixCodexToolSchema(t *testing.T) {
	out := os.Getenv("RSFIX_OUT")
	if out == "" {
		t.Skip("RSFIX_OUT not set")
	}
	var schemas, integers []rsfixToolSchemaCase
	schema := func(name, in string) {
		schemas = append(schemas, rsfixCase(name, "", in, NormalizeCodexToolSchemas([]byte(in))))
	}
	integer := func(name, ua, in string) {
		var headers http.Header
		if ua != "" {
			headers = http.Header{"User-Agent": []string{ua}}
		}
		integers = append(integers, rsfixCase(name, ua, in, NormalizeCodexToolIntegerTypes([]byte(in), headers)))
	}

	// 57d005fe: TestNormalizeCodexToolSchemas_StripsOutOfRangeInt64Bounds.
	schema("int64_bounds", `{
		"model": "gpt-6-luna",
		"tools": [{
			"type": "function",
			"name": "emulate_process",
			"description": "Emulate a process.",
			"parameters": {
				"type": "object",
				"properties": {
					"options": {
						"type": "object",
						"properties": {
							"seed": {"type": "integer", "minimum": 0, "maximum": 18446744073709552000},
							"over_max_int64": {"type": "integer", "minimum": -9223372036854775809, "maximum": 9223372036854775808, "exclusiveMinimum": -9223372036854775809, "exclusiveMaximum": 9223372036854775808},
							"exact_int64_limits": {"type": "integer", "minimum": -9223372036854775808, "maximum": 9223372036854775807, "exclusiveMinimum": -9223372036854775808, "exclusiveMaximum": 9223372036854775807},
							"float_bounds": {"type": "number", "minimum": -0.5, "maximum": 3.14, "exclusiveMinimum": 0.25, "exclusiveMaximum": 99.95},
							"scientific_bounds": {"type": "number", "minimum": 1e5, "maximum": 1e100, "exclusiveMinimum": -1.8446744073709552e+19, "exclusiveMaximum": 18446744073709552000.0},
							"high_precision_fraction": {"type": "number", "maximum": 18446744073709552000.00000000000000000000000000000000000000000000000000000000000000000000000000000001},
							"extreme_exponents": {"type": "number", "minimum": 1.1e-9223372036854775808, "maximum": 10e9223372036854775807},
							"user_data": {"type": "object", "default": {"maximum": 18446744073709552000}, "enum": [{"maximum": 18446744073709552000}]},
							"maximum": {"type": "integer", "minimum": 0}
						}
					}
				},
				"$defs": {"uint64_id": {"type": "integer", "maximum": 18446744073709551615}}
			}
		}]
	}`)
	// Boundary numbers, one property each, so each verdict shows separately.
	for i, n := range []string{
		"9223372036854775807", "9223372036854775808", "-9223372036854775808", "-9223372036854775809",
		"0009223372036854775807", "9.223372036854775807e18", "9.223372036854775808e18", "-9.223372036854775808E18",
		"92233720368547758070e-1", "92233720368547758080e-1", "1e19", "1e18", "1E+19", "0e999999999999999999999",
		"1e999999999999999999999", "1e-999999999999999999999", "0.0", "-0.0e5", "1.5", "100e-2", "1e-2",
		"12345678901234567890.5", "0.00000000000000000001e40", "1e40", "1e41",
	} {
		schema(fmt.Sprintf("bound_%02d", i), `{"tools":[{"type":"function","name":"f","parameters":{"type":"object","properties":{"n":{"type":"number","maximum":`+n+`,"exclusiveMinimum":`+n+`}}}}]}`)
	}
	// Unsupported regex escapes, schema-aware traversal, re-encoding (sorted keys, no
	// HTML escaping, last duplicate wins), and union collapse after the strip.
	schema("patterns", `{"tools":[{"type":"function","name":"p","parameters":{"type":"object","z":"<a&b>","properties":{"word":{"type":"string","pattern":"^\\p{L}+$","description":"x < y"},"nul":{"type":"string","pattern":"a\\0b"},"escaped":{"type":"string","pattern":"a\\\\p{L}"},"ok":{"type":"string","pattern":"^[a-z]+$"},"data":{"type":"object","default":{"pattern":"\\p{L}"},"enum":[{"pattern":"\\p{L}"}]},"arr":{"type":"array","items":[{"type":"string","pattern":"\\P{Lu}"}],"prefixItems":[{"pattern":"\\0"}]}},"patternProperties":{"^\\p{L}+$":{"type":"string"},"^x$":{"type":"string","pattern":"\\p{N}"}},"$defs":{"d":{"pattern":"\\p{L}","minimum":1e30}},"dup":1,"dup":2}}]}`)
	schema("unicode_escape_only", `{"tools":[{"type":"function","name":"u","parameters":{"type":"object","properties":{"w":{"type":"string","pattern":"\u005cp{L}","title":"caf\u00e9"}}}}]}`)
	schema("no_candidates", `{"tools":[{"type":"function","name":"n","parameters":{"type":"object","properties":{"a":{"type":"integer","maximum":10,"description":"<keep>"}}}}]}`)
	schema("in_range_bounds_untouched", `{"tools":[{"type":"function","name":"n","parameters":{"properties":{"a":{"maximum":10,  "minimum":-5}},"z":1,"a":2}}]}`)
	var branches []string
	for i := 0; i < 9; i++ {
		branches = append(branches, fmt.Sprintf(`{"const":"v%d","description":"d"}`, i))
	}
	schema("strip_then_union", `{"tools":[{"type":"namespace","name":"ns","tools":[{"type":"custom","name":"c","parameters":{"type":"object","properties":{"mode":{"oneOf":[`+strings.Join(branches, ",")+`]},"big":{"type":"integer","maximum":99999999999999999999}}}}]},{"type":"web_search"}]}`)
	schema("non_object_params", `{"tools":[{"type":"function","name":"x","parameters":[{"maximum":1e30}]},{"type":"function","name":"y","parameters":{"maximum":"1e30","minimum":true}}]}`)

	// d7914afd: TestNormalizeCodexToolIntegerTypesSourceFields.
	sourceFields := []struct {
		name   string
		fields []string
	}{
		{"test_sync_tool", []string{"barrier.properties.participants", "barrier.properties.timeout_ms"}},
		{"create_goal", []string{"token_budget"}},
		{"get_channels", []string{"limit"}},
		{"list_threads", []string{"limit", "max_chars_per_post"}},
		{"search_posts", []string{"limit", "max_chars_per_post"}},
		{"read_thread", []string{"limit", "max_chars_per_post"}},
		{"read_post", []string{"offset_chars", "limit_chars"}},
		{"memories__list", []string{"max_results"}},
		{"memories__read", []string{"line_offset", "max_lines"}},
		{"memories__search", []string{"context_lines", "max_results"}},
		{"history__list_windows", []string{"limit"}},
		{"history__list_items", []string{"limit", "max_chars_per_item"}},
		{"history__read_item", []string{"offset_chars", "limit_chars"}},
		{"history__search_contents", []string{"limit"}},
		{"notes__list_files_by_prefix", []string{"max_results"}},
		{"notes__read_file", []string{"start_line", "stop_line", "start_line.anyOf.0", "stop_line.anyOf.0"}},
		{"notes__search_contents", []string{"max_matches_per_file", "max_files"}},
		{"image_gen__imagegen", []string{"num_last_images_to_include"}},
		{"web__run", []string{
			"search_query.items.properties.recency", "image_query.items.properties.recency",
			"open.items.properties.lineno", "click.items.properties.id",
			"screenshot.items.properties.pageno", "weather.items.properties.duration",
			"sports.items.properties.num_games",
		}},
		{"collaboration__wait_agent", []string{"timeout_ms"}},
		{"multi_agent_v1__wait_agent", []string{"timeout_ms"}},
		{"functions__create_goal", []string{"token_budget"}},
		{"collab__read_post", []string{"offset_chars", "limit_chars"}},
		{"collaboration__get_channels", []string{"limit"}},
		{"collaboration__list_threads", []string{"limit", "max_chars_per_post"}},
		{"collaboration__search_posts", []string{"limit", "max_chars_per_post"}},
		{"collaboration__read_thread", []string{"limit", "max_chars_per_post"}},
		{"collaboration__read_post", []string{"offset_chars", "limit_chars"}},
		{"exec_command", []string{"yield_time_ms", "max_output_tokens", "timeout_ms"}},
		{"functions__write_stdin", []string{"session_id"}},
		{"tool_search", []string{"limit"}},
	}
	for _, tt := range sourceFields {
		for _, typeJSON := range []string{`"number"`, `["number","integer","null"]`} {
			// The declared fields nest by path; build the properties tree by hand.
			props := map[string]any{"unrelated": map[string]any{"type": "number", "default": 1.5}}
			for _, field := range tt.fields {
				setRSFixPath(props, strings.Split(field, "."), json.RawMessage(`{"type":`+typeJSON+`,"description":"Keep metadata","minimum":0,"default":1}`))
			}
			raw, _ := json.Marshal(props)
			in := fmt.Sprintf(`{"tools":[{"type":"function","name":%q,"parameters":{"type":"object","properties":%s},"output_schema":{"properties":{"wall_time_seconds":{"type":"number"}}}}],"input":[{"type":"function_call","arguments":"{\"limit\":1.5}"}]}`, tt.name, raw)
			integer("source/"+tt.name, "codex-tui/0.154.0", in)
			if tt.name == "test_sync_tool" {
				integer("source/"+tt.name, "curl/8.7.1", in)
				integer("source/"+tt.name, "", in)
			}
		}
	}

	// TestNormalizeCodexToolIntegerTypesNamespaceFormats.
	nsSchema := `{"type":"object","properties":{"line_offset":{"type":"number"},"max_lines":{"type":["number","null"]},"max_tokens":{"type":"number"}}}`
	for _, body := range []string{
		`{"tools":[{"type":"function","name":"memories__read","parameters":%s}]}`,
		`{"tools":[{"type":"function","function":{"name":"memories__read","parameters":%s}}]}`,
		`{"tools":[{"name":"memories__read","input_schema":%s}]}`,
		`{"tools":[{"function_declarations":[{"name":"memories__read","parameters":%s}]}]}`,
		`{"tools":[{"functionDeclarations":[{"name":"memories__read","parametersJsonSchema":%s}]}]}`,
		`{"tools":[{"type":"namespace","name":"memories","tools":[{"type":"function","name":"read","parameters":%s}]}]}`,
		`{"input":[{"type":"additional_tools","tools":[{"type":"namespace","name":"memories","tools":[{"type":"function","name":"read","parameters":%s}]}]}]}`,
		`{"tools":[{"type":"namespace","name":"memories","tools":[{"function_declarations":[{"name":"read","parameters":%s}]}]}]}`,
		`{"tools":[{"type":"namespace","name":"outer","tools":[{"type":"namespace","name":"memories","tools":[{"type":"function","name":"read","parameters":%s}]}]}]}`,
		`{"tools":[{"type":"namespace","tools":[{"type":"function","name":"memories__read","parameters":%s}]}]}`,
		`{"tools":[{"type":"namespace","name":"functions","tools":[{"type":"function","name":"memories__read","parameters":%s}]}]}`,
		`{"tools":[{"type":"namespace","name":"collab","tools":[{"type":"function","name":"wait_agent","parameters":{"properties":{"timeout_ms":{"type":"number"}}}},{"type":"function","name":"read_post","parameters":%s}]}]}`,
	} {
		integer("namespace", "Codex/1.0", fmt.Sprintf(body, nsSchema))
	}

	// TestNormalizeCodexToolIntegerTypesPreservesUnprovenFields.
	for _, tt := range []struct{ name, namespace string }{
		{"unknown_tool", ""}, {"mcp__server__read_post", ""},
		{"read", ""}, {"run", ""}, {"imagegen", ""},
		{"read_post", "user_tools"}, {"wait_agent", "mcp__server"},
		{"read", "skills"}, {"read", "user_tools"},
		{"read_post", "multi_agent_v1"}, {"read_post", "arbitrary_collaboration"},
	} {
		tool := fmt.Sprintf(`{"type":"function","name":%q,"parameters":{"properties":{"limit":{"type":"number"},"offset_chars":{"type":"number"},"line_offset":{"type":"number"},"timeout_ms":{"type":"number"}}}}`, tt.name)
		if tt.namespace != "" {
			tool = fmt.Sprintf(`{"type":"namespace","name":%q,"tools":[%s]}`, tt.namespace, tool)
		}
		integer("unproven", "codex", `{"tools":[`+tool+`]}`)
	}
	integer("nested_path", "codex", `{"tools":[{"name":"test_sync_tool","parameters":{"properties":{"barrier.participants":{"type":"number"},"other":{"properties":{"participants":{"type":"number"}}},"barrier":{"properties":{"participants":{"type":"number"},"ratio":{"type":"number"}}}}}}]}`)
	integer("notes_union", "codex", `{"tools":[{"name":"notes__read_file","parameters":{"type":"object","properties":{"start_line":{"anyOf":[{"type":"number"},{"type":"null"}],"default":-3},"stop_line":{"anyOf":[{"type":"number"},{"type":"number"}],"default":-1},"ratio":{"anyOf":[{"type":"number"},{"type":"null"}]},"other":{"properties":{"start_line":{"anyOf":[{"type":"number"}]}}}},"required":["path"]}}],"input":[{"type":"function_call","arguments":"{\"start_line\":-3,\"stop_line\":-1}"}]}`)

	// TestNormalizeCodexToolIntegerTypesHistoryNotesSchemas.
	// Compacted to keep the golden small; integer declarations degrade to number.
	pretty, err := os.ReadFile("../../../client/codex/tool-schema/testdata/history_notes_tools.json")
	if err != nil {
		t.Fatal(err)
	}
	var compact bytes.Buffer
	if err := json.Compact(&compact, pretty); err != nil {
		t.Fatal(err)
	}
	fixture := compact.Bytes()
	for _, namespace := range gjson.GetBytes(fixture, "tools").Array() {
		tool := namespace.Get("tools.0")
		name := namespace.Get("name").String() + "__" + tool.Get("name").String()
		degrade := func(s string) string { return strings.ReplaceAll(s, `"type":"integer"`, `"type":"number"`) }
		for _, body := range []string{
			`{"tools":[` + namespace.Raw + `]}`,
			fmt.Sprintf(`{"tools":[{"name":%q,"parameters":%s}]}`, name, tool.Get("parameters").Raw),
			`{"input":[{"type":"additional_tools","tools":[` + namespace.Raw + `]}]}`,
		} {
			integer("history/"+name, "codex", degrade(body))
		}
		for _, unknown := range []string{tool.Get("name").String(), "mcp__server__" + name, "user__" + name} {
			integer("history_unknown/"+name, "codex", fmt.Sprintf(`{"tools":[{"name":%q,"parameters":%s}]}`, unknown, degrade(tool.Get("parameters").Raw)))
		}
	}

	data, err := json.MarshalIndent(map[string]any{"schemas": schemas, "integers": integers}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.MkdirAll(out, 0o755); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(filepath.Join(out, "codex_tool_schema_go.json"), append(data, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}
}

// setRSFixPath places value at a properties path such as "barrier.properties.timeout_ms",
// creating the objects on the way ("x.anyOf.0" builds a nullable union).
func setRSFixPath(node map[string]any, path []string, value json.RawMessage) {
	if len(path) == 1 {
		node[path[0]] = value
		return
	}
	if len(path) == 3 && path[1] == "anyOf" && path[2] == "0" {
		node[path[0]] = map[string]any{"anyOf": []any{value, map[string]any{"type": "null"}}}
		return
	}
	child, _ := node[path[0]].(map[string]any)
	if child == nil {
		child = map[string]any{}
		node[path[0]] = child
	}
	setRSFixPath(child, path[1:], value)
}
