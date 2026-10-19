package executor

import (
	"encoding/json"
	"os"
	"testing"
)

func TestZZReplayDump(t *testing.T) {
	for _, rec := range recAccumulators {
		if !rec.acc.overflow {
			rec.acc.appendPendingThoughtSignatures()
		}
		rec.Items = strs(rec.acc.items)
		rec.Overflow = rec.acc.overflow
		rec.Terminal = rec.acc.terminal
	}
	limit := func(n int) bool { return n <= 1<<16 }
	prepares := []recPrepare{}
	for _, r := range recPrepares {
		if limit(len(r.Request.Payload) + len(r.Out)) {
			prepares = append(prepares, r)
		}
	}
	applies := []recApply{}
	for _, r := range recApplies {
		if limit(len(r.Payload) + len(r.Out)) {
			applies = append(applies, r)
		}
	}
	accs := []*recAccumulator{}
	for _, r := range recAccumulators {
		size := len(r.Request)
		for _, o := range r.Observed {
			size += len(o)
		}
		if limit(size) {
			accs = append(accs, r)
		}
	}
	data, err := json.MarshalIndent(map[string]any{"prepare": prepares, "apply": applies, "scope": recScopes, "accumulator": accs}, "", " ")
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(os.Getenv("REPLAY_DUMP"), append(data, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}
}
