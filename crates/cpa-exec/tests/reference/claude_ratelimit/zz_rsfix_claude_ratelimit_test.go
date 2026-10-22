package executor

// Recorder for cliproxy-rs (see README.md). Copied into a scratch copy of CLIProxyAPI
// v8.0.35 under internal/runtime/executor, it runs Go's Claude 429 helpers on fixed
// inputs and writes their results to $RSFIX_OUT.

import (
	"encoding/json"
	"net/http"
	"os"
	"strconv"
	"strings"
	"testing"
	"time"

	"github.com/router-for-me/CLIProxyAPI/v8/internal/runtime/executor/helps"
)

const rsfixPerMinute = `{"type":"error","error":{"type":"rate_limit_error","message":"This request would exceed the requests per minute rate limit. Please try again later."}}`

type rsfixCase struct {
	Name    string      `json:"name"`
	Headers [][2]string `json:"headers"`
	Body    string      `json:"body"`
}

type rsfixRecord struct {
	rsfixCase
	// Cooldown at the fixed now without Go's 1-30 s fuzz, in milliseconds.
	ResetMs      *int64 `json:"reset_ms"`
	HeaderOnlyMs *int64 `json:"header_only_ms"`
	Shared       bool   `json:"shared"`
	// classifyClaudeUpstreamErrorWithCooling(429, ..., false) and
	// newClaudeFastDirectResponseError for a 429, with time.Now() as now.
	Scope         string `json:"scope"`
	ClassifyRetry bool   `json:"classify_retry"`
	FastScope     string `json:"fast_scope"`
	FastRetry     bool   `json:"fast_retry"`
}

// {now+N} is the Unix second N seconds after now; {date+N} is that instant as an
// HTTP date.
func rsfixExpand(raw string, now time.Time) string {
	for _, kind := range []string{"now", "date"} {
		for {
			start := strings.Index(raw, "{"+kind+"+")
			if start < 0 {
				break
			}
			end := strings.Index(raw[start:], "}") + start
			offset, err := strconv.ParseInt(raw[start+len(kind)+2:end], 10, 64)
			if err != nil {
				panic(err)
			}
			at := now.Add(time.Duration(offset) * time.Second)
			value := strconv.FormatInt(at.Unix(), 10)
			if kind == "date" {
				value = at.UTC().Format(http.TimeFormat)
			}
			raw = raw[:start] + value + raw[end+1:]
		}
	}
	return raw
}

func rsfixHeaders(c rsfixCase, now time.Time) http.Header {
	h := http.Header{}
	for _, kv := range c.Headers {
		h.Add(kv[0], rsfixExpand(kv[1], now))
	}
	return h
}

// The fuzz is a whole number of seconds from 1 to 30; the minimum over many calls
// less one second is the base cooldown.
func rsfixDefuzz(call func() *time.Duration) *int64 {
	var best *time.Duration
	for i := 0; i < 600; i++ {
		got := call()
		if got == nil {
			return nil
		}
		if best == nil || *got < *best {
			best = got
		}
	}
	ms := (*best - time.Second).Milliseconds()
	return &ms
}

func rsfixScope(err error) (string, bool) {
	scope := "status"
	if s, ok := err.(interface{ IsRequestScoped() bool }); ok && s.IsRequestScoped() {
		scope = "request"
	} else if s, ok := err.(interface{ IsCredentialScoped() bool }); ok {
		scope = "model"
		if s.IsCredentialScoped() {
			scope = "credential"
		}
	}
	retry := false
	if r, ok := err.(interface{ RetryAfter() *time.Duration }); ok {
		retry = r.RetryAfter() != nil
	}
	return scope, retry
}

func TestRSFixClaudeRateLimit(t *testing.T) {
	out := os.Getenv("RSFIX_OUT")
	if out == "" {
		t.Skip("RSFIX_OUT not set")
	}
	now := time.Date(2026, 10, 17, 14, 0, 0, 0, time.UTC)
	perMinute := func(message string) string {
		return `{"type":"error","error":{"type":"rate_limit_error","message":` + strconv.Quote(message) + `}}`
	}
	short := [][2]string{{"Retry-After", "30"}, {"Anthropic-Ratelimit-Unified-Reset", "{now+18000}"}}
	with := func(extra ...[2]string) [][2]string {
		return append(append([][2]string{}, short...), extra...)
	}
	cases := []rsfixCase{
		{"requests per minute: short retry wins over 5h unified reset", short, rsfixPerMinute},
		{"input tokens per minute", short, perMinute("This request would exceed the input tokens per minute rate limit. Please try again later.")},
		{"output tokens per minute", short, perMinute("This request would exceed the output tokens per minute rate limit. Please try again later.")},
		{"case and whitespace", short, perMinute("  THIS REQUEST WOULD EXCEED THE REQUESTS PER MINUTE RATE LIMIT. PLEASE TRY AGAIN LATER.\n\t")},
		{"http date retry", [][2]string{{"Retry-After", "{date+45}"}, {"Anthropic-Ratelimit-Unified-Reset", "{now+18000}"}}, rsfixPerMinute},
		{"fractional retry", [][2]string{{"Retry-After", "12.5"}, {"Anthropic-Ratelimit-Unified-Reset", "{now+18000}"}}, rsfixPerMinute},
		{"maximum usable retry", [][2]string{{"Retry-After", "608400"}, {"Anthropic-Ratelimit-Unified-Reset", "{now+18000}"}}, rsfixPerMinute},
		{"retry beyond the window", [][2]string{{"Retry-After", "608401"}, {"Anthropic-Ratelimit-Unified-Reset", "{now+18000}"}}, rsfixPerMinute},
		{"zero retry", [][2]string{{"Retry-After", "0"}, {"Anthropic-Ratelimit-Unified-Reset", "{now+18000}"}}, rsfixPerMinute},
		{"invalid retry", [][2]string{{"Retry-After", "soon"}, {"Anthropic-Ratelimit-Unified-Reset", "{now+18000}"}}, rsfixPerMinute},
		{"repeated retry", with([2]string{"Retry-After", "30"}), rsfixPerMinute},
		{"no retry", [][2]string{{"Anthropic-Ratelimit-Unified-Reset", "{now+18000}"}}, rsfixPerMinute},
		{"empty unified status does not block", with([2]string{"Anthropic-Ratelimit-Unified-Status", " "}), rsfixPerMinute},
		{"empty overage status does not block", with([2]string{"Anthropic-Ratelimit-Unified-Overage-Status", ""}), rsfixPerMinute},
		{"allowed unified status blocks", with([2]string{"Anthropic-Ratelimit-Unified-Status", "allowed"}), rsfixPerMinute},
		{"overage status blocks", with([2]string{"Anthropic-Ratelimit-Unified-Overage-Status", "allowed"}), rsfixPerMinute},
		{"7d_oi status blocks", with([2]string{"Anthropic-Ratelimit-Unified-7d_oi-Status", "allowed"}), rsfixPerMinute},
		{"5h rejection keeps the window", with([2]string{"Anthropic-Ratelimit-Unified-5h-Status", "rejected"}, [2]string{"Anthropic-Ratelimit-Unified-5h-Reset", "{now+7200}"}), rsfixPerMinute},
		{"5h reset without status is ignored", with([2]string{"Anthropic-Ratelimit-Unified-5h-Reset", "{now+7200}"}), rsfixPerMinute},
		{"wrong error type", short, `{"type":"error","error":{"type":"api_error","message":"This request would exceed the requests per minute rate limit. Please try again later."}}`},
		{"substring message", short, perMinute("This request would exceed the requests per minute rate limit. Please try again later. Your weekly quota is exhausted.")},
		{"negated message", short, perMinute("This request would not exceed the requests per minute rate limit. Please try again later.")},
		{"uppercase json keys", short, `{"ERROR":{"Type":"rate_limit_error","MESSAGE":"This request would exceed the requests per minute rate limit. Please try again later."}}`},
		// cliproxy-rs keeps only the last of two byte-identical keys (DIFFERENCES-FROM-GO.md).
		{"later null keeps the error", short, `{"error":{"type":"rate_limit_error","message":"This request would exceed the requests per minute rate limit. Please try again later."},"error":null}`},
		{"long s and kelvin fold", short, `{"error":{"type":"rate_limit_error","meſſage":"This request would exceed the requests per minute rate limit. Please try again later."}}`},
		{"duplicate error objects merge", short, `{"error":{"type":"rate_limit_error"},"Error":{"message":"This request would exceed the requests per minute rate limit. Please try again later."}}`},
		{"null message", short, `{"error":{"type":"rate_limit_error","message":null}}`},
		{"numeric message", short, `{"error":{"type":"rate_limit_error","message":7}}`},
		{"string error", short, `{"error":"rate_limit_error"}`},
		{"array body", short, `[` + rsfixPerMinute + `]`},
		{"trailing garbage", short, rsfixPerMinute + `x`},
		{"empty body", short, ``},
		{"ordinary 429 without per-minute body", short, `{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}`},
		{"overage-only rejection", with(
			[2]string{"Anthropic-Ratelimit-Unified-Status", "rejected"},
			[2]string{"Anthropic-Ratelimit-Unified-5h-Status", "allowed"},
			[2]string{"Anthropic-Ratelimit-Unified-7d-Status", "allowed"},
			[2]string{"Anthropic-Ratelimit-Unified-Overage-Status", "rejected"},
		), rsfixPerMinute},
		{"fast mode credits", [][2]string{{"Retry-After", "30"}}, `{"type":"error","error":{"type":"rate_limit_error","message":"Usage credits are required for fast mode"}}`},
		{"empty headers", [][2]string{}, rsfixPerMinute},
		{"repeated status, later value non-empty", with([2]string{"Anthropic-Ratelimit-Unified-Status", ""}, [2]string{"Anthropic-Ratelimit-Unified-Status", "rejected"}), rsfixPerMinute},
		{"repeated unified resets", with([2]string{"Anthropic-Ratelimit-Unified-Reset", "{now+7200}"}), rsfixPerMinute},
		{"repeated unified resets, ordinary body", with([2]string{"Anthropic-Ratelimit-Unified-Reset", "{now+7200}"}), `{"type":"error","error":{"type":"rate_limit_error","message":"slow down"}}`},
		{"7d rejection", with([2]string{"Anthropic-Ratelimit-Unified-7d-Status", "rejected"}, [2]string{"Anthropic-Ratelimit-Unified-7d-Reset", "{now+86400}"}), rsfixPerMinute},
		{"competing 5h and 7d rejections", with(
			[2]string{"Anthropic-Ratelimit-Unified-5h-Status", "rejected"},
			[2]string{"Anthropic-Ratelimit-Unified-5h-Reset", "{now+7200}"},
			[2]string{"Anthropic-Ratelimit-Unified-7d-Status", "rejected"},
			[2]string{"Anthropic-Ratelimit-Unified-7d-Reset", "{now+172800}"},
		), rsfixPerMinute},
		{"competing rejections, later 5h", with(
			[2]string{"Anthropic-Ratelimit-Unified-5h-Status", "rejected"},
			[2]string{"Anthropic-Ratelimit-Unified-5h-Reset", "{now+90000}"},
			[2]string{"Anthropic-Ratelimit-Unified-7d-Status", "rejected"},
			[2]string{"Anthropic-Ratelimit-Unified-7d-Reset", "{now+86400}"},
		), rsfixPerMinute},
		{"lowercase header names", [][2]string{{"retry-after", "30"}, {"anthropic-ratelimit-unified-reset", "{now+18000}"}}, rsfixPerMinute},
	}
	records := make([]rsfixRecord, 0, len(cases))
	for _, c := range cases {
		h := rsfixHeaders(c, now)
		before := h.Clone()
		rec := rsfixRecord{rsfixCase: c}
		rec.ResetMs = rsfixDefuzz(func() *time.Duration { return helps.ParseClaudeRateLimitResetForError(h, []byte(c.Body), now) })
		rec.HeaderOnlyMs = rsfixDefuzz(func() *time.Duration { return helps.ParseClaudeRateLimitReset(h, now) })
		rec.Shared = helps.ClaudeHeadersIndicateUnifiedRateLimitRejection(h)
		live := rsfixHeaders(c, time.Now())
		rec.Scope, rec.ClassifyRetry = rsfixScope(classifyClaudeUpstreamErrorWithCooling(http.StatusTooManyRequests, live, []byte(c.Body), false))
		rec.FastScope, rec.FastRetry = rsfixScope(newClaudeFastDirectResponseError(&http.Response{StatusCode: http.StatusTooManyRequests, Header: live}, []byte(c.Body)))
		if len(h) != len(before) {
			t.Fatalf("%s: headers mutated", c.Name)
		}
		records = append(records, rec)
	}
	data, err := json.MarshalIndent(map[string]any{
		"reference": "CLIProxyAPI v8.0.35 (ae3ee3be)",
		"now_unix":  now.Unix(),
		"cases":     records,
	}, "", "  ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(out, append(data, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}
}
