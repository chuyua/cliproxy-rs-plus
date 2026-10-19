package executor

import (
	"encoding/json"
	"os"
	"regexp"
	"strings"
	"testing"
)

// TestZZHelpersDump writes the recorded helper calls. A record keeps its Go test name
// only when that test is defined in one of the suites named in HELPER_SUITES, so the
// fixture cites exactly the tests whose helper calls it replays.
func TestZZHelpersDump(t *testing.T) {
	cited := map[string]bool{}
	for _, file := range strings.Split(os.Getenv("HELPER_SUITES"), ",") {
		src, err := os.ReadFile(file)
		if err != nil {
			t.Fatal(err)
		}
		for _, m := range regexp.MustCompile(`(?m)^func (Test\w+)\(`).FindAllStringSubmatch(string(src), -1) {
			cited[m[1]] = true
		}
	}
	seen := map[string]bool{}
	out := []helperRecord{}
	for _, r := range helperRecords {
		if !cited[r.Test] {
			r.Test = ""
		}
		if len(r.In)+len(r.Out)+len(r.Original) > 1<<15 {
			continue
		}
		key, _ := json.Marshal(r)
		if seen[string(key)] {
			continue
		}
		seen[string(key)] = true
		out = append(out, r)
	}
	data, err := json.Marshal(out)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(os.Getenv("HELPER_DUMP"), append(data, '\n'), 0o644); err != nil {
		t.Fatal(err)
	}
}
