# v0.2.6 parity rebaseline

This review compares Rust release `d1261f2` with the existing Go reference `6fecc6e`.
It changes documentation and audit tooling, not runtime behavior or the checklist target.
The separate [upstream inventory](upstream-v8.0.35.md) covers Go through v8.0.35.

## Counts before and after

Counts are covered / partial / missing. The previous generated audit had 854 / 722 / 111;
the README's 835 / 707 / 145 was already stale.

| Milestone | Before | After |
|---|---|---|
| M1 | 53 / 65 / 0 | 66 / 52 / 0 |
| M2 | 111 / 40 / 7 | 113 / 45 / 0 |
| M3 | 208 / 125 / 17 | 216 / 134 / 0 |
| M4 | 233 / 259 / 18 | 240 / 256 / 14 |
| M5 | 180 / 102 / 21 | 201 / 102 / 0 |
| M6 | 69 / 131 / 48 | 114 / 115 / 19 |
| **All** | **854 / 722 / 111** | **950 / 704 / 33** |

Only gaps have an area in the generated audit. These counts are partial / missing.

| Area | Before | After |
|---|---|---|
| claude | 60 / 0 | 48 / 0 |
| codex | 63 / 12 | 64 / 0 |
| device-providers | 20 / 1 | 20 / 0 |
| google | 28 / 12 | 36 / 0 |
| home | 22 / 5 | 25 / 0 |
| manage | 125 / 13 | 130 / 0 |
| observe | 1 / 12 | 1 / 12 |
| openai-xai | 27 / 8 | 27 / 1 |
| plugins | 108 / 39 | 86 / 18 |
| realtime | 7 / 1 | 8 / 0 |
| server | 221 / 4 | 222 / 2 |
| translate | 30 / 0 | 30 / 0 |
| tui | 10 / 4 | 7 / 0 |

## Review evidence and decisions

[rebaseline.tsv](rebaseline.tsv) records each of the 833 previous gaps, including all
627 explicit missing or partial judgments in `manual.tsv`, plus 13 previously covered
operations whose fixtures do not request the matching HTTP method. Each row records its previous
status, revised status, implementation or test evidence, and remaining gap. It is a
snapshot of this review, not another source of judgments. `manual.tsv` remains the
editable source; `status.md` is generated from it and the automatic evidence.

There are 164 status changes: 73 partial to covered, 36 missing to covered, 42
missing to partial, and 13 covered to partial. Extracted fixture evidence now requires
both the HTTP method and the complete route, not another operation on the same path.
A working subsystem does not make an entire Go test suite covered.
The retained partial judgments still lack complete case-level proof or runtime wiring.
Names in comments are search hints, not proof that every assertion was ported.

| Changed items | Reason and evidence |
|---|---|
| M6-0050, M6-0058, M6-0059, M6-0218; M4-0244; M5-0234 | The TUI exists, including standalone mode. `tui/e2e.rs` drives management actions; `golden.rs` checks Go-recorded tabs; `client.rs` ports four Go client cases; `main.rs` tests standalone startup. |
| M3-0036, M3-0050; M5-0021, M5-0235, M5-0300 | AI Studio's executor, browser sessions and `/v1/ws` exist. `aistudio_relay.rs` checks session registration, requests, streaming and ordinary disconnect cleanup. M5-0235 remains partial: `Session::close` drops the disconnect error when its pending-response buffer is full, unlike Go's eviction-and-delivery behavior; `non_stream` can return the accumulated response as success. No test covers saturated-buffer cleanup. The configurable relay attachment contract and full session-race suite also remain partial. |
| M3-0142; M2-0145..0149 | Responses steering exists. `ws_e2e.rs` and `ws_fixtures/go/zz_rsfix_ws_test.go` replay five error-recovery and four idle-disconnect scenarios for M2-0146, and all four `TestResponsesSteeringFullDuplexIntegration` scenarios for M2-0147. Those two suites are covered; incomplete authorization, helper and validation suites stay partial. |
| M1-0003; M4-0259 | Grok Shell model dispatch exists and `media_go.rs` replays catalog cases. The full Go dispatch suite remains partial. |
| M3-0056, M3-0057, M3-0154, M3-0262, M3-0263; M2-0139, M2-0150 | Images, video binding and output modalities are implemented. Codex image fixtures check a non-default base model and invalid-model fallback. Complete native fidelity, binding expiry, config parsing and named extraction suites still need tests. |
| M1-0041, M1-0043, M1-0044, M1-0048..0053, M1-0055, M1-0056, M1-0085 | Existing Claude executor scenarios set these v8 config values; Go-unit replay checks all five subagent TTL cases. Alias restoration remains partial because three of 23 cases are not replayed. M1-0084 stays partial: replaying terminal-stream exchanges does not test downstream cancellation with an open upstream or the resulting successful usage accounting. |
| M3-0005, M3-0006, M3-0010, M3-0011; M5-0032, M5-0078..0082, M5-0087, M5-0090, M5-0091, M5-0097..0100, M5-0128..0130, M5-0227, M5-0228, M5-0276 | The fresh probe recognizes all 237 primary-listener pairs. `legacy_go.rs`, `plugin_routes.rs`, `media_go.rs` and plugin quota tests exercise aliases and handlers. The scanner now reads the HTTP steps and scenarios actually replayed from their fixtures; route presence alone remains partial. |
| M4-0250, M4-0307; M3-0221 | `request_logging.rs` captures inbound and upstream bytes, buffered and streaming responses, WebSockets and Home delivery. Go middleware and upstream goldens run. Concurrent filename collisions and the exact Go metadata suite remain partial. |
| M3-0246; M4-0268, M4-0269 | `codex_response.rs` converts Grok keepalives. The specific Go executor and handler cases are not reproduced, so these become partial, not covered. |
| M6-0070, M6-0071, M6-0074, M6-0084..0087, M6-0091, M6-0093, M6-0103, M6-0104, M6-0106, M6-0107, M6-0109, M6-0117..0119, M6-0123, M6-0124, M6-0129, M6-0130, M6-0133..0142, M6-0147..0154 | Plugin frontend authentication, routing, request/response interceptors, CLI, quota, HTTP, stream and auth callbacks have integration or Go-host fixtures. `plugin_wiring.rs`, `plugin_cli.rs`, `plugin_routes.rs`, `host_http.rs`, `go_host.rs` and `plugin_host_auth.rs` check them. M6-0149 (`host.log`) covers only success/error RPC envelopes, not logged fields or request-ID restoration. |
| M4-0282, M4-0283, M4-0480..0482; M5-0284..0290; M6-0210 | Old manual overrides hid complete Go-case matches for Home limits/registry and duplex execution. Home model-info binding is also tested in `cliproxy/src/home.rs`. |
| M3-0209, M3-0227, M3-0229, M3-0232, M3-0240, M3-0241, M3-0342, M3-0344 | WebRTC media relay and Antigravity compaction, refresh, connection reuse and dispatch behavior exist. Their full Go suites are not reproduced. The relay requires the optional `media-relay` feature and still differs from Pion. |
| M6-0155, M6-0162, M6-0173, M6-0188, M6-0194, M6-0195, M6-0214..0216, M6-0225, M6-0230, M6-0231, M6-0237, M6-0247, M6-0248 | Plugin OAuth persistence, config edits, Home logs/dispatch, plugin termination/errors and store validation have implementations and other tests. Their named Go suites remain incomplete. |

Notes also correct stale descriptions without changing status: filesystem events are wired;
commercial mode suppresses capture; RESP subscribers and built-in executor usage hooks
exist; plugin login IDs work; the Codex catalog updater starts lazily outside Home.
Plugin auth/model registration, scheduler selection, translation, thinking and WebSocket
observers still lack server integration. Plugin-executor usage publication, Home plugin
synchronization, Windows plugin loading, host model callbacks and pprof remain gaps.
The byte-only translator goldens do not prove buffer reuse, so those suites stay partial.

## Verification

The following checks run without provider credentials or live provider traffic:

- `CARGO_BUILD_JOBS=2 nice -n 19 cargo build -p cliproxy`: passed.
- `unshare -Urn sh -c 'ip link set lo up; python3 docs/parity-audit/probe.py "$PWD/target/debug/cliproxy"'`: 237/237 routed. Each server has a private temporary auth directory.
- `python3 -m unittest discover -s docs/parity-audit -p 'test_*.py'`: three tests pass, covering shared README/PARITY counts, method-specific fixture evidence, and unchanged pages when the README summary is missing or duplicated.
- Targeted Rust tests pass in a network-denied namespace: `cpa-home --lib` (120), `cliproxy tui::` (24), standalone startup/address tests (2), plugin wiring (1), plugin CLI (1), `cpa-plugin` host HTTP (7) and store (1), `cpa-server` AI Studio (2), media Go replay (4), duplex dispatch (1), plugin host auth (2), plugin routes (1), request logging (17), steering (25), and `cpa-exec` Claude Go-unit replay (22 passed, one pre-existing cancellation test ignored) and Codex images (1).
- `audit.py` is rerun in full. A second run must leave the generated pages unchanged; a milestone-filtered run must not overwrite their full totals.
- The upstream table has exactly the 288 unique commits in `6fecc6e..v8.0.35`: 19 ported, 205 to port, zero not applicable, 61 nothing to port, and three deliberate differences. Of the 19 formerly excluded Antigravity commits, seven are ported and 12 remain to port.

This is an audit of existing tests, not a live-provider compatibility claim. The Go
checkout is read-only. Existing Go goldens remain pinned to the versions from which
they were recorded.
