# Patches a scratch copy of CLIProxyAPI so its own replay tests record the Antigravity
# reasoning replay calls they make (see README.md next to this file).
import sys
p=sys.argv[1] + '/internal/runtime/executor/antigravity_reasoning_replay.go'
s=open(p).read()
def rename(old,new):
    global s
    assert old in s, old
    s=s.replace(old,new,1)
rename('func prepareAntigravityGeminiReasoningReplayPayload(ctx','func prepareAntigravityGeminiReasoningReplayPayloadInner(ctx')
rename('func applyAntigravityReasoningReplayItems(payload','func applyAntigravityReasoningReplayItemsInner(payload')
rename('func antigravityReasoningReplayScopeFromRequest(ctx','func antigravityReasoningReplayScopeFromRequestInner(ctx')
rename('''	items, snapshot, ok, err := internalcache.GetAntigravityReasoningReplayItemsWithSnapshotRequired(ctx, scope.modelName, scope.sessionKey)
''','''	items, snapshot, ok, err := internalcache.GetAntigravityReasoningReplayItemsWithSnapshotRequired(ctx, scope.modelName, scope.sessionKey)
	recLedger = items
''')
rename('''			log.Warnf("antigravity executor: reasoning replay broke Gemini function call pairing (%v); degrading to original payload", errPairing)''','''			recInvalidated = true
			log.Warnf("antigravity executor: reasoning replay broke Gemini function call pairing (%v); degrading to original payload", errPairing)''')
rename('''	overflow                bool
	terminal                bool
}''','''	overflow                bool
	terminal                bool
	rec                     *recAccumulator
}''')
rename('''func (a *antigravityReasoningReplayAccumulator) observeResponsePayload(payload []byte) {
''','''func (a *antigravityReasoningReplayAccumulator) observeResponsePayload(payload []byte) {
	if a.rec != nil {
		a.rec.Observed = append(a.rec.Observed, string(payload))
	}
''')
rename('''	return &antigravityReasoningReplayAccumulator{
		scope:                   scope,''','''	acc := &antigravityReasoningReplayAccumulator{
		scope:                   scope,''')
rename('''		overflow:                len(items) > internalcache.AntigravityReasoningReplayCacheMaxItemsPerEntry || itemBytes > internalcache.AntigravityReasoningReplayCacheMaxBytesPerEntry,
	}
}''','''		overflow:                len(items) > internalcache.AntigravityReasoningReplayCacheMaxItemsPerEntry || itemBytes > internalcache.AntigravityReasoningReplayCacheMaxBytesPerEntry,
	}
	acc.rec = &recAccumulator{Test: currentTest(), Request: string(requestPayload), acc: acc}
	recAccumulators = append(recAccumulators, acc.rec)
	return acc
}''')
open(p,'w').write(s)

