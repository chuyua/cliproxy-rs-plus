package executor

import (
	"crypto/sha256"
	"fmt"
	"strings"
	"testing"

	"github.com/tidwall/sjson"
)

// Extra inputs for the Rust port's replay fixture: edge cases Go's own tests do not
// reach. The recording hooks capture Go's results; nothing is asserted here.
func TestZYReplayExtra(t *testing.T) {
	hash := func(kind, text string) string {
		sum := sha256.Sum256([]byte(kind + "\x00" + text))
		return fmt.Sprintf("%x", sum[:])
	}
	signed := `{"request":{"systemInstruction":{"parts":[{"text":"sys"}]},"contents":[` +
		`{"role":"user","parts":[{"text":"q"}]},` +
		`{"role":"model","parts":[{"thought":true,"text":"why","thoughtSignature":"c2lnLXRob3VnaHQtZXh0cmEtMTIzNA=="},{"text":"say","thoughtSignature":"c2lnLXRleHQtZXh0cmEtMTIzNDU2"},{"functionCall":{"id":"x-1","name":"f","args":{"a":1}},"thoughtSignature":"c2lnLWNhbGwtZXh0cmEtMTIzNDU2"}]},` +
		`{"role":"user","parts":[{"functionResponse":{"id":"x-1","name":"f","response":{"ok":1}}}]}]}}`
	items := antigravityReasoningReplayItemsFromRequest([]byte(signed))
	unsigned := []byte(signed)
	for part := 0; part < 3; part++ {
		unsigned, _ = sjson.DeleteBytes(unsigned, fmt.Sprintf("request.contents.1.parts.%d.thoughtSignature", part))
	}
	// Roles hash lowercased, so a recased history keeps its context.
	recased := []byte(strings.Replace(strings.Replace(string(unsigned), `"role":"model"`, `"role":"MODEL"`, 1), `"role":"user"`, `"role":"User"`, 1))
	applyAntigravityReasoningReplayItems(recased, items, nil)
	applyAntigravityReasoningReplayItems(unsigned, items, nil)

	// A target hash is searched across the turn when the part index is stale, and a
	// target kind that disagrees with the part rejects the hash match.
	payload := []byte(`{"request":{"contents":[{"role":"user","parts":[{"text":"q"}]},{"role":"model","parts":[{"text":"intro"},{"thought":true,"text":"deep"},{"text":"deep"}]}]}}`)
	stale := fmt.Sprintf(`{"type":"thought_signature","thoughtSignature":"c2lnLXN0YWxlLWluZGV4LTEyMzQ1Ng==","contentIndex":1,"partIndex":0,"targetKind":"text","targetHash":%q}`, hash("text", "deep"))
	wrongKind := fmt.Sprintf(`{"type":"thought_signature","thoughtSignature":"c2lnLXdyb25nLWtpbmQtMTIzNDU2Nzg=","contentIndex":1,"partIndex":1,"targetKind":"text","targetHash":%q}`, hash("thought", "deep"))
	applyAntigravityReasoningReplayItems(payload, [][]byte{[]byte(stale)}, nil)
	applyAntigravityReasoningReplayItems(payload, [][]byte{[]byte(wrongKind)}, nil)

	// A repeated stream chunk records its call once; an unsigned repeat at a new
	// part index records again.
	scope := antigravityReasoningReplayScope{modelName: "gemini-3-flash", sessionKey: "extra"}
	chunk := []byte(`{"response":{"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"id":"r-1","name":"f","args":{"a":1}},"thoughtSignature":"c2lnLXJlcGVhdC1jaHVuay0xMjM0NTY="}]}}]}}`)
	unsignedChunk := []byte(`{"response":{"candidates":[{"content":{"role":"model","parts":[{"functionCall":{"name":"g","args":{}}}]}}]}}`)
	acc := newAntigravityReasoningReplayAccumulator(scope, []byte(`{"request":{"contents":[{"role":"user","parts":[{"text":"go"}]}]}}`))
	acc.observeResponsePayload(chunk)
	acc.observeResponsePayload(chunk)
	acc.observeResponsePayload(unsignedChunk)
	acc.observeResponsePayload(unsignedChunk)
	acc.observeResponsePayload([]byte(`{"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"end"}]},"finishReason":"STOP"}]}}`))
	recasedHistory := []byte(`{"request":{"contents":[{"role":"USER","parts":[{"text":"go"}]},{"role":"Model","parts":[{"text":"so far"}]}]}}`)
	acc = newAntigravityReasoningReplayAccumulator(scope, recasedHistory)
	acc.observeResponsePayload([]byte(`{"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"more","thoughtSignature":"c2lnLXJlY2FzZWQtaGlzdG9yeS0xMjM="}]},"finishReason":"STOP"}]}}`))
}
