// Generates crates/cpa-core/src/config/legacy_paths.json from Go v8.0.35: the legacy
// leaf and struct paths that internal/config/config_v8.go buildV8Paths derives by
// walking the Config type, in its order, each with its v8 destination. The prefix
// table is copied from buildV8Paths (it is unexported); the walk is the same.
//
//	go run . ../../../src/config/legacy_paths.json
package main

import (
	"encoding/json"
	"os"
	"reflect"
	"strings"

	"github.com/router-for-me/CLIProxyAPI/v8/internal/config"
)

type configPath struct{ old, current string }

func main() {
	prefixes := []configPath{
		{"host", "server.host"}, {"port", "server.port"}, {"max-request-body-bytes", "server.max-request-body-bytes"}, {"trusted-proxies", "server.trusted-proxies"},
		{"github-token", "server.github-token"},
		{"tls", "server.tls"}, {"commercial-mode", "server.commercial-mode"}, {"discovery", "server.discovery"},
		{"remote-management", "management"}, {"api-keys", "access.api-keys"},
		{"credential-concurrency", "credentials.concurrency"}, {"credential-in-flight", "credentials.in-flight"},
		{"force-model-prefix", "routing.force-model-prefix"},
		{"request-retry", "routing.retry.request-retry"}, {"max-retry-credentials", "routing.retry.max-retry-credentials"},
		{"max-retry-interval", "routing.retry.max-retry-interval"},
		{"disable-cooling", "routing.cooldown.disable-cooling"}, {"save-cooldown-status", "routing.cooldown.save-cooldown-status"},
		{"transient-error-cooldown-seconds", "routing.cooldown.transient-error-cooldown-seconds"},
		{"proxy-url", "requests.proxy-url"}, {"tls-handshake-timeout-seconds", "requests.tls-handshake-timeout-seconds"}, {"passthrough-headers", "requests.passthrough-headers"},
		{"nonstream-keepalive-interval", "requests.nonstream-keepalive-interval"}, {"streaming", "requests.streaming"}, {"payload", "requests.payload"},
		{"auth-dir", "oauth.auth-dir"}, {"auth-auto-refresh-workers", "oauth.auth-auto-refresh-workers"},
		{"oauth-model-alias", "oauth.model-alias"}, {"oauth-excluded-models", "oauth.excluded-models"},
		{"oauth-request-scoped-errors", "oauth.request-scoped-errors"}, {"oauth-settings", "oauth.settings"}, {"ws-auth", "oauth.providers.aistudio.ws-auth"},
		{"codex.disable-codex-cloaking", "upstream.codex.disable-codex-cloaking"},
		{"codex.stream-bootstrap-buffering", "upstream.codex.stream-bootstrap-buffering"},
		{"codex.stream-bootstrap-timeout", "upstream.codex.stream-bootstrap-timeout"},
		{"codex.orphan-delegation-compatibility", "upstream.codex.orphan-delegation-compatibility"},
		{"codex.model-level-cooling", "upstream.codex.model-level-cooling"},
		{"codex.response-steering", "upstream.codex.response-steering"},
		{"codex", "oauth.providers.codex"}, {"codex-header-defaults", "oauth.providers.codex.header-defaults"},
		{"claude", "upstream.claude"}, {"claude-code", "upstream.claude"},
		{"disable-claude-cloak-mode", "upstream.claude.disable-claude-cloak-mode"},
		{"claude-header-defaults", "upstream.claude.header-defaults"},
		{"antigravity", "oauth.providers.antigravity"},
		{"antigravity-signature-cache-enabled", "oauth.providers.antigravity.signature-cache-enabled"},
		{"antigravity-signature-bypass-strict", "oauth.providers.antigravity.signature-bypass-strict"},
		{"quota-exceeded.antigravity-credits", "oauth.providers.antigravity.antigravity-credits"},
		{"xai", "upstream.xai"}, {"devin", "oauth.providers.devin"},
		{"disable-image-generation", "multimedia.disable-image-generation"}, {"gpt-image-2-base-model", "multimedia.gpt-image-2-base-model"},
		{"video-result-auth-cache-ttl", "multimedia.video-result-auth-cache-ttl"},
		{"debug", "observability.logs.debug"}, {"logging-to-file", "observability.logs.logging-to-file"},
		{"logs-max-total-size-mb", "observability.logs.logs-max-total-size-mb"}, {"request-log", "observability.logs.request-log"},
		{"error-logs-max-files", "observability.logs.error-logs-max-files"},
		{"usage-statistics-enabled", "observability.usage.usage-statistics-enabled"},
		{"redis-usage-queue-retention-seconds", "observability.usage.redis-usage-queue-retention-seconds"}, {"pprof", "observability.pprof"},
	}
	var leaves, structs [][2]string
	match := func(path string) (string, bool) {
		for _, prefix := range prefixes {
			if path == prefix.old || strings.HasPrefix(path, prefix.old+".") {
				return prefix.current + strings.TrimPrefix(path, prefix.old), true
			}
		}
		return "", false
	}
	var walk func(reflect.Type, string)
	walk = func(t reflect.Type, path string) {
		if t.Kind() == reflect.Struct {
			if current, ok := match(path); ok {
				structs = append(structs, [2]string{path, current})
			}
			for i := 0; i < t.NumField(); i++ {
				field := t.Field(i)
				tag := strings.Split(field.Tag.Get("yaml"), ",")[0]
				if tag == "-" || field.PkgPath != "" {
					continue
				}
				if field.Anonymous {
					walk(field.Type, path)
					continue
				}
				child := tag
				if path != "" {
					child = path + "." + tag
				}
				walk(field.Type, child)
			}
			return
		}
		if current, ok := match(path); ok {
			leaves = append(leaves, [2]string{path, current})
		}
	}
	walk(reflect.TypeOf(config.Config{}), "")
	data, err := json.MarshalIndent(map[string][][2]string{"leaves": leaves, "structs": structs}, "", " ")
	if err != nil {
		panic(err)
	}
	if err := os.WriteFile(os.Args[1], append(data, '\n'), 0o644); err != nil {
		panic(err)
	}
}
