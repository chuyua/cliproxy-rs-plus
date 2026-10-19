# Antigravity reasoning replay recorder

`tests/fixtures/antigravity_replay_go.json` records every call that Go's own
reasoning replay tests make to `prepareAntigravityGeminiReasoningReplayPayload`,
`applyAntigravityReasoningReplayItems` (including the randomized differentials),
`antigravityReasoningReplayScopeFromRequest` and the response accumulator, with
Go's results. `zy_replay_extra_test.go` adds edge cases Go's tests do not reach
(recased roles, stale part indexes, kind and hash disagreements, repeated stream
chunks); it asserts nothing. `antigravity_replay_tests.rs` replays each call on the Rust port.

To regenerate it from a scratch copy of the pinned Go tree (never the reference
checkout itself), with external network denied:

    tar -C CLIProxyAPI --exclude=.git -cf - . | tar -C /tmp/cpa-rec -xf -
    python3 patch.py /tmp/cpa-rec
    cp antigravity_replay_rec.go zy_replay_extra_test.go zz_replay_dump_test.go /tmp/cpa-rec/internal/runtime/executor/
    cd /tmp/cpa-rec && REPLAY_DUMP=/tmp/replay_dump.json \
      go test ./internal/runtime/executor/ -count=1 -run 'Replay|DegradeAntigravity|ZYReplayExtra|ZZReplayDump'

then write the dump compactly with duplicate `apply` records removed, and blank the
`test` field of records whose Go test is defined outside the replay suites
(`antigravity_reasoning_replay*_test.go` and `zy_replay_extra_test.go`), so the
fixture cites only the suites it ports.
