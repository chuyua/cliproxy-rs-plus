package auth

// Records CLIProxyAPI v8.0.35 (ae3ee3be) CodexSIWCNeedsRefresh for Sign in with ChatGPT
// partitions whose expiry exercises parseTimeValue, normaliseUnix, expirationFromMap,
// parseJWTExp and Go's zero time, at two fixed clocks. See README.md.

import (
	"encoding/base64"
	"encoding/json"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"
)

func TestRSFixSIWCExpiry(t *testing.T) {
	header := base64.RawURLEncoding.EncodeToString([]byte(`{"alg":"none"}`))
	jwt := func(payload string) string {
		return header + "." + base64.RawURLEncoding.EncodeToString([]byte(payload)) + ".sig"
	}
	// The access token as a JSON string member.
	token := func(t string) string {
		b, _ := json.Marshal(t)
		return `{"access_token":` + string(b) + `}`
	}
	const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
	exp := `{"exp": 1792666800}` // 19 bytes: the last character carries 4 unused bits
	raw := base64.RawURLEncoding.EncodeToString([]byte(exp))
	trailing := raw[:len(raw)-1] + string(alphabet[strings.IndexByte(alphabet, raw[len(raw)-1])|1])
	padded := base64.URLEncoding.EncodeToString([]byte(exp)) // ends in "=="
	// Each extra is merged over a partition that can refresh and has an opaque token.
	cases := []struct{ Name, Extra string }{
		{"relative i64::MAX seconds wraps", `{"expires_in":"9223372036854775807","timestamp":"2026-10-22T10:00:00Z"}`},
		{"relative 1e10 seconds wraps", `{"expires_in":10000000000,"timestamp":"2026-10-22T10:00:00Z"}`},
		{"relative after a Unix issue time", `{"expiresIn":"3600","issued_at":1792663200}`},
		{"relative float seconds", `{"expires_in":3600.9,"timestamp":1792663200}`},
		{"nested token map", `{"Token":{"expires_in":600,"issuedAt":1792663200000}}`},
		{"no issue time", `{"expires_in":3600}`},
		{"zero Unix expiry ends the search", `{"expired":0,"expires_in":3600,"timestamp":1792663200}`},
		{"lower-case z", `{"expired":"2026-10-22T11:00:00z"}`},
		{"lower-case t", `{"expired":"2026-10-22t11:00:00Z"}`},
		{"naive fraction", `{"expired":"2026-10-22 11:00:00.123"}`},
		{"comma fraction", `{"expired":"2026-10-22T11:00:00,123Z"}`},
		{"long fraction", `{"expired":"2026-10-22T11:00:00.1234567891234Z"}`},
		{"leap second", `{"expired":"2026-10-22T11:00:60Z"}`},
		{"one-digit hour", `{"expired":"2026-10-23T9:00:00Z"}`},
		{"one-digit naive hour", `{"expired":"2026-10-23 9:30"}`},
		{"one-digit month", `{"expired":"2026-1-23 11:00:00"}`},
		{"run of spaces", `{"expired":"2026-10-23   11:00"}`},
		{"24-hour offset", `{"expired":"2026-10-23T11:00:00+24:00"}`},
		{"surrounding space", `{"expired":" 2026-10-22T11:00:00+00:30 "}`},
		{"February 30", `{"expired":"2026-02-30 11:00"}`},
		{"year zero", `{"expired":"0000-03-01T00:00:00Z","last_refresh":"2026-10-22T09:58:00Z"}`},
		{"zero instant falls back", `{"expired":"0001-01-01T00:00:00Z","last_refresh":"2026-10-22T09:58:00Z"}`},
		{"zero instant with offset", `{"expired":"0001-01-01T01:00:00+01:00","last_refresh":"2026-10-22T09:58:00Z"}`},
		{"zero issue time tries the next", `{"expires_in":3600,"timestamp":"0001-01-01T00:00:00Z","issued_at":1792663200}`},
		{"far-future milliseconds", `{"expired":"9223372036854775807"}`},
		{"out-of-range float", `{"expired":1e300}`},
		{"last refresh", `{"last_refresh":"2026-10-22T09:58:00Z"}`},
		{"jwt exp", `{"access_token":"` + jwt(`{"exp":1792666800}`) + `"}`},
		{"jwt far-future string exp", `{"access_token":"` + jwt(`{"exp":"9223372036854775807"}`) + `"}`},
		{"float at the unit threshold", `{"expired":1000000000000.9999}`},
		{"jwt case variant replaces exp", token(jwt(`{"exp":1792666800,"Exp":1}`))},
		{"jwt later null clears exp", token(jwt(`{"exp":1792666800,"EXP":null}`))},
		{"jwt skips an out-of-range member", token(jwt(`{"exp":1792666800,"junk":1e400}`))},
		{"jwt out-of-range exp fails", token(jwt(`{"Exp":1e400,"exp":1792666800}`))},
		{"jwt non-object payload", token(jwt(`[1792666800]`))},
		{"jwt CR and LF in the payload", token(header + "." + raw[:10] + "\r\n" + raw[10:] + ".sig")},
		{"jwt non-zero trailing bits", token(header + "." + trailing + ".sig")},
		{"jwt padded payload", token(header + "." + padded + ".sig")},
		{"jwt short padding with LF", token(header + "." + padded[:len(padded)-1] + "\n.sig")},
	}
	clocks := []time.Time{time.Date(2026, 10, 22, 10, 0, 0, 0, time.UTC), time.Date(2026, 10, 22, 10, 56, 0, 0, time.UTC)}
	type record struct {
		Name string `json:"name"`
		SIWC string `json:"siwc"`
		Due  []bool `json:"due"`
	}
	var out struct {
		Clocks []string `json:"clocks"`
		Cases  []record `json:"cases"`
	}
	for _, at := range clocks {
		out.Clocks = append(out.Clocks, at.Format(time.RFC3339))
	}
	for _, c := range cases {
		// Decoded as the file store decodes a credential file: numbers become float64.
		siwc := map[string]any{"access_token": "opaque", "refresh_token": "rt", "client_id": "c"}
		if err := json.Unmarshal([]byte(c.Extra), &siwc); err != nil {
			t.Fatal(c.Name, err)
		}
		raw, _ := json.Marshal(siwc)
		var decoded map[string]any
		_ = json.Unmarshal(raw, &decoded)
		r := record{Name: c.Name, SIWC: string(raw)}
		for _, at := range clocks {
			_, has, due := CodexSIWCNeedsRefresh(&Auth{Metadata: map[string]any{"siwc": decoded}}, at)
			if !has {
				t.Fatal(c.Name, "cannot refresh")
			}
			r.Due = append(r.Due, due)
		}
		out.Cases = append(out.Cases, r)
	}
	data, _ := json.MarshalIndent(out, "", "  ")
	dir := os.Getenv("RSFIX_OUT")
	_ = os.MkdirAll(dir, 0o755)
	if err := os.WriteFile(filepath.Join(dir, "codex_siwc_expiry_go.json"), append(data, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}
}
