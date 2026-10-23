# Frozen v8.0.35 reference update

The target is CLIProxyAPI [v8.0.35](https://github.com/router-for-me/CLIProxyAPI/tree/ae3ee3be303fb07fb8173279923413005bf32177),
commit `ae3ee3be303fb07fb8173279923413005bf32177`, released on 19 October 2026 UTC.
The Rust tree under audit is still v0.2.6 (`d1261f2`). This update changes the
inventory and its judgments, not runtime code, stored fixtures, or their recorded
Go versions. The preceding [rebaseline](rebaseline.md) keeps its original reference
and counts as a historical report.

## Inventory changes

All 1,687 existing IDs remain. New IDs append to each milestone's range, with
additions grouped separately so concurrent porting branches can retain their IDs.
The checklist now has 1,951 items:

| Kind | Added | Current total |
|---|---:|---:|
| Go test-suite files | 227 | 989 |
| Configuration leaves | 28 | 436 |
| Public routes | 3 | 77 |
| Management routes | 1 | 186 |
| Plugin RPC methods | 1 | 62 |
| Plugin data contracts | 3 | 31 |
| CLI flags | 1 | 34 |
| **All additions** | **264** | |

The new routes are `GET /v1/models/*model`, `POST /v1/audio/speech`,
`POST /v1/tts`, and `POST /v8/management/oauth/siwc/bind`. The probe gets an empty
404 for each. Speech returns buffered binary audio in Go, not generation SSE.
The new CLI flag is `--siwc-bind`, for binding an existing Codex OAuth credential.
The plugin additions are `host.routing.reset_cooldown`, `ConfigField.Default`,
`ThinkingSupport.DisableType`, and the paired `UsageRecord.IsNativeKey` and
`UsageRecord.AccessProvider` fields. Existing Rust plugin schemas lack these fields.

The 28 config leaves include eight provider-specific `thinking.disable-type`
paths, model catalog sources, Vertex Interactions, the process GitHub token,
request-body and TLS limits, session rebind policy, Codex web-search caching and
SIWC identity, Claude tool search and date retention, four Claude header controls,
and pre-first-token streaming keep-alives. Eighteen existing shared settings move
to canonical `upstream.*` paths. Their historical OAuth aliases remain accepted
in Go, with canonical presence taking precedence.

No old route, configuration leaf, or RPC method is deleted from the Go tree.
The checklist marks 43 removed test declarations in 17 existing files as
historical, outside the active case lists. Five other declarations move to a new
shadow-epoch suite and link to their new locations. Two Responses request
translator registrations now use request envelopes instead of byte-only request
functions. Their existing IDs record the replacement rather than adding duplicates.

The original inventory excluded 30 files: examples, catalog-fetch tools,
benchmark-only files, and support helpers. Those exclusions remain.
Go added 228 test files. The support-only `internal/cache/issue6332_export_test.go`
has no test declaration and is excluded like the other helpers, so 227 are
suites; its draft ID `M4-0520` stays unassigned. Current suite entries list their
actual Test, Fuzz, Example, and Benchmark declarations with current line numbers.
The translator implementation and test LOC counts also follow the frozen tree.

## Judgment changes

At the freeze, none of the additions was covered: 197 were partial and 67
missing. Later porting changes may cover an addition with evidence; the verifier
requires only that each addition keeps a judgment with evidence. `M1-0133`,
the Claude `/responses/compact` suite, is missing: Rust answers 501 there, as
Go `6fecc6e` did. Partial means the corresponding subsystem exists, but the
current Go assertions have not been verified. A source filename, a matching leaf name, or a test's old name does not
prove the added behavior.

Of the existing items, 96 change from covered to partial:

- Sixty-nine suites have changed Go source. All 208 changed existing suites receive
  explicit notes preserving their previous-reference evidence. Of the other 139,
  138 were already partial or missing, and `M5-0283` stays covered: its Go diff
  (+17/-2) only changes GitHub-token assertions in the panel downloader that
  cliproxy-rs deliberately lacks (owner-approved embedded dashboard). Current declarations replace old case lists;
  historical evidence can still contain its original case counts.
- Sixteen previously covered settings move to `upstream.*`. Two other moved
  settings were already partial. Existing alias tests do not prove canonical
  reads, cross-auth-kind semantics, and presence precedence.
- Ten changed contracts need new assertion coverage: cloak `never`/`uncloak`
  (`M1-0042`), local catalog overrides (`M1-0062`, `M1-0063`), the two request
  envelopes (`M2-0014`, `M3-0031`), historical config aliases (`M4-0230`), and four
  SIWC-sensitive v8 handlers (`M5-0042`, `M5-0043`, `M5-0065`, `M5-0066`).
- `M3-0283` additionally inventories an existing Benchmark declaration omitted
  from its old case list. Its two functional cases still match Rust tests; the
  benchmark has no matching replay.

For example, Responses steering still works through the old Rust setting.
`M3-0142` is partial now because Go's new canonical shared setting is unproven.
`M2-0146` is partial because its Go error-recovery suite changed; the unchanged
four-scenario full-duplex suite `M2-0147` stays covered. These judgments do not
reverse the evidence recorded in the previous-reference rebaseline.

`M5-0235` remains partial for the saturated-buffer disconnect-cleanup gap found
in the rebaseline review. No runtime fix is included here.

### Covered-to-partial items by area

These are all 96 downgraded IDs. The suite rows refer to changed Go test files
whose new assertions are not established by the existing Rust fixtures.
[checklist.md](checklist.md) lists each file's current declarations and source
citations. [manual.tsv](manual.tsv) retains the Rust evidence and the remaining
gap for each ID.

| Area | Items | Reason |
|---|---|---|
| claude (10) | M4-0134; M1-0048, M1-0049, M1-0050, M1-0051, M1-0052, M1-0053, M1-0055, M1-0056 | Nine canonical `upstream.claude.*` settings need shared-setting and presence-precedence tests. |
| claude | M1-0042 | Cloak `never` and `uncloak` change the contract. |
| codex (9) | M3-0138, M3-0139, M3-0140, M3-0141, M3-0142; M4-0133 | Six canonical `upstream.codex.*` settings need shared-setting and presence-precedence tests. |
| codex | M3-0223, M3-0245, M5-0290 | Changed client-model, compact-executor, and WebSocket-duplex suites. |
| google (11) | M3-0230, M3-0231; M2-0040, M2-0041; M4-0357, M4-0358, M4-0359; M1-0109; M2-0133; M3-0334 | Ten changed suites: Antigravity finish reasons, Interactions, and model deduplication; Gemini Interactions, content turns, and schemas; signature validation and provider compatibility; Claude thinking effort. |
| google | M3-0283 | Vertex payload helpers include a previously omitted Benchmark declaration. The two functional cases still match. |
| manage (5) | M4-0230 | Historical Codex multi-agent aliases need precedence coverage. |
| manage | M5-0042, M5-0043, M5-0065, M5-0066 | OAuth callback and credential handlers now include SIWC behavior. |
| openai-xai (1) | M3-0137 | Canonical `upstream.xai.inject-x-search` needs shared-setting and presence-precedence tests. |
| server (3) | M4-0324, M2-0146, M4-0504 | Changed apply-patch integration, Responses steering error-recovery, and session longest-common-prefix suites. |
| translate (55) | M2-0014, M3-0031 | Claude and Antigravity Responses request registrations now use envelopes. |
| translate | M3-0301, M3-0302, M3-0304, M3-0305, M3-0306, M3-0309 | Six changed Antigravity translator suites. |
| translate | M2-0057, M2-0060, M2-0061, M2-0062, M2-0066, M2-0067, M2-0069, M2-0070, M2-0072 | Nine changed Claude translator suites. |
| translate | M3-0310, M3-0312, M3-0314, M3-0315, M3-0316, M3-0318, M3-0320, M3-0321, M3-0323, M3-0324 | Ten changed Codex translator suites. |
| translate | M2-0088, M2-0089, M2-0090, M2-0093, M2-0094, M2-0095, M2-0096, M2-0097, M2-0098, M2-0099, M2-0100, M2-0102, M2-0104, M2-0106 | Fourteen changed Gemini translator suites. |
| translate | M2-0107, M2-0108 | Two changed Interactions translator suites. |
| translate | M2-0110, M2-0111, M2-0113, M2-0115, M2-0117, M2-0118, M2-0122, M2-0123, M2-0124, M2-0125, M2-0130, M2-0131 | Twelve changed OpenAI translator suites. |
| tui (2) | M1-0062, M1-0063 | Local model catalogs now override startup catalog sources. |

### Retired declarations by area

These 43 declarations no longer exist in their original Go files. They remain
historical notes in the checklist, outside active case lists. They are separate
from the five declarations moved to the new shadow-epoch suite. Area follows
the current audit owner.

#### claude (16 declarations)

`M1-0086`, `internal/runtime/executor/claude_executor_test.go`:

- `TestClaudeExecutor_ExecuteTransportMatchesResponseFormat`
- `TestClaudeExecutor_PayloadOverrideFableModelReconciled`
- `TestClaudeExecutor_PayloadOverrideNonFableToFableReconciled`
- `TestClaudeExecutor_PayloadOverrideUnrelatedModelRuleDoesNotPreserveFableFallbacks`
- `TestClaudeExecutor_PayloadOverrideMaxTokensTo1ReclassifiesAsProbe`
- `TestClaudeExecutor_PayloadOverrideFableToProbeStripsFableAdditions`
- `TestClaudeExecutor_PayloadOverrideProbeToNormalReinitializes`
- `TestClaudeExecutor_PayloadSystemTTLOverrideStillRemovesInjectedFableReportingBlock`
- `TestClaudeExecutor_PayloadSonnetToFableWithSystemTTLEditsAddsReportingBlock`
- `TestClaudeExecutor_PayloadStringSystemFableAddsReportingBlock`
- `TestClaudeExecutor_PayloadStringSystemMentioningReportingOutcomesStillInjectsFableReportingBlock`
- `TestClaudeExecutor_PayloadFableThinkingAdaptiveToDisabledDropsInjectedDisplay`
- `TestClaudeExecutor_PayloadSonnetToFableWithSensitiveWordsObfuscatesInjectedReportingBlock`
- `TestClaudeExecutor_PayloadFableToSonnetWithSensitiveWordsRemovesReportingBlock`
- `TestClaudeExecutor_PayloadReplacesSystemOnOriginalFableReaddsReportingBlock`

`M1-0093`, `internal/runtime/executor/claude_mid_system_model_test.go`:

- `TestClaudeExecutor_PayloadOverrideReconcilesRelocatedSystemPrompt`

#### codex (1 declaration)

`M3-0264`, `internal/runtime/executor/codex_per_credential_cloaking_issue6034_test.go`:

- `TestCodexV8OAuthCloakingDoesNotAffectAPIKeys`

#### google (11 declarations)

`M3-0225`, `internal/runtime/executor/aistudio_executor_test.go`:

- `TestAIStudioTranslateRequestNormalizesThinkingLevelAfterPayloadOverride`

`M2-0039`, `internal/runtime/executor/gemini_executor_test.go`:

- `TestGeminiExecutorAppliesPayloadRulesBeforeLeadingUserNormalization`

`M3-0334`, `sdk/cliproxy/antigravity_models_dedup_test.go`:

- `TestAntigravityCapabilityProbe_CachedWithinTTL`
- `TestAntigravityCapabilityProbe_CacheExpiresAfterTTL`
- `TestAntigravityCapabilityProbe_CachesEmptyWebSearchModelIDs`
- `TestAntigravityCapabilityProbe_CachesFailureWithBackoff`
- `TestAntigravityCapabilityProbe_InvalidJSONHandledAsFailure`
- `TestAntigravityCapabilityProbe_AuthErrorDoesNotPoisonCache`
- `TestAntigravityCapabilityProbe_ConcurrentAuthErrorDoesNotBlockHealthyAccount`

`M3-0335`, `sdk/cliproxy/antigravity_models_timeout_test.go`:

- `TestAntigravityCapabilityProbe_HardTimeout`
- `TestAntigravityCapabilityProbe_ConcurrentBaseURLs`

#### home (1 declaration)

`M4-0491`, `sdk/cliproxy/service_executionregistry_test.go`:

- `TestHomeConfigWorkerCancelsBlockedAntigravityModelRefreshBeforePublish`

#### manage (3 declarations)

`M4-0380`, `internal/watcher/synthesizer/file_test.go`:

- `TestFileSynthesizer_Synthesize_OAuthExcludedModelsMerged`
- `TestFileSynthesizer_Synthesize_MetaOAuthExcludedModelsMerged`

`M4-0381`, `internal/watcher/synthesizer/helpers_test.go`:

- `TestApplyAuthExcludedModelsMeta_OAuthMergeWritesCombinedModels`

#### plugins (3 declarations)

`M6-0175`, `internal/pluginhost/adapters_test.go`:

- `TestRegisterModelsRegistersProviderModelsAndClientID`
- `TestRegisterModelsPrunesStaleClientAfterSnapshotChange`

`M6-0184`, `internal/pluginhost/host_test.go`:

- `TestPluginLoadedTracksLoadedPluginAfterDisabled`

#### server (6 declarations)

`M4-0342`, `internal/runtime/executor/helps/response_model_test.go`:

- `TestIsCodexModelSubstituted`
- `TestUsageReporterThrottlesRepeatedSubstitutionWarnings`
- `TestUsageReporterThrottlesSubstitutionWarningsAcrossServedModelCase`
- `TestCodexModelSubstitutionThrottleBoundsStoredEntries`

`M4-0353`, `internal/runtime/executor/oauth_scope_test.go`:

- `TestV8OAuthRequestSettingsDoNotAffectAPIKeys`

`M4-0490`, `sdk/cliproxy/service_excluded_models_test.go`:

- `TestRegisterModelsForAuth_UsesPreMergedExcludedModelsAttribute`

#### translate (1 declaration)

`M3-0306`, `internal/translator/antigravity/openai/chat-completions/antigravity_openai_response_test.go`:

- `TestConvertAntigravityResponseToOpenAIIncludesZeroCompletionTokensWhenMissing`

#### tui (1 declaration)

`M4-0252`, `cmd/server/main_test.go`:

- `TestModelCatalogUpdaterPlan`

## Counts before and after

Counts are covered / partial / missing. Before means the completed v0.2.6
rebaseline against `6fecc6e`, not the stale audit preceding that review.

| Milestone | Before | Frozen v8.0.35 target |
|---|---|---|
| M1 | 66 / 52 / 0 | 54 / 83 / 14 |
| M2 | 113 / 45 / 0 | 71 / 114 / 4 |
| M3 | 216 / 134 / 0 | 187 / 215 / 22 |
| M4 | 240 / 256 / 14 | 232 / 330 / 27 |
| M5 | 201 / 102 / 0 | 196 / 125 / 10 |
| M6 | 114 / 115 / 19 | 114 / 130 / 23 |
| **All** | **950 / 704 / 33** | **854 / 997 / 100** |

Only gaps have area assignments, so area counts are partial / missing.
Shared config gaps follow their provider rather than the Go config file's owner.

| Area | Before | Frozen v8.0.35 target |
|---|---|---|
| claude | 48 / 0 | 75 / 13 |
| codex | 64 / 0 | 82 / 11 |
| device-providers | 20 / 0 | 28 / 2 |
| google | 36 / 0 | 80 / 4 |
| home | 25 / 0 | 27 / 0 |
| manage | 130 / 0 | 148 / 5 |
| observe | 1 / 12 | 1 / 12 |
| openai-xai | 27 / 1 | 31 / 6 |
| plugins | 86 / 18 | 99 / 22 |
| realtime | 8 / 0 | 8 / 0 |
| server | 222 / 2 | 286 / 19 |
| translate | 30 / 0 | 123 / 5 |
| tui | 7 / 0 | 9 / 1 |

The [upstream commit inventory](upstream-v8.0.35.md) is unchanged in classification:
288 unique commits (256 non-merge and 32 merge), with 19 ported, 205 to port,
zero not applicable, 61 nothing to port, and three deliberate differences.
The checklist counts contracts and files, so its totals do not equal commit totals.

## Verification

All Go inspection uses `git show`, `git diff`, and `git ls-tree` against a read-only
clone. The verifier fails explicitly, so `python -O` cannot skip it. It checks:

- the exact tag commit;
- stable IDs: all 1,687 retained IDs keep their milestone, section and contract
  text, compared without Go line numbers, LOC counts and link targets. Suite rows
  compare their text outside the fragments regenerated from Go (line range,
  declaration list, removed and moved declarations), each matched entry by entry so
  prose around or inside them stays compared; those fragments are checked against
  Go below. Each of the 18 `upstream.*`
  moves must name a canonical path and previous alias from Go's `config_v8.go`
  alias table and must equal its old text once that prefix is rewritten back. The
  other 10 rewrites are pinned to the approved text by a SHA-256 prefix;
- test suites: the 228 added Go files, 989 suite entries with their declarations
  and lines and a range covering the whole file, and the excluded support-only
  file. Each of the 43 retired declarations was declared in the same file at
  `6fecc6e` and is not at v8.0.35; each of the 5 moved declarations likewise left
  its file and is found at its cited target line;
- routes: each of Go's 260 router and `ServeMux` registrations under `internal/`
  and `sdk/` is cited by a route item with the same method and full path, and every
  route item cites one. Full paths resolve gin `Group` prefixes, nested groups and
  string constants; a `Handle(method, ...)` inside a method-list loop counts once
  per listed method; `ServeMux` registrations accept any method. The resolver
  recognizes the registration forms in the frozen tree (`receiver.METHOD(path, ...)`
  on the engine, a `ServeMux` or a `Group` variable); such a call it cannot reduce to
  methods and a full path fails unless listed. The only listed call is
  `AttachWebsocketRoute`'s `s.engine.GET(trimmed, ...)`, which registers the wsrelay
  gateway path passed in at runtime (`M5-0021`). Forms absent at v8.0.35, such as
  `Match` or a chained `Group(...).GET`, are not recognized;
- config leaves: the 434 YAML leaves walked from Go's `Config` struct equal the
  checklist's legacy paths, and the 28 added since `6fecc6e` equal the additions
  section;
- the 34 CLI flags in `cmd/server/main.go`, including the `discover` flag set;
- the 62 RPC method constants, and that each of the 7 exported plugin API fields
  added since `6fecc6e` is cited by an item narrower than a whole-file citation;
- 2,375 current, in-bounds source citations.

It does not check Rust behavior, whether item prose (handlers, types, defaults,
meanings) is accurate, plugin capabilities and data contracts that predate
`6fecc6e`, or the two routes Go serves without a statically resolved path registration
(`M5-0020`, the WebUI OAuth forwarder, and `M5-0021`, the AI Studio relay route)
and plugin-defined routes.

```sh
python3 docs/parity-audit/verify_reference.py /path/to/CLIProxyAPI
python3 -m unittest discover -s docs/parity-audit -p 'test_*.py'
unshare -Urn sh -c 'ip link set lo up; python3 docs/parity-audit/probe.py "$PWD/target/debug/cliproxy"'
python3 docs/parity-audit/audit.py
sha256sum README.md docs/PARITY.md docs/parity-audit/status.md > /tmp/parity-summary.sha256
python3 docs/parity-audit/audit.py
sha256sum -c /tmp/parity-summary.sha256
```

The sixteen Python tests pass, including the route resolver's group, constant,
loop, `ServeMux` and unresolved-call cases, and suite-text cases where prose
appended after or inserted inside a history fragment must stay compared, every
stripped history fragment and declaration is returned for validation, and a span
that only forms once another is removed (a declaration, a history note or a link)
is never stripped. Links are stripped first; one pass over that text then replaces
each declaration and history span with a sentinel that checklist text may not
contain, so no removal can join its neighbours; line numbers are normalized last.
A skeleton that still holds a declaration or history marker fails with the row ID. The route matcher accepts ordinary query strings and
wildcard paths, but rejects route suffixes hidden in queries, embedded quotes,
and line breaks. Its regression fails in six cases before the matcher fix.
The matcher fix does not change any current judgments.
The namespace denies external network access and the
probe uses private config and credential directories. It finds 237 of 241 routed
method/path pairs. The second audit leaves all three generated pages unchanged.
No live provider traffic, deployment, or fixture regeneration is part of this update.
