package main

import (
	"bytes"
	"context"
	"encoding/json"
	"io"
	"net/http"
	"sort"
	"strings"
	"time"

	"github.com/router-for-me/CLIProxyAPI/v8/internal/config"
	"github.com/router-for-me/CLIProxyAPI/v8/internal/runtime/executor"
	sdkauth "github.com/router-for-me/CLIProxyAPI/v8/sdk/auth"
	cliproxyauth "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/auth"
)

// reply is one scripted answer to the next request.
type reply struct {
	Status int    `json:"status"`
	Body   string `json:"body"`
	// Location is the redirect target of a 3xx reply.
	Location string `json:"location,omitempty"`
}

// recorded is a request as Go's client handed it to the transport: the header map
// before the transport adds its own fields.
type recorded struct {
	Method  string      `json:"method"`
	URL     string      `json:"url"`
	Headers [][2]string `json:"headers"`
	Body    string      `json:"body"`
}

// scripted answers requests in order and records them; nothing leaves the process.
type scripted struct {
	replies  []reply
	requests []recorded
}

func (s *scripted) RoundTrip(req *http.Request) (*http.Response, error) {
	var body []byte
	if req.Body != nil {
		body, _ = io.ReadAll(req.Body)
	}
	var headers [][2]string
	for name, values := range req.Header {
		for _, v := range values {
			headers = append(headers, [2]string{name, v})
		}
	}
	sort.Slice(headers, func(i, j int) bool { return headers[i][0] < headers[j][0] })
	s.requests = append(s.requests, recorded{Method: req.Method, URL: req.URL.String(), Headers: headers, Body: string(body)})
	if len(s.replies) == 0 {
		return &http.Response{StatusCode: 599, Body: io.NopCloser(strings.NewReader("unscripted")), Header: http.Header{}, Request: req}, nil
	}
	r := s.replies[0]
	s.replies = s.replies[1:]
	header := http.Header{"Content-Type": {"application/json"}}
	if r.Location != "" {
		header.Set("Location", r.Location)
	}
	return &http.Response{
		StatusCode: r.Status,
		Header:     header,
		Body:       io.NopCloser(bytes.NewReader([]byte(r.Body))),
		Request:    req,
	}, nil
}

type authCase struct {
	Name string `json:"name"`
	// Op is complete (CompleteAntigravityOAuth), prepare (PrepareRequestAuth) or refresh.
	Op       string         `json:"op"`
	Metadata map[string]any `json:"metadata,omitempty"`
	Replies  []reply        `json:"replies"`

	Requests []recorded `json:"requests"`
	// FileName and Result describe a completed login or the prepared metadata, without
	// the clock-dependent timestamp and expired values.
	FileName   string         `json:"file_name,omitempty"`
	Result     map[string]any `json:"result,omitempty"`
	Unchanged  bool           `json:"unchanged,omitempty"`
	Error      string         `json:"error,omitempty"`
	Status     int            `json:"status,omitempty"`
	RetryAfter float64        `json:"retry_after,omitempty"`
}

func ok(body string) reply { return reply{Status: 200, Body: body} }

const tokenOK = `{"access_token":"ya29.fake-new","refresh_token":"1//fake-refresh-new","expires_in":3599,"token_type":"Bearer","scope":"x"}`
const userOK = `{"email":"user@example.com","verified_email":true}`

func authCases() []authCase {
	// FAR and PAST are expiry placeholders resolved when a case runs (a day ahead, an
	// hour ago), so the recorded cases stay valid on any later day.
	far, past := "FAR", "PAST"
	creds := func(extra map[string]any) map[string]any {
		m := map[string]any{"type": "antigravity", "access_token": "ya29.fake-old", "refresh_token": "1//fake-refresh", "expired": far, "email": "user@example.com"}
		for k, v := range extra {
			if v == nil {
				delete(m, k)
			} else {
				m[k] = v
			}
		}
		return m
	}
	long := strings.Repeat("x", 300)
	return []authCase{
		{Name: "complete_basic", Op: "complete", Replies: []reply{ok(tokenOK), ok(userOK), ok(`{"cloudaicompanionProject":"proj-1","currentTier":{"id":"standard-tier"}}`)}},
		{Name: "complete_project_object", Op: "complete", Replies: []reply{ok(tokenOK), ok(userOK), ok(`{"cloudaicompanionProject":{"id":" proj-2 "}}`)}},
		{Name: "complete_project_id_key", Op: "complete", Replies: []reply{ok(tokenOK), ok(userOK), ok(`{"cloudaicompanionProject":"  ","projectId":"proj-pid"}`)}},
		{Name: "complete_onboard_default_tier", Op: "complete", Replies: []reply{ok(tokenOK), ok(userOK),
			ok(`{"allowedTiers":[{"id":"legacy-tier"},{"id":"  ","isDefault":true},{"id":"standard-tier","isDefault":true}]}`),
			ok(`{"done":false}`), ok(`{"done":true,"response":{"cloudaicompanionProject":{"id":"proj-3"}}}`)}},
		{Name: "complete_onboard_current_tier", Op: "complete", Replies: []reply{ok(tokenOK), ok(userOK), ok(`{"currentTier":{"id":"g1-pro-tier"}}`), ok(`{"done":true,"response":{"project":"proj-4"}}`)}},
		{Name: "complete_onboard_null_load", Op: "complete", Replies: []reply{ok(tokenOK), ok(userOK), ok(`null`), ok(`{"done":true,"response":{"projectId":"proj-5"}}`)}},
		{Name: "complete_onboard_error", Op: "complete", Replies: []reply{ok(tokenOK), ok(userOK), ok(`{}`), {Status: 400, Body: " " + long + " "}}},
		{Name: "complete_onboard_without_project", Op: "complete", Replies: []reply{ok(tokenOK), ok(userOK), ok(`{}`), ok(`{"done":true,"response":{}}`)}},
		{Name: "complete_onboard_never_done", Op: "complete", Replies: []reply{ok(tokenOK), ok(userOK), ok(`{}`), ok(`{}`), ok(`{"done":"yes"}`), ok(`{}`), ok(`{}`), ok(`{}`)}},
		{Name: "complete_token_rejected", Op: "complete", Replies: []reply{{Status: 400, Body: `{"error":"invalid_grant"}`}}},
		{Name: "complete_token_rejected_empty", Op: "complete", Replies: []reply{{Status: 401, Body: "  "}}},
		{Name: "complete_token_trailing_value", Op: "complete", Replies: []reply{ok(tokenOK + ` {"x":1}`), ok(userOK), ok(`{"project":"proj-6"}`)}},
		{Name: "complete_token_bad_expires", Op: "complete", Replies: []reply{ok(`{"access_token":"a","expires_in":"soon"}`)}},
		{Name: "complete_empty_access_token", Op: "complete", Replies: []reply{ok(`{"refresh_token":"r"}`)}},
		{Name: "complete_userinfo_rejected", Op: "complete", Replies: []reply{ok(tokenOK), {Status: 401, Body: `{"error":{"code":401}}`}}},
		{Name: "complete_userinfo_no_email", Op: "complete", Replies: []reply{ok(tokenOK), ok(`{"email":"  "}`)}},
		{Name: "complete_load_failed", Op: "complete", Replies: []reply{ok(tokenOK), ok(userOK), {Status: 500, Body: " backend error \n"}}},
		{Name: "complete_load_invalid_json", Op: "complete", Replies: []reply{ok(tokenOK), ok(userOK), ok(`{"cloudaicompanionProject":`)}},
		{Name: "complete_load_array", Op: "complete", Replies: []reply{ok(tokenOK), ok(userOK), ok(`["proj"]`)}},

		{Name: "prepare_fresh_token", Op: "prepare", Metadata: creds(nil), Replies: []reply{ok(`{"cloudaicompanionProject":"proj-p1"}`)}},
		{Name: "prepare_expired_token", Op: "prepare", Metadata: creds(map[string]any{"expired": past}), Replies: []reply{ok(tokenOK), ok(`{"cloudaicompanionProject":"proj-p2"}`)}},
		{Name: "prepare_refresh_keeps_old_refresh_token", Op: "prepare", Metadata: creds(map[string]any{"expired": nil}), Replies: []reply{ok(`{"access_token":"ya29.only-access","expires_in":60}`), ok(`{"project":"proj-p3"}`)}},
		// refreshToken's own lookup fails (logged), then PrepareRequestAuth looks again.
		{Name: "prepare_expired_token_project_denied", Op: "prepare", Metadata: creds(map[string]any{"expired": past}), Replies: []reply{ok(tokenOK), {Status: 403, Body: "first"}, {Status: 404, Body: "second"}}},
		{Name: "prepare_expired_token_second_lookup_ok", Op: "prepare", Metadata: creds(map[string]any{"expired": past}), Replies: []reply{ok(tokenOK), {Status: 503, Body: "busy"}, ok(`{"project":"proj-late"}`)}},
		{Name: "prepare_without_refresh_token", Op: "prepare", Metadata: creds(map[string]any{"expired": past, "refresh_token": nil})},
		{Name: "prepare_project_denied", Op: "prepare", Metadata: creds(nil), Replies: []reply{{Status: 403, Body: `{"error":{"code":403,"message":"denied"}}`}}},
		{Name: "prepare_refresh_rate_limited", Op: "prepare", Metadata: creds(map[string]any{"expired": past}), Replies: []reply{{Status: 429, Body: `{"error":{"code":429,"message":"slow down","details":[{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"7.5s"}]}}`}}},
		{Name: "prepare_refresh_rate_limited_message", Op: "prepare", Metadata: creds(map[string]any{"expired": past}), Replies: []reply{{Status: 429, Body: `{"error":{"message":"Please retry after 12s."}}`}}},
		{Name: "prepare_has_project", Op: "prepare", Metadata: creds(map[string]any{"project_id": " proj-x "})},
		{Name: "refresh_basic", Op: "refresh", Metadata: creds(map[string]any{"project_id": "proj-x"}), Replies: []reply{ok(tokenOK)}},
		{Name: "refresh_rejected", Op: "refresh", Metadata: creds(map[string]any{"project_id": "proj-x"}), Replies: []reply{{Status: 400, Body: `{"error":"invalid_grant","error_description":"Token has been expired or revoked."}`}}},
		{Name: "refresh_invalid_json", Op: "refresh", Metadata: creds(map[string]any{"project_id": "proj-x"}), Replies: []reply{ok(`{"access_token":5}`)}},
	}
}

// stable drops the clock-dependent fields a login or refresh writes.
func stable(m map[string]any) map[string]any {
	out := map[string]any{}
	for k, v := range m {
		if k == "timestamp" || k == "expired" {
			out[k] = "<clock>"
			continue
		}
		out[k] = v
	}
	return out
}

func runAuth(c *authCase) {
	rt := &scripted{replies: append([]reply(nil), c.Replies...)}
	ctx := context.WithValue(context.Background(), "cliproxy.roundtripper", http.RoundTripper(rt))
	statusOf := func(err error) {
		c.Error = err.Error()
		if sc, ok := err.(interface{ StatusCode() int }); ok {
			c.Status = sc.StatusCode()
		}
		if ra, ok := err.(interface{ RetryAfter() *time.Duration }); ok && ra.RetryAfter() != nil {
			c.RetryAfter = ra.RetryAfter().Seconds()
		}
	}
	switch c.Op {
	case "complete":
		record, err := sdkauth.CompleteAntigravityOAuth(ctx, "4/fake-code", "http://localhost:51121/oauth-callback", &http.Client{Transport: rt})
		if err != nil {
			statusOf(err)
		} else {
			c.FileName = record.FileName
			c.Result = stable(record.Metadata)
		}
	case "prepare", "refresh":
		raw, _ := json.Marshal(c.Metadata)
		var metadata map[string]any
		_ = json.Unmarshal(raw, &metadata)
		switch metadata["expired"] {
		case "FAR":
			metadata["expired"] = time.Now().Add(24 * time.Hour).Format(time.RFC3339)
		case "PAST":
			metadata["expired"] = time.Now().Add(-time.Hour).Format(time.RFC3339)
		}
		// No ID: the post-refresh credits warm-up stays off.
		auth := &cliproxyauth.Auth{Provider: "antigravity", Metadata: metadata, Attributes: map[string]string{}}
		exec := executor.NewAntigravityExecutor(&config.Config{})
		var updated *cliproxyauth.Auth
		var err error
		if c.Op == "prepare" {
			updated, err = exec.PrepareRequestAuth(ctx, auth)
		} else {
			updated, err = exec.Refresh(ctx, auth)
		}
		switch {
		case err != nil:
			statusOf(err)
		case updated == nil:
			c.Unchanged = true
		default:
			c.Result = stable(updated.Metadata)
		}
	}
	c.Requests = rt.requests
}
