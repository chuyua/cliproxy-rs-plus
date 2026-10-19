// Generates goldens for the Antigravity port from the pinned Go code. Nothing contacts
// Google: the version cache is never refreshed, so every value uses the fallback, and
// executor scenarios talk to a local capture server (or a scripted round tripper).
package main

import (
	"encoding/json"
	"fmt"
	"os"

	"github.com/router-for-me/CLIProxyAPI/v8/internal/misc"
	sdkauthurl "github.com/router-for-me/CLIProxyAPI/v8/sdk/auth"
	coreusage "github.com/router-for-me/CLIProxyAPI/v8/sdk/cliproxy/usage"
)

type userAgentCase struct {
	In       string `json:"in"`
	Request  string `json:"request"`
	LoadCode string `json:"load_code_assist"`
	Onboard  string `json:"onboard_user"`
	Version  string `json:"version"`
}

func userAgents() map[string]any {
	inputs := []string{
		"",
		"   ",
		"antigravity/hub/2.2.1 darwin/arm64",
		"antigravity/1.23.2 windows/amd64",
		"antigravity/1.9.0 windows/amd64 google-api-nodejs-client/9.0.0",
		" Antigravity/1.9.0 windows/amd64 ",
		"ANTIGRAVITY/HUB/3.0.0\tlinux",
		"antigravity/hub/ x",
		"antigravity/ google-api-nodejs-client/1",
		"antigravity/hub/4.0.0 GOOGLE-API-NODEJS-CLIENT/2",
		"antigravity/hub/4.0.0google-api-nodejs-client/2",
		"curl/8.1",
		"Mozilla/5.0 antigravity/1.0.0",
	}
	var cases []userAgentCase
	for _, in := range inputs {
		cases = append(cases, userAgentCase{
			In:       in,
			Request:  misc.AntigravityRequestUserAgent(in),
			LoadCode: misc.AntigravityLoadCodeAssistUserAgent(in),
			Onboard:  misc.AntigravityOnboardUserUserAgent(in),
			Version:  misc.AntigravityVersionFromUserAgent(in),
		})
	}
	return map[string]any{
		"latest":     misc.AntigravityLatestVersion(),
		"user_agent": misc.AntigravityUserAgent(),
		"cases":      cases,
	}
}

// scenarioSets are executor scenarios recorded from a newer Go tree than the pinned one,
// selected by ANTIGRAVITY_GO_SET (see v8_0_27.go).
var scenarioSets = map[string]func() []execScenario{}

func main() {
	if len(os.Args) != 2 {
		fmt.Fprintln(os.Stderr, "usage: generator OUTPUT.json")
		os.Exit(2)
	}
	coreusage.RegisterPlugin(capturedUsage)
	if set := os.Getenv("ANTIGRAVITY_GO_SET"); set != "" {
		scenarios, ok := scenarioSets[set]
		if !ok {
			panic("unknown scenario set " + set + "; build with its tag")
		}
		execs := scenarios()
		defaultCredentials(execs)
		for i := range execs {
			runExec(&execs[i])
		}
		data, err := json.MarshalIndent(map[string]any{"go": set, "executor": execs}, "", "  ")
		if err != nil {
			panic(err)
		}
		if err := os.WriteFile(os.Args[1], append(data, '\n'), 0o644); err != nil {
			panic(err)
		}
		return
	}
	execs := execScenarios()
	defaultCredentials(execs)
	for i := range execs {
		runExec(&execs[i])
	}
	auth := authCases()
	for i := range auth {
		runAuth(&auth[i])
	}
	authURLs := map[string]string{
		"st4te|http://localhost:51121/oauth-callback": sdkauthurl.BuildAntigravityAuthURL("st4te", "http://localhost:51121/oauth-callback"),
		"s| ":                             sdkauthurl.BuildAntigravityAuthURL("s", " "),
		"a b&c|http://127.0.0.1:9/cb?x=1": sdkauthurl.BuildAntigravityAuthURL("a b&c", "http://127.0.0.1:9/cb?x=1"),
	}
	data, err := json.MarshalIndent(map[string]any{"user_agents": userAgents(), "auth": auth, "auth_urls": authURLs, "executor": execs}, "", "  ")
	if err != nil {
		panic(err)
	}
	if err := os.WriteFile(os.Args[1], append(data, '\n'), 0o644); err != nil {
		panic(err)
	}
}
