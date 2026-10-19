# Go commits from 6fecc6e to v8.0.27

cliproxy-rs follows CLIProxyAPI at [`6fecc6e`](https://github.com/router-for-me/CLIProxyAPI/commit/6fecc6e5567912661654a4eaf9b8f5436facd1c2) (v8.0.10). Go has since shipped v8.0.11 to v8.0.27 ([`bf93416`](https://github.com/router-for-me/CLIProxyAPI/commit/bf9341629ef2300ac9d846a2e80fe2756f63a671)): 167 commits, 21 of them merges. This file lists every one of them, grouped by area, and is the plan for porting them.

The 12 commits marked "ported here" are ported already and follow v8.0.27: the Claude prompt-cache and prompt-shaping fixes, Claude thread continuation, and Claude rate-limit and error answers. Go goldens recorded from v8.0.27 (`crates/cpa-exec/tests/reference/claude/README.md`) or Rust tests ported from Go's own cases check them. The exception is where a cloaked API key's caller system prompt goes: it stays as at `6fecc6e`, because v8.0.27's placement changes between the first and second turn of a conversation and so misses the prompt cache (`docs/DIFFERENCES-FROM-GO.md`). Everything else still follows `6fecc6e`.

Columns:

- User-visible: whether the commit changes what a client, an operator or an upstream sees (bytes on the wire, answers, routing, config). Refactors, races fixed without a visible symptom, logs and tests are "no".
- This PR: "ported here", "to port" (a later PR), "not applicable" (the code it changes has no counterpart in cliproxy-rs; Antigravity accounts are not ported, see the README's upcoming features), "nothing to port" (docs, assets, Go tests, merges, or behaviour cliproxy-rs already has), or "differs on purpose" (listed in `docs/DIFFERENCES-FROM-GO.md`).
- Size: lines added and removed in Go, without tests and test data. S is under 50 lines, M under 300, L 300 or more. The size of the Rust port can differ.

## Totals

| Area | Commits | Ported here | To port | Not applicable | Nothing to port | Differs on purpose |
|---|---:|---:|---:|---:|---:|---:|
| Claude: prompt cache and prompt shaping | 10 | 6 | 0 | 0 | 1 | 3 |
| Claude: thread continuation | 4 | 4 | 0 | 0 | 0 | 0 |
| Claude: rate limits and error answers | 2 | 2 | 0 | 0 | 0 | 0 |
| Claude: other executor work | 2 | 0 | 2 | 0 | 0 | 0 |
| Claude: Responses tool_search | 12 | 0 | 12 | 0 | 0 | 0 |
| Translation to and from Claude | 12 | 0 | 12 | 0 | 0 | 0 |
| Payload rules | 1 | 0 | 1 | 0 | 0 | 0 |
| Codex | 15 | 0 | 15 | 0 | 0 | 0 |
| Gemini, Vertex and AI Studio | 9 | 0 | 9 | 0 | 0 | 0 |
| xAI | 6 | 0 | 6 | 0 | 0 | 0 |
| OpenAI-compatible and Responses translation | 5 | 0 | 5 | 0 | 0 | 0 |
| Devin and Meta | 3 | 0 | 3 | 0 | 0 | 0 |
| Credentials, selection and cooldowns | 10 | 0 | 10 | 0 | 0 | 0 |
| Model registry and catalogs | 4 | 0 | 3 | 0 | 1 | 0 |
| Config and Management API | 7 | 0 | 7 | 0 | 0 | 0 |
| Plugins | 4 | 0 | 4 | 0 | 0 | 0 |
| Server, handlers and streaming | 4 | 0 | 4 | 0 | 0 | 0 |
| Usage and logging | 2 | 0 | 2 | 0 | 0 | 0 |
| Antigravity | 19 | 0 | 0 | 19 | 0 | 0 |
| Docs and assets | 11 | 0 | 0 | 0 | 11 | 0 |
| Go tests only | 4 | 0 | 0 | 0 | 4 | 0 |
| Merge commits | 21 | 0 | 0 | 0 | 21 | 0 |
| **All** | **167** | **12** | **95** | **19** | **38** | **3** |

Suggested order for the commits still to port: the rest of the Claude executor (`e9c8769`, `2bc236d`, `3b69068`) and Claude translation, then credentials and cooldowns (`c163bae`, `50d5fe1`, `935aa6e`), Codex, the payload rule barrier (`868e137`, which changes where every executor applies payload rules and removes the Claude post-rule repairs), the Responses `tool_search` series as one piece, and then the rest by area.

## Claude: prompt cache and prompt shaping

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`6d57ac90`](https://github.com/router-for-me/CLIProxyAPI/commit/6d57ac90) | fix(claude): pin session date in cloaked reminder to prevent prompt cache invalidation | yes | ported here | M (+77 −15) | currentDate reminder pinned per session; replays its Go tests |
| [`5dbce4f3`](https://github.com/router-for-me/CLIProxyAPI/commit/5dbce4f3) | perf(claude): prefilter JSON payloads before diagnostic walks | no | ported here | M (+59 −1) | byte prefilter before the title-helper and 1h-TTL walks |
| [`5d890405`](https://github.com/router-for-me/CLIProxyAPI/commit/5d890405) | fix(claude): keep system messages top level for terminal user turns | yes | differs on purpose | S (+31 −0) | keeps an API key's caller system text top-level for a terminal user run, then moves it on the second turn, which misses the cache; cliproxy-rs keeps it after the leading user run (DIFFERENCES-FROM-GO.md) |
| [`26e5efbb`](https://github.com/router-for-me/CLIProxyAPI/commit/26e5efbb) | fix(claude): reserve cache breakpoint for thread continuation | yes | ported here | S (+6 −0) | a `thread` request keeps one breakpoint free |
| [`eb6a768d`](https://github.com/router-for-me/CLIProxyAPI/commit/eb6a768d) | feat(claude): support explicit prompt cache options | yes | ported here | M (+235 −64) | `prompt_cache_options.mode: explicit`; also marks caller blocks kept beside advisor history |
| [`eca1d113`](https://github.com/router-for-me/CLIProxyAPI/commit/eca1d113) | fix(claude): keep system prompt at top level for single terminal user turn | yes | differs on purpose | S (+1 −1) | single terminal user turn; as `5d890405` |
| [`39632e52`](https://github.com/router-for-me/CLIProxyAPI/commit/39632e52) | fix(claude): keep system prompt at top level for leading user message runs | yes | differs on purpose | S (+1 −1) | leading user run of several turns; as `5d890405` |
| [`14dc4e05`](https://github.com/router-for-me/CLIProxyAPI/commit/14dc4e05) | fix(claude): relocate caller system prompt to messages for OAuth requests | yes | nothing to port | S (+17 −8) | OAuth tokens always relocate caller system text, in Messages and count_tokens, as cliproxy-rs does for API keys too; checked by the v8.0.27 goldens |
| [`2d8ec73b`](https://github.com/router-for-me/CLIProxyAPI/commit/2d8ec73b) | fix(claude): preserve mid-conversation effort directives during system insertion | yes | ported here | S (+25 −12) | effort directives do not end the leading user run |
| [`be8c35f2`](https://github.com/router-for-me/CLIProxyAPI/commit/be8c35f2) | fix(claude): ignore invalid and null cache control entries in executor | yes | ported here | S (+14 −18) | null and non-ephemeral `cache_control` are not breakpoints |

## Claude: thread continuation

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`5c959cb0`](https://github.com/router-for-me/CLIProxyAPI/commit/5c959cb0) | fix(claude): mark missing thread state error for client replay | yes | ported here | M (+64 −4) | `details.error_code: thread_not_found`; a request fault, so no failover or cooldown |
| [`678a9257`](https://github.com/router-for-me/CLIProxyAPI/commit/678a9257) | feat(claude): preserve OAuth tool aliases across thread continuations | yes | ported here | M (+158 −4) | aliases kept per thread turn (1,024 keys) |
| [`a2976eb8`](https://github.com/router-for-me/CLIProxyAPI/commit/a2976eb8) | fix(claude): share OAuth tool alias store across scoped executor copies | no | ported here | S (+14 −6) | one store per executor; the Rust executor is never copied per request |
| [`e7b22260`](https://github.com/router-for-me/CLIProxyAPI/commit/e7b22260) | fix(claude): support plain-text and structured response bodies for missing thread errors | yes | ported here | S (+18 −7) | plain-text missing-thread errors |

## Claude: rate limits and error answers

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`42d484f9`](https://github.com/router-for-me/CLIProxyAPI/commit/42d484f9) | fix(claude): map request timeout status to timeout_error | yes | ported here | S (+1 −1) | 408 answers `timeout_error` |
| [`80809679`](https://github.com/router-for-me/CLIProxyAPI/commit/80809679) | fix(claude): ignore overage billing resets in unified rate limit parsing | yes | ported here | M (+60 −5) | overage billing resets and resets past seven days set no cooldown |

## Claude: other executor work

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`2bc236d2`](https://github.com/router-for-me/CLIProxyAPI/commit/2bc236d2) | feat(claude): support responses compaction in claude executor | yes | to port | L (+318 −4) | `/responses/compact` through Claude; Rust answers 501 as Go 6fecc6e did |
| [`e9c8769a`](https://github.com/router-for-me/CLIProxyAPI/commit/e9c8769a) | fix(executor): normalize and restore non-portable tool call IDs | yes | to port | M (+129 −3) | tool_use IDs rewritten to Anthropic's charset and restored (also Devin) |

## Claude: Responses tool_search

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`58c0f6a2`](https://github.com/router-for-me/CLIProxyAPI/commit/58c0f6a2) | feat(claude): support Codex client tool_search on the Responses API | yes | to port | L (+295 −74) | Codex `tool_search` as Anthropic native tool search |
| [`a68ad134`](https://github.com/router-for-me/CLIProxyAPI/commit/a68ad134) | fix(claude): address tool_search review findings | yes | to port | M (+67 −33) | follow-up fixes to the above |
| [`48ad3487`](https://github.com/router-for-me/CLIProxyAPI/commit/48ad3487) | feat(executor): add Responses tool_search strip and legacy model helpers | yes | to port | M (+158 −0) | helpers that strip `tool_search` for unsupported routes |
| [`aeec5e97`](https://github.com/router-for-me/CLIProxyAPI/commit/aeec5e97) | feat(claude): emit native deferred tools and tool_reference for tool_search | yes | to port | S (+40 −4) | deferred tools and `tool_reference` blocks |
| [`ae33711c`](https://github.com/router-for-me/CLIProxyAPI/commit/ae33711c) | feat(config): add native-tool-search switch for Claude API keys | yes | to port | S (+12 −0) | `native-tool-search` switch per Claude API key |
| [`52992659`](https://github.com/router-for-me/CLIProxyAPI/commit/52992659) | feat(claude): fall back to pre-tool-search translation on unsupported upstreams | yes | to port | M (+52 −0) | fallback translation for unsupported upstreams |
| [`11f04c20`](https://github.com/router-for-me/CLIProxyAPI/commit/11f04c20) | feat(codex): advertise tool search only for natively supported Claude routes | yes | to port | M (+135 −20) | Codex model catalog advertises tool search per route |
| [`8eed08ef`](https://github.com/router-for-me/CLIProxyAPI/commit/8eed08ef) | fix(claude): drop cache_control from deferred tool_search tools | yes | to port | S (+2 −0) | no `cache_control` on deferred tools |
| [`77e3d0d3`](https://github.com/router-for-me/CLIProxyAPI/commit/77e3d0d3) | feat(config): add upstream.claude.tool-search switch | yes | to port | S (+18 −1) | `upstream.claude.tool-search` switch |
| [`956c3da1`](https://github.com/router-for-me/CLIProxyAPI/commit/956c3da1) | feat(claude): gate Responses tool_search translation behind upstream.claude.tool-search | yes | to port | M (+52 −14) | translation gated behind the switch |
| [`03a3cfec`](https://github.com/router-for-me/CLIProxyAPI/commit/03a3cfec) | feat(claude): honor upstream.claude.tool-search in executor, catalog and server | yes | to port | S (+29 −5) | executor, catalog and server honour the switch |
| [`29bd0415`](https://github.com/router-for-me/CLIProxyAPI/commit/29bd0415) | fix(claude): pin upstream.claude.tool-search per request | yes | to port | M (+130 −64) | switch pinned per request |

## Translation to and from Claude

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`e6f6f26a`](https://github.com/router-for-me/CLIProxyAPI/commit/e6f6f26a) | fix(claude): handle pause_turn stop reason in responses translation | yes | to port | S (+7 −1) | Claude `pause_turn` in Responses output |
| [`2c1dcc7c`](https://github.com/router-for-me/CLIProxyAPI/commit/2c1dcc7c) | fix(claude): include web search sources in codex request when search tool is present | yes | to port | S (+7 −1) | Claude to Codex: web search sources requested |
| [`61a70dc6`](https://github.com/router-for-me/CLIProxyAPI/commit/61a70dc6) | feat(claude): adapt native messages JSON to SSE for non-stream translation | yes | to port | M (+90 −0) | non-stream Claude JSON adapted to SSE for Chat and Responses |
| [`33ec5502`](https://github.com/router-for-me/CLIProxyAPI/commit/33ec5502) | fix(claude): refine model version parsing for assistant prefill rejection | yes | to port | S (+31 −4) | model version parsing for the assistant prefill rule |
| [`68afd0d1`](https://github.com/router-for-me/CLIProxyAPI/commit/68afd0d1) | feat(codex): support action sources in Claude web search translation | yes | to port | S (+11 −3) | Codex web search action sources in Claude output |
| [`ba1bbd75`](https://github.com/router-for-me/CLIProxyAPI/commit/ba1bbd75) | fix(gemini): support document blocks in Claude request translation | yes | to port | S (+4 −2) | Claude to Gemini: document blocks |
| [`d510dccf`](https://github.com/router-for-me/CLIProxyAPI/commit/d510dccf) | fix(gemini): support camelCase systemInstruction in request translation | yes | to port | S (+5 −1) | Gemini to Claude: camelCase `systemInstruction` |
| [`a0d31924`](https://github.com/router-for-me/CLIProxyAPI/commit/a0d31924) | fix(gemini): map refusal finish reasons and handle partless stream completion in Claude response translation | yes | to port | M (+87 −41) | Gemini to Claude: refusal finish reasons, partless completion |
| [`68ea171d`](https://github.com/router-for-me/CLIProxyAPI/commit/68ea171d) | fix(gemini): handle carrier thinking blocks for signed visible text and tool calls in Claude response translation | yes | to port | M (+156 −89) | Gemini to Claude: signed carrier thinking blocks |
| [`3b69068e`](https://github.com/router-for-me/CLIProxyAPI/commit/3b69068e) | fix(translator): stop dropping attachments and empty turns | yes | to port | L (+453 −141) | attachments and empty turns no longer dropped (many translators, Claude executor) |
| [`a881c036`](https://github.com/router-for-me/CLIProxyAPI/commit/a881c036) | fix(claude): respect registry completion limit for default responses max tokens | yes | to port | S (+12 −4) | Responses to Claude: default `max_tokens` from the registry |
| [`9a7db8e3`](https://github.com/router-for-me/CLIProxyAPI/commit/9a7db8e3) | fix(claude): defer stream message delta emission to trailing usage chunk | yes | to port | S (+5 −3) | OpenAI to Claude stream: `message_delta` waits for trailing usage |

## Payload rules

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`868e137f`](https://github.com/router-for-me/CLIProxyAPI/commit/868e137f) | feat(payload): introduce payload finalizer for protobuf and multipart requests | yes | to port | L (+585 −222) | payload rules move to a final barrier for every executor; Claude's post-rule repairs go |

## Codex

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`8fbf152b`](https://github.com/router-for-me/CLIProxyAPI/commit/8fbf152b) | fix(executor): preserve top-level responses token usage when service tier is present | yes | to port | S (+26 −6) | top-level Responses usage kept beside `service_tier` |
| [`d306f2c5`](https://github.com/router-for-me/CLIProxyAPI/commit/d306f2c5) | fix(codex): retain template freeform apply_patch capability by default | yes | to port | S (+9 −5) | freeform `apply_patch` kept by default |
| [`2044a01f`](https://github.com/router-for-me/CLIProxyAPI/commit/2044a01f) | fix(codex): preserve web search sources in responses request include | yes | to port | S (+22 −4) | web search sources kept in `include` |
| [`a3b77566`](https://github.com/router-for-me/CLIProxyAPI/commit/a3b77566) | fix(codex): publish main usage before image tool usage in execute | yes | to port | S (+1 −1) | main usage published before image tool usage |
| [`d7914afd`](https://github.com/router-for-me/CLIProxyAPI/commit/d7914afd) | fix(codex): expand integer field normalization mappings for tools | yes | to port | M (+107 −21) | integer field normalization for tool schemas |
| [`d31b61cb`](https://github.com/router-for-me/CLIProxyAPI/commit/d31b61cb) | fix(websockets): propagate upstream disconnect errors during session activation | yes | to port | S (+47 −2) | upstream disconnects during session activation (Codex and xAI) |
| [`16d98881`](https://github.com/router-for-me/CLIProxyAPI/commit/16d98881) | feat(codex): preserve URL citations in OpenAI response translation | yes | to port | M (+125 −9) | URL citations in Chat Completions output |
| [`c7b4d573`](https://github.com/router-for-me/CLIProxyAPI/commit/c7b4d573) | fix(codex): record reasoning effort after applying payload overrides | yes | to port | S (+5 −5) | reasoning effort recorded after payload overrides |
| [`6331b38b`](https://github.com/router-for-me/CLIProxyAPI/commit/6331b38b) | feat(openai): support response.interrupt in responses websocket handling | yes | to port | M (+257 −24) | `response.interrupt` on the Responses WebSocket |
| [`51bde0ff`](https://github.com/router-for-me/CLIProxyAPI/commit/51bde0ff) | fix(codex): preserve resolved upstream model in direct openai image requests | yes | to port | S (+42 −7) | resolved upstream model kept for direct image requests |
| [`d318bcc3`](https://github.com/router-for-me/CLIProxyAPI/commit/d318bcc3) | fix(codex): handle late response interrupts on websocket sessions | yes | to port | M (+106 −13) | late `response.interrupt` on WebSocket sessions |
| [`b0debb35`](https://github.com/router-for-me/CLIProxyAPI/commit/b0debb35) | fix(codex): shorten over-length tool call IDs in chat completions | yes | to port | S (+26 −0) | over-length tool call IDs shortened |
| [`7b0319fb`](https://github.com/router-for-me/CLIProxyAPI/commit/7b0319fb) | feat(codex): support Sign in with ChatGPT | yes | to port | L (+2384 −256) | Sign in with ChatGPT |
| [`e240bd84`](https://github.com/router-for-me/CLIProxyAPI/commit/e240bd84) | fix(codex): fallback default model template when gpt-5.5 is missing | yes | to port | M (+85 −25) | default model template when gpt-5.5 is missing |
| [`1e37b52b`](https://github.com/router-for-me/CLIProxyAPI/commit/1e37b52b) | fix(executor): preserve websocket sessions across reload and auth removal | yes | to port | M (+199 −33) | WebSocket sessions survive reload and credential removal |

## Gemini, Vertex and AI Studio

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`5ec31442`](https://github.com/router-for-me/CLIProxyAPI/commit/5ec31442) | fix(gemini): handle split usage metadata and max tokens in responses translation | yes | to port | M (+183 −77) | split usage metadata and max tokens in Responses translation |
| [`ed4d972e`](https://github.com/router-for-me/CLIProxyAPI/commit/ed4d972e) | fix(responses): complete reasoning summary stream lifecycle events | yes | to port | S (+32 −13) | Interactions to Responses: reasoning summary events |
| [`e1e3a811`](https://github.com/router-for-me/CLIProxyAPI/commit/e1e3a811) | fix(gemini): enforce object type on schema nodes with properties | yes | to port | S (+36 −0) | `type: object` on schema nodes with properties |
| [`dbc4aad9`](https://github.com/router-for-me/CLIProxyAPI/commit/dbc4aad9) | fix(gemini): ignore context cancellation after terminal chunk in Vertex stream | yes | to port | S (+43 −0) | Vertex: cancellation after the terminal chunk |
| [`f51624aa`](https://github.com/router-for-me/CLIProxyAPI/commit/f51624aa) | fix(translator): report unsendable user turn attachments in openai responses to interactions | yes | to port | L (+1462 −356) | Responses to Interactions: unsendable attachments reported |
| [`0f96f568`](https://github.com/router-for-me/CLIProxyAPI/commit/0f96f568) | refactor(translator): update openai and interactions request converters to return errors | no | to port | L (+126 −231) | request converters return errors (refactor behind f51624aa) |
| [`2d5351b3`](https://github.com/router-for-me/CLIProxyAPI/commit/2d5351b3) | feat(vertex): add support for native gemini interactions api | yes | to port | L (+591 −110) | Vertex native Interactions API |
| [`15d2148b`](https://github.com/router-for-me/CLIProxyAPI/commit/15d2148b) | fix(gemini): ensure trailing user turn for function response with media | yes | to port | S (+24 −1) | trailing user turn for a function response with media |
| [`9126772d`](https://github.com/router-for-me/CLIProxyAPI/commit/9126772d) | fix(gemini): strip unsupported dependent schema keywords | yes | to port | S (+1 −0) | unsupported dependent schema keywords stripped |

## xAI

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`6a853dce`](https://github.com/router-for-me/CLIProxyAPI/commit/6a853dce) | feat(xai): expose Grok TTS on /v1/audio/speech | yes | to port | L (+376 −9) | Grok TTS on `/v1/audio/speech` |
| [`57bde351`](https://github.com/router-for-me/CLIProxyAPI/commit/57bde351) | fix(executor): preserve upstream usage on apply patch failures | yes | to port | M (+53 −19) | upstream usage kept on apply_patch failures (xAI and Meta) |
| [`306c9bd8`](https://github.com/router-for-me/CLIProxyAPI/commit/306c9bd8) | fix(routing): restrict speech-only models to speech endpoints and refine xAI error scoping | yes | to port | M (+113 −5) | speech-only models limited to speech routes; xAI error scoping |
| [`90654da5`](https://github.com/router-for-me/CLIProxyAPI/commit/90654da5) | feat(xai): dynamically fetch latest Grok CLI version from npm registry with fallback | yes | to port | M (+200 −3) | Grok CLI version fetched from npm at startup |
| [`34e73c75`](https://github.com/router-for-me/CLIProxyAPI/commit/34e73c75) | fix(xai): add proxy support and semver validation to Grok CLI updater | yes | to port | M (+131 −32) | proxy and semver checks for the version fetch |
| [`76c87ce9`](https://github.com/router-for-me/CLIProxyAPI/commit/76c87ce9) | fix(xai): reset terminal error on websocket connection configuration | yes | to port | S (+1 −0) | WebSocket terminal error reset |

## OpenAI-compatible and Responses translation

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`91c934f6`](https://github.com/router-for-me/CLIProxyAPI/commit/91c934f6) | feat(responses): support local shell tool translation | yes | to port | M (+213 −6) | Responses `local_shell` tool |
| [`97dd9eec`](https://github.com/router-for-me/CLIProxyAPI/commit/97dd9eec) | fix(openai): report error on truncated stream missing finish_reason | yes | to port | S (+33 −3) | error on a truncated stream without `finish_reason` |
| [`0ea4e1dc`](https://github.com/router-for-me/CLIProxyAPI/commit/0ea4e1dc) | fix(openai): complete responses stream on clean EOF after finish reason | yes | to port | M (+56 −6) | Responses stream completes on clean EOF after the finish reason |
| [`d2597a24`](https://github.com/router-for-me/CLIProxyAPI/commit/d2597a24) | fix(openai): preserve unknown encrypted content in responses compat mode | yes | to port | S (+5 −1) | unknown encrypted content kept in Responses compat mode |
| [`f8e4a840`](https://github.com/router-for-me/CLIProxyAPI/commit/f8e4a840) | fix(openai-compat): measure streaming TTFT on effective token frames | no | to port | S (+4 −2) | TTFT measured on token frames (usage timing only) |

## Devin and Meta

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`ea8ffd5f`](https://github.com/router-for-me/CLIProxyAPI/commit/ea8ffd5f) | fix(devin): stream content early and defer thought stop for late signatures | yes | to port | S (+27 −7) | Devin streams content early, late thought signatures |
| [`249e9ef9`](https://github.com/router-for-me/CLIProxyAPI/commit/249e9ef9) | fix(meta): preserve foreign reasoning encrypted content in responses requests | yes | to port | S (+14 −5) | Meta: foreign reasoning encrypted content kept |
| [`cad1b503`](https://github.com/router-for-me/CLIProxyAPI/commit/cad1b503) | fix(devin): do not seed input token estimates for claude streams | yes | to port | S (+4 −3) | Devin: no seeded input tokens for Claude streams |

## Credentials, selection and cooldowns

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`c163bae4`](https://github.com/router-for-me/CLIProxyAPI/commit/c163bae4) | fix(auth): preserve terminal unauthorized state and prevent unsafe refresh retry | yes | to port | M (+132 −26) | terminal unauthorized state and single-use refresh tokens; touches Claude OAuth refresh |
| [`d4692663`](https://github.com/router-for-me/CLIProxyAPI/commit/d4692663) | fix(auth): snapshot auth before unlocking during registration and update | no | to port | S (+12 −2) | race: snapshot before unlock (check the Rust scheduler for the same window) |
| [`8a01bc02`](https://github.com/router-for-me/CLIProxyAPI/commit/8a01bc02) | feat(auth): introduce mutation locking and persistence delta merging | no | to port | M (+210 −30) | mutation locking and persistence delta merging |
| [`50d5fe18`](https://github.com/router-for-me/CLIProxyAPI/commit/50d5fe18) | fix(cliproxy): avoid selecting rejected credentials during refresh and preserve active quota cooldowns | yes | to port | M (+68 −6) | rejected credentials not selected during refresh; quota cooldowns kept |
| [`6920ed5d`](https://github.com/router-for-me/CLIProxyAPI/commit/6920ed5d) | fix(cliproxy): propagate credential version and registration epoch in execution results | no | to port | M (+80 −18) | credential version and registration epoch in results |
| [`935aa6e3`](https://github.com/router-for-me/CLIProxyAPI/commit/935aa6e3) | fix(cliproxy): prevent in-flight token refresh from overwriting concurrently updated credentials | yes | to port | S (+6 −0) | in-flight refresh does not overwrite a newer credential |
| [`7c2133e2`](https://github.com/router-for-me/CLIProxyAPI/commit/7c2133e2) | fix(cliproxy): reconcile concurrent auth updates during batch model registration | no | to port | S (+39 −8) | concurrent auth updates during batch model registration |
| [`4a8ecf5c`](https://github.com/router-for-me/CLIProxyAPI/commit/4a8ecf5c) | fix(auth): skip local filtering for home dispatch | yes | to port | S (+7 −2) | Home dispatch skips local filtering |
| [`5dbd92a0`](https://github.com/router-for-me/CLIProxyAPI/commit/5dbd92a0) | fix(auth): preserve upstream error cooldown on canceled stream contexts | yes | to port | S (+39 −4) | upstream error cooldown kept on canceled streams |
| [`7a687f42`](https://github.com/router-for-me/CLIProxyAPI/commit/7a687f42) | feat(routing): support session-affinity-rebind-on option | yes | to port | L (+297 −19) | `session-affinity-rebind-on` |

## Model registry and catalogs

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`1672be5d`](https://github.com/router-for-me/CLIProxyAPI/commit/1672be5d) | feat(registry): support custom model catalog sources and dynamic reload | yes | to port | L (+361 −287) | custom model catalog sources and reload |
| [`01e28567`](https://github.com/router-for-me/CLIProxyAPI/commit/01e28567) | fix(registry): disable Devin catalog for home builds | yes | to port | S (+3 −1) | no Devin catalog in Home builds |
| [`18b6cc8a`](https://github.com/router-for-me/CLIProxyAPI/commit/18b6cc8a) | refactor(registry): use model getters for Devin and Meta | no | nothing to port | S (+2 −3) | Go refactor |
| [`bbcd5734`](https://github.com/router-for-me/CLIProxyAPI/commit/bbcd5734) | fix(registry): treat credential_quota as quota cooldown in model availability | yes | to port | S (+23 −13) | `credential_quota` counts as a quota cooldown in availability |

## Config and Management API

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`52d5507d`](https://github.com/router-for-me/CLIProxyAPI/commit/52d5507d) | feat(config): add shared upstream provider settings | yes | to port | L (+205 −156) | shared `upstream` provider settings |
| [`3be5fa44`](https://github.com/router-for-me/CLIProxyAPI/commit/3be5fa44) | fix(config): support historical v8 aliases | yes | to port | M (+212 −46) | historical v8 config aliases |
| [`0fb50a18`](https://github.com/router-for-me/CLIProxyAPI/commit/0fb50a18) | feat(management): inject auth indexes into v8 api-keys config | yes | to port | L (+503 −8) | auth indexes in v8 `api-keys` |
| [`7c41cd05`](https://github.com/router-for-me/CLIProxyAPI/commit/7c41cd05) | feat: add global server GitHub token configuration | yes | to port | M (+78 −22) | global server GitHub token |
| [`0a23627d`](https://github.com/router-for-me/CLIProxyAPI/commit/0a23627d) | fix(config): normalize collection styles and unquote mapping keys in v8 api | yes | to port | S (+20 −7) | v8 API: collection styles and quoted mapping keys |
| [`0c708401`](https://github.com/router-for-me/CLIProxyAPI/commit/0c708401) | fix(management): resolve v8 plugin quota contract gaps | yes | to port | M (+64 −41) | v8 plugin quota contract |
| [`3d61c954`](https://github.com/router-for-me/CLIProxyAPI/commit/3d61c954) | fix(management): conditionally expose quota_plugin_id for v8 auth files | yes | to port | S (+3 −1) | `quota_plugin_id` on v8 auth files |

## Plugins

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`8348923a`](https://github.com/router-for-me/CLIProxyAPI/commit/8348923a) | feat(pluginhost): add host routing reset cooldown callback | yes | to port | M (+66 −0) | host routing reset-cooldown callback |
| [`67465884`](https://github.com/router-for-me/CLIProxyAPI/commit/67465884) | fix(pluginhost): detach unselected plugins on config apply | yes | to port | M (+69 −0) | unselected plugins detached on config apply |
| [`6e4c54e5`](https://github.com/router-for-me/CLIProxyAPI/commit/6e4c54e5) | feat(pluginhost): expose quota provider plugin IDs | yes | to port | S (+9 −0) | quota provider plugin IDs |
| [`fdbc6103`](https://github.com/router-for-me/CLIProxyAPI/commit/fdbc6103) | feat(auth): enhance plugin auth handling with multi-auth support and improved state management | yes | to port | M (+250 −32) | multi-auth plugin credentials |

## Server, handlers and streaming

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`c997cbb5`](https://github.com/router-for-me/CLIProxyAPI/commit/c997cbb5) | feat(httpwire): introduce error-aware response flushing helper | no | to port | L (+328 −66) | error-aware flushing (Go `http.Flusher` plumbing; check the Rust writers) |
| [`cd9e5ace`](https://github.com/router-for-me/CLIProxyAPI/commit/cd9e5ace) | fix(api): compact json error response bodies for SSE compatibility | yes | to port | S (+6 −1) | upstream JSON error bodies compacted to one line for SSE |
| [`dd7b4334`](https://github.com/router-for-me/CLIProxyAPI/commit/dd7b4334) | feat(server): support max-request-body-bytes request body limit | yes | to port | M (+222 −68) | `max-request-body-bytes` |
| [`519d590e`](https://github.com/router-for-me/CLIProxyAPI/commit/519d590e) | feat(streaming): support send-keepalive-before-ft option | yes | to port | L (+475 −123) | `send-keepalive-before-ft` |

## Usage and logging

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`611ebba4`](https://github.com/router-for-me/CLIProxyAPI/commit/611ebba4) | refactor(executor): unify model reporting and replay logs | no | to port | M (+52 −144) | shared model observation; log wording |
| [`37fe2608`](https://github.com/router-for-me/CLIProxyAPI/commit/37fe2608) | feat(usage): propagate access provider and native key metadata to usage records | yes | to port | M (+243 −5) | access provider and native key in usage records |

## Antigravity

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`d7c3c0aa`](https://github.com/router-for-me/CLIProxyAPI/commit/d7c3c0aa) | feat(antigravity): trace upstream connection reuse in debug logs | yes | not applicable | M (+91 −9) | connection reuse debug trace; Antigravity is not ported |
| [`e3abd9ae`](https://github.com/router-for-me/CLIProxyAPI/commit/e3abd9ae) | fix(antigravity): replace retired Claude 4.6 models with 5.5 | yes | not applicable | S (+8 −8) | Claude 4.6 models replaced with 5.5; Antigravity is not ported |
| [`c575291a`](https://github.com/router-for-me/CLIProxyAPI/commit/c575291a) | fix(antigravity): align Claude 5.5 token limits with upstream catalog | yes | not applicable | S (+4 −4) | Claude 5.5 token limits; Antigravity is not ported |
| [`6b037f61`](https://github.com/router-for-me/CLIProxyAPI/commit/6b037f61) | fix(antigravity): use actual Claude 5.5 high model IDs | yes | not applicable | S (+8 −8) | Claude 5.5 high model IDs; Antigravity is not ported |
| [`8a945b3f`](https://github.com/router-for-me/CLIProxyAPI/commit/8a945b3f) | fix(antigravity): restore Claude 4.6 model definitions | yes | not applicable | S (+48 −0) | Claude 4.6 models restored; Antigravity is not ported |
| [`9b9c2bdf`](https://github.com/router-for-me/CLIProxyAPI/commit/9b9c2bdf) | feat(antigravity): support Claude 5.5 CAQS thinking signature replay | yes | not applicable | M (+136 −22) | Claude 5.5 CAQS signature replay; Antigravity is not ported |
| [`174248ca`](https://github.com/router-for-me/CLIProxyAPI/commit/174248ca) | fix(antigravity): reject ambiguous CAQS signature wrappers | yes | not applicable | S (+34 −8) | ambiguous CAQS wrappers rejected; Antigravity is not ported |
| [`7e857c37`](https://github.com/router-for-me/CLIProxyAPI/commit/7e857c37) | feat(antigravity): refresh per-account model entitlements safely | yes | not applicable | L (+749 −234) | per-account model entitlements; Antigravity is not ported |
| [`9521fe29`](https://github.com/router-for-me/CLIProxyAPI/commit/9521fe29) | fix(antigravity): preserve model lifecycle and cooldown state | yes | not applicable | S (+21 −9) | model lifecycle and cooldown state; Antigravity is not ported |
| [`ad381435`](https://github.com/router-for-me/CLIProxyAPI/commit/ad381435) | fix(antigravity): coalesce probes and respect plugin model ownership | yes | not applicable | M (+144 −99) | coalesced probes, plugin model ownership; Antigravity is not ported |
| [`cbc4cd55`](https://github.com/router-for-me/CLIProxyAPI/commit/cbc4cd55) | feat(interactions): support antigravity session continuation and model detail lookup | yes | not applicable | L (+299 −14) | Interactions session continuation; Antigravity is not ported |
| [`4e4dede4`](https://github.com/router-for-me/CLIProxyAPI/commit/4e4dede4) | fix(antigravity): surface backend errors and support multi-line stream payloads | yes | not applicable | S (+33 −0) | backend errors, multi-line stream payloads; Antigravity is not ported |
| [`a4acc9f7`](https://github.com/router-for-me/CLIProxyAPI/commit/a4acc9f7) | fix(antigravity): treat client disconnect after terminal stream chunk as success | yes | not applicable | M (+52 −0) | client disconnect after the terminal chunk; Antigravity is not ported |
| [`17dc1b81`](https://github.com/router-for-me/CLIProxyAPI/commit/17dc1b81) | feat(antigravity): enhance context hash rejection logging and introduce replay phases | yes | not applicable | M (+47 −26) | context hash rejection logging, replay phases; Antigravity is not ported |
| [`40f4df07`](https://github.com/router-for-me/CLIProxyAPI/commit/40f4df07) | feat(antigravity): enhance context hash rejection logging and batch retry logic | yes | not applicable | M (+71 −23) | context hash rejection batch retry; Antigravity is not ported |
| [`ab151f5d`](https://github.com/router-for-me/CLIProxyAPI/commit/ab151f5d) | fix(antigravity): include thoughts token count in openai completion tokens | yes | not applicable | S (+1 −1) | thoughts tokens in completion tokens; Antigravity is not ported |
| [`dfe0a0ef`](https://github.com/router-for-me/CLIProxyAPI/commit/dfe0a0ef) | fix(antigravity): omit tool_calls from default streaming delta template | yes | not applicable | S (+1 −1) | no `tool_calls` in the default delta template; Antigravity is not ported |
| [`a2718314`](https://github.com/router-for-me/CLIProxyAPI/commit/a2718314) | fix(antigravity): drop trailing thought-only model turns | yes | not applicable | M (+51 −1) | trailing thought-only model turns dropped; Antigravity is not ported |
| [`091ff27f`](https://github.com/router-for-me/CLIProxyAPI/commit/091ff27f) | feat(antigravity): persist model capability hints in auth metadata | yes | not applicable | L (+801 −77) | model capability hints in auth metadata; Antigravity is not ported |

## Docs and assets

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`980c3bc3`](https://github.com/router-for-me/CLIProxyAPI/commit/980c3bc3) | docs(readme): add panel4cliproxyapi to community projects | no | nothing to port | S (+8 −0) |  |
| [`6d06098d`](https://github.com/router-for-me/CLIProxyAPI/commit/6d06098d) | docs(readme): add cliproxy-rs to More choices | no | nothing to port | S (+12 −0) |  |
| [`8ef43e4d`](https://github.com/router-for-me/CLIProxyAPI/commit/8ef43e4d) | fix(readme): remove AxisNow sponsorship details from README files | no | nothing to port | S (+0 −12) |  |
| [`dfd7c638`](https://github.com/router-for-me/CLIProxyAPI/commit/dfd7c638) | docs(readme): add Oh-My-CPA to usage statistics | no | nothing to port | S (+12 −0) |  |
| [`d2e3a77e`](https://github.com/router-for-me/CLIProxyAPI/commit/d2e3a77e) | docs(readme): add japanese translation for panel4cliproxyapi | no | nothing to port | S (+4 −0) |  |
| [`54946fa3`](https://github.com/router-for-me/CLIProxyAPI/commit/54946fa3) | docs(readme): remove full QQ group in README_CN | no | nothing to port | S (+1 −1) |  |
| [`5cff76dd`](https://github.com/router-for-me/CLIProxyAPI/commit/5cff76dd) | docs: refresh CPA-Manager-Plus descriptions in multilingual READMEs | no | nothing to port | S (+3 −3) |  |
| [`3de4e248`](https://github.com/router-for-me/CLIProxyAPI/commit/3de4e248) | docs(readme): remove aiberm sponsorship | no | nothing to port | S (+0 −12) |  |
| [`0cb6ecfd`](https://github.com/router-for-me/CLIProxyAPI/commit/0cb6ecfd) | feat(sponsorship): add Modelflare sponsorship details to README files | no | nothing to port | S (+12 −0) |  |
| [`f1d4a94e`](https://github.com/router-for-me/CLIProxyAPI/commit/f1d4a94e) | fix(assets): update Modelflare logo image | no | nothing to port | - |  |
| [`67870b67`](https://github.com/router-for-me/CLIProxyAPI/commit/67870b67) | docs: add sub2api to "Who is with us?" | no | nothing to port | S (+12 −0) |  |

## Go tests only

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`30aa1f12`](https://github.com/router-for-me/CLIProxyAPI/commit/30aa1f12) | test(executor): relax connection reuse bounds in transport tests | no | nothing to port | tests only | transport test bounds |
| [`da5f109f`](https://github.com/router-for-me/CLIProxyAPI/commit/da5f109f) | test(cliproxy): fix model catalog restore configuration in tests | no | nothing to port | tests only | catalog restore in tests |
| [`ce518c9b`](https://github.com/router-for-me/CLIProxyAPI/commit/ce518c9b) | test(claude): cover a reload between tool_search fallback and translation | no | nothing to port | tests only | tool_search test |
| [`77cf88b0`](https://github.com/router-for-me/CLIProxyAPI/commit/77cf88b0) | test(claude): pin executor clock in tool_search disabled golden | no | nothing to port | tests only | tool_search golden clock |

## Merge commits

| Commit | Subject | User-visible | This PR | Size | Notes |
|---|---|---|---|---|---|
| [`e2bff010`](https://github.com/router-for-me/CLIProxyAPI/commit/e2bff010) | Merge pull request #6285 from sususu98/feat/antigravity-connection-debug-trace | no | nothing to port | - | no change of its own |
| [`0594a632`](https://github.com/router-for-me/CLIProxyAPI/commit/0594a632) | Merge pull request #6318 from router-for-me/feat/antigravity-caqs-signature-replay | no | nothing to port | - | no change of its own |
| [`38a089ea`](https://github.com/router-for-me/CLIProxyAPI/commit/38a089ea) | Merge pull request #6330 from sususu98/feat/antigravity-account-model-entitlements | no | nothing to port | - | no change of its own |
| [`13da3cee`](https://github.com/router-for-me/CLIProxyAPI/commit/13da3cee) | Merge pull request #6373 from WizisCool/docs/add-oh-my-cpa-usage-statistics | no | nothing to port | - | no change of its own |
| [`3e83083b`](https://github.com/router-for-me/CLIProxyAPI/commit/3e83083b) | Merge pull request #6386 from router-for-me/logs | no | nothing to port | - | no change of its own |
| [`30700d44`](https://github.com/router-for-me/CLIProxyAPI/commit/30700d44) | Merge pull request #6391 from router-for-me/home | no | nothing to port | - | no change of its own |
| [`7dc25a7e`](https://github.com/router-for-me/CLIProxyAPI/commit/7dc25a7e) | Merge pull request #6355 from vayungodara/docs/add-cliproxy-rs | no | nothing to port | - | no change of its own |
| [`abe54e85`](https://github.com/router-for-me/CLIProxyAPI/commit/abe54e85) | Merge pull request #6112 from yaanlaan/docs/add-panel4cliproxyapi | no | nothing to port | - | no change of its own |
| [`061001f8`](https://github.com/router-for-me/CLIProxyAPI/commit/061001f8) | Merge PR #6418: feat(xai): expose Grok TTS on /v1/audio/speech | no | nothing to port | - | no change of its own |
| [`f3703e82`](https://github.com/router-for-me/CLIProxyAPI/commit/f3703e82) | Merge pull request #6444 from router-for-me/feat/server-github-token | no | nothing to port | - | no change of its own |
| [`7cd8f7ba`](https://github.com/router-for-me/CLIProxyAPI/commit/7cd8f7ba) | Merge pull request #6441 from soilSpoon/fix/attachment-parts | no | nothing to port | - | no change of its own |
| [`298649f6`](https://github.com/router-for-me/CLIProxyAPI/commit/298649f6) | Merge pull request #6446 from jellyfish-p/feat/grok-cli-dynamic-version | no | nothing to port | - | no change of its own |
| [`84740392`](https://github.com/router-for-me/CLIProxyAPI/commit/84740392) | Merge pull request #6488 from router-for-me/fix/openai-compat-stream-ttft | no | nothing to port | - | no change of its own |
| [`9612b7ac`](https://github.com/router-for-me/CLIProxyAPI/commit/9612b7ac) | Merge pull request #6490 from router-for-me/home | no | nothing to port | - | no change of its own |
| [`763b41d4`](https://github.com/router-for-me/CLIProxyAPI/commit/763b41d4) | Merge pull request #6499 from seakee/docs/update-cpamp-description | no | nothing to port | - | no change of its own |
| [`d739485d`](https://github.com/router-for-me/CLIProxyAPI/commit/d739485d) | Merge pull request #6525 from router-for-me/models | no | nothing to port | - | no change of its own |
| [`82912f24`](https://github.com/router-for-me/CLIProxyAPI/commit/82912f24) | Merge pull request #6520 from gitrvc-hub/fix/claude-responses-tool-search | no | nothing to port | - | no change of its own |
| [`89006da1`](https://github.com/router-for-me/CLIProxyAPI/commit/89006da1) | Merge branch 'pr6520' into dev | no | nothing to port | - | no change of its own |
| [`696bbaee`](https://github.com/router-for-me/CLIProxyAPI/commit/696bbaee) | Merge pull request #6547 from router-for-me/mgmt | no | nothing to port | - | no change of its own |
| [`a27799ec`](https://github.com/router-for-me/CLIProxyAPI/commit/a27799ec) | Merge pull request #6548 from gracjankubicki/fix/tool-search-golden-date | no | nothing to port | - | no change of its own |
| [`bf934162`](https://github.com/router-for-me/CLIProxyAPI/commit/bf934162) | Merge pull request #6557 from devbymak/docs/add-sub2api | no | nothing to port | - | no change of its own |
