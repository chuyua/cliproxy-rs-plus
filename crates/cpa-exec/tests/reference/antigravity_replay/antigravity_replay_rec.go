package executor

import (
	"context"
	"runtime"
	"strings"

	"github.com/router-for-me/CLIProxyAPI/v8/internal/runtime/executor/helps"
	cliproxyexecutor "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/executor"
)

// Recording hooks for the Rust port's replay fixture (not part of CLIProxyAPI).

var (
	recLedger       [][]byte
	recInvalidated  bool
	recPrepares     []recPrepare
	recApplies      []recApply
	recScopes       []recScope
	recAccumulators []*recAccumulator
)

// currentTest is the Go test function on the stack.
func currentTest() string {
	pcs := make([]uintptr, 64)
	frames := runtime.CallersFrames(pcs[:runtime.Callers(2, pcs)])
	for {
		frame, more := frames.Next()
		if i := strings.Index(frame.Function, ".Test"); i >= 0 {
			name := frame.Function[i+1:]
			if j := strings.Index(name, "."); j >= 0 {
				name = name[:j]
			}
			return name
		}
		if !more {
			return ""
		}
	}
}

type recRequest struct {
	Model     string              `json:"model"`
	Source    string              `json:"source"`
	Body      string              `json:"body"`
	Original  string              `json:"original"`
	Headers   map[string][]string `json:"headers"`
	Execution string              `json:"execution"`
	Derived   string              `json:"derived"`
	Payload   string              `json:"payload"`
}

type recPrepare struct {
	Test        string     `json:"test"`
	Request     recRequest `json:"request"`
	Session     string     `json:"session"`
	Ledger      []string   `json:"ledger"`
	Out         string     `json:"out"`
	Error       string     `json:"error"`
	Invalidated bool       `json:"invalidated"`
}

type recApply struct {
	Test    string         `json:"test"`
	Payload string         `json:"payload"`
	Items   []string       `json:"items"`
	Schemas map[string]any `json:"schemas"`
	Out     string         `json:"out"`
	Changed bool           `json:"changed"`
}

type recScope struct {
	Test    string     `json:"test"`
	Request recRequest `json:"request"`
	Session string     `json:"session"`
}

type recAccumulator struct {
	Test     string   `json:"test"`
	Request  string   `json:"request"`
	Observed []string `json:"observed"`
	Items    []string `json:"items"`
	Overflow bool     `json:"overflow"`
	Terminal bool     `json:"terminal"`
	acc      *antigravityReasoningReplayAccumulator
}

func strs(items [][]byte) []string {
	out := []string{}
	for _, item := range items {
		out = append(out, string(item))
	}
	return out
}

func recRequestOf(modelName string, req cliproxyexecutor.Request, opts cliproxyexecutor.Options, payload []byte) recRequest {
	execution := metadataString(opts.Metadata, cliproxyexecutor.ExecutionSessionMetadataKey)
	if execution == "" {
		execution = metadataString(req.Metadata, cliproxyexecutor.ExecutionSessionMetadataKey)
	}
	headers := map[string][]string{}
	for k, v := range opts.Headers {
		headers[k] = v
	}
	return recRequest{
		Model: modelName, Source: opts.SourceFormat.String(), Body: string(req.Payload), Original: string(opts.OriginalRequest),
		Headers: headers, Execution: execution, Derived: helps.DerivedSessionID(opts.Metadata, req.Metadata), Payload: string(payload),
	}
}

func prepareAntigravityGeminiReasoningReplayPayload(ctx context.Context, modelName string, req cliproxyexecutor.Request, opts cliproxyexecutor.Options, payload []byte) ([]byte, antigravityReasoningReplayScope, error) {
	recLedger, recInvalidated = nil, false
	in := recRequestOf(modelName, req, opts, payload)
	out, scope, err := prepareAntigravityGeminiReasoningReplayPayloadInner(ctx, modelName, req, opts, payload)
	rec := recPrepare{Test: currentTest(), Request: in, Session: scope.sessionKey, Ledger: strs(recLedger), Out: string(out), Invalidated: recInvalidated}
	if err != nil {
		rec.Error = err.Error()
	}
	recPrepares = append(recPrepares, rec)
	return out, scope, err
}

func applyAntigravityReasoningReplayItems(payload []byte, items [][]byte, toolSchemas map[string]any) ([]byte, bool) {
	in := string(payload)
	out, changed := applyAntigravityReasoningReplayItemsInner(payload, items, toolSchemas)
	recApplies = append(recApplies, recApply{Test: currentTest(), Payload: in, Items: strs(items), Schemas: toolSchemas, Out: string(out), Changed: changed})
	return out, changed
}

func antigravityReasoningReplayScopeFromRequest(ctx context.Context, modelName string, req cliproxyexecutor.Request, opts cliproxyexecutor.Options, payload []byte) antigravityReasoningReplayScope {
	scope := antigravityReasoningReplayScopeFromRequestInner(ctx, modelName, req, opts, payload)
	recScopes = append(recScopes, recScope{Test: currentTest(), Request: recRequestOf(modelName, req, opts, payload), Session: scope.sessionKey})
	return scope
}
