//! The replay cache's snapshot rules (Go antigravity_reasoning_replay_cache_test.go,
//! in-process cases) and the purge and capacity behavior, each on a private store.

use super::*;

fn item(signature: &str) -> Vec<u8> {
    format!(r#"{{"type":"thought_signature","contentIndex":1,"partIndex":0,"thoughtSignature":"{signature}"}}"#)
        .into_bytes()
}

fn items(signatures: &[&str]) -> Vec<Vec<u8>> {
    normalize(&signatures.iter().map(|s| item(s)).collect::<Vec<_>>()).unwrap()
}

/// [`replay_get_with_snapshot`] on a private store.
fn read(c: &mut Caches, key: String, at: Instant) -> (Option<Vec<Vec<u8>>>, ReplaySnapshot) {
    with_items(c.get(key, at))
}

fn min(n: u64) -> Duration {
    Duration::from_secs(60 * n)
}

#[test]
fn normalizes_signatures_and_native_calls() {
    let raw = vec![
        br#"{"type":"thought_signature","thoughtSignature":" sig-aaaaaaaaaaaaaaaa ","targetKind":"text","targetHash":"h","extra":1}"#.to_vec(),
        br#"{"type":"thought_signature","thoughtSignature":"short"}"#.to_vec(),
        br#"{"type":"other","thoughtSignature":"sig-bbbbbbbbbbbbbbbb"}"#.to_vec(),
        br#"{"type":"function_call_part","functionCall":{"id":"c1","name":"f","args":{"b":1,"a":[2]}},"thoughtSignature":"skip_thought_signature_validator","contentIndex":3,"targetOccurrence":-1,"contextHash":" ctx "}"#.to_vec(),
        br#"{"type":"function_call_part","name":"g","args":"{\"x\":1}","call_id":" c2 ","thoughtSignature":"sig","partIndex":2,"targetOccurrence":0,"targetKind":"text"}"#.to_vec(),
        br#"{"type":"function_call_part","name":"no-args"}"#.to_vec(),
    ];
    assert_eq!(
        normalize(&raw).unwrap(),
        vec![
            br#"{"type":"thought_signature","thoughtSignature":"sig-aaaaaaaaaaaaaaaa","targetKind":"text","targetHash":"h"}"#.to_vec(),
            br#"{"type":"function_call_part","call_id":"c1","name":"f","args":{"b":1,"a":[2]},"contentIndex":3,"contextHash":"ctx"}"#.to_vec(),
            br#"{"type":"function_call_part","call_id":"c2","name":"g","args":"{\"x\":1}","thoughtSignature":"sig","partIndex":2,"targetOccurrence":0}"#.to_vec(),
        ]
    );
    assert!(normalize(&raw[1..3]).is_none());
}

/// TestAntigravityReasoningReplayConditionalMutationRejectsStaleLocalSnapshot.
#[test]
fn stale_snapshots_neither_replace_nor_delete() {
    let (mut c, t0) = (Caches::default(), Instant::now());
    c.put("k".into(), items(&["old-local-signature-123456"]), t0);
    let (_, snapshot) = read(&mut c, "k".into(), t0);
    c.put("k".into(), items(&["new-local-signature-123456"]), t0);
    assert!(!c.replace("k".into(), &snapshot, items(&["stale-local-signature-123456"]), t0));
    assert!(!c.delete_if_unchanged("k".into(), &snapshot, t0));
    assert_eq!(
        read(&mut c, "k".into(), t0).0.unwrap(),
        items(&["new-local-signature-123456"])
    );
}

/// TestAntigravityReasoningReplayNonPrefixReplaceRotatesLocalBranch and
/// TestAntigravityReasoningReplayConditionalReplaceAcceptsDescendantLocalChain.
#[test]
fn descendants_extend_a_branch_and_resets_rotate_it() {
    let (mut c, t0) = (Caches::default(), Instant::now());
    c.put("k".into(), items(&["non-prefix-old-signature-123456"]), t0);
    let (_, first) = read(&mut c, "k".into(), t0);
    let (_, stale) = read(&mut c, "k".into(), t0);
    let new = items(&["non-prefix-new-signature-123456"]);
    assert!(c.replace("k".into(), &first, new.clone(), t0));
    let mut latest = new.clone();
    latest.extend(items(&["non-prefix-latest-signature-123456"]));
    assert!(
        !c.replace("k".into(), &stale, latest, t0),
        "stale descendant crossed a reset"
    );

    let (prefix, middle, last) = (
        "descendant-prefix-signature-123456",
        "descendant-middle-signature-123456",
        "descendant-latest-signature-123456",
    );
    c.put("d".into(), items(&[prefix]), t0);
    let (_, stale) = read(&mut c, "d".into(), t0);
    let (_, first) = read(&mut c, "d".into(), t0);
    assert!(c.replace("d".into(), &first, items(&[prefix, middle]), t0));
    assert!(c.replace("d".into(), &stale, items(&[prefix, middle, last]), t0));
    assert_eq!(read(&mut c, "d".into(), t0).0.unwrap().len(), 3);
}

/// TestAntigravityReasoningReplayDescendantMergeRejectsResetBranchABA.
#[test]
fn a_reset_branch_rejects_old_descendants() {
    let (mut c, t0) = (Caches::default(), Instant::now());
    let (prefix, middle) = ("reset-prefix-signature-123456", "reset-middle-signature-123456");
    c.put("k".into(), items(&[prefix]), t0);
    let (_, stale) = read(&mut c, "k".into(), t0);
    let (_, first) = read(&mut c, "k".into(), t0);
    assert!(c.replace("k".into(), &first, items(&[prefix, middle]), t0));
    let (_, current) = read(&mut c, "k".into(), t0);
    assert!(c.delete_if_unchanged("k".into(), &current, t0));
    let (_, reset) = read(&mut c, "k".into(), t0);
    assert!(c.replace("k".into(), &reset, items(&[prefix]), t0));
    assert!(!c.replace("k".into(), &stale, items(&[prefix, "reset-stale-signature-123456"]), t0));
}

/// TestAntigravityReasoningReplayConditionalDeleteTombstoneBlocksStaleFirstWriter,
/// ...EvictedTombstoneStillBlocksStaleFirstWriter and
/// ...UnrelatedEvictionDoesNotBlockAbsentSnapshot.
#[test]
fn absent_snapshots_are_fenced() {
    let (mut c, t0) = (Caches::default(), Instant::now());
    let (none, stale) = read(&mut c, "k".into(), t0);
    assert!(none.is_none());
    let (_, clear) = read(&mut c, "k".into(), t0);
    assert!(c.delete_if_unchanged("k".into(), &clear, t0));
    assert!(!c.replace("k".into(), &stale, items(&["stale-first-writer-signature-123456"]), t0));

    let (mut c, t0) = (Caches::default(), Instant::now());
    let (_, stale) = read(&mut c, "k".into(), t0);
    let (_, clear) = read(&mut c, "k".into(), t0);
    assert!(c.delete_if_unchanged("k".into(), &clear, t0));
    c.evict_oldest();
    assert!(c.replay.is_empty());
    assert!(!c.replace(
        "k".into(),
        &stale,
        items(&["evicted-stale-first-writer-signature-123456"]),
        t0
    ));

    let (mut c, t0) = (Caches::default(), Instant::now());
    c.put("older".into(), items(&["evicted-live-signature-123456"]), t0);
    let (_, snapshot) = read(&mut c, "untouched".into(), t0 + min(1));
    // Only the older entry goes (the batch is larger than the map here).
    c.replay.remove("older");
    c.eviction_epoch += 1;
    assert!(c.replace(
        "untouched".into(),
        &snapshot,
        items(&["first-write-after-unrelated-eviction-123456"]),
        t0
    ));
}

#[test]
fn misses_reserve_tombstones_that_count_toward_capacity() {
    let (mut c, t0) = (Caches::default(), Instant::now());
    c.put("a".into(), vec![b"item".to_vec()], t0);
    // 10,239 distinct misses fill the map; the next one evicts the oldest 128, `a` first.
    for i in 1..MAX_ENTRIES {
        assert!(
            read(&mut c, format!("absent-{i}"), t0 + Duration::from_micros(i as u64))
                .0
                .is_none()
        );
    }
    assert_eq!(c.replay.len(), MAX_ENTRIES);
    assert_eq!(
        read(&mut c, "a".into(), t0 + min(1)).0.as_deref(),
        Some(&[b"item".to_vec()][..])
    );
    read(&mut c, "absent-last".into(), t0 + min(2));
    assert_eq!(c.replay.len(), MAX_ENTRIES - EVICT_BATCH + 1);
    assert!(c.replay.contains_key("a"), "refreshed by the hit");
    assert!(!c.replay.contains_key("absent-1"));
    // A tombstone hit stays a miss and refreshes the tombstone.
    assert!(read(&mut c, "absent-last".into(), t0 + min(3)).0.is_none());
    assert_eq!(c.replay["absent-last"].at, t0 + min(3));
}

/// TestAntigravityReasoningReplayLocalTombstonesStayWithinEntryBound.
#[test]
fn tombstones_stay_within_the_entry_bound() {
    let (mut c, t0) = (Caches::default(), Instant::now());
    for i in 0..=MAX_ENTRIES {
        c.delete_if_unchanged(
            format!("tombstone-{i}"),
            &ReplaySnapshot::default(),
            t0 + Duration::from_micros(i as u64),
        );
    }
    assert!(c.replay.len() <= MAX_ENTRIES);
}

#[test]
fn expired_entries_purge_at_go_tick_times() {
    let (mut c, t0) = (Caches::default(), Instant::now());
    c.put("first".into(), vec![b"i".to_vec()], t0); // starts the ticker
    c.put("x".into(), vec![b"i".to_vec()], t0 + min(5));
    // Tick 6 (t0+60m): `first` is exactly 60m old (Go deletes only past the TTL) and
    // `x` 55m old, so both stay, although `x` is 61m old now.
    read(&mut c, "y".into(), t0 + min(66));
    assert!(c.replay.contains_key("x"));
    assert!(c.replay.contains_key("first"));
    read(&mut c, "z".into(), t0 + min(70) + Duration::from_secs(1));
    assert!(!c.replay.contains_key("x") && !c.replay.contains_key("first"));
    assert!(c.replay.contains_key("y") && c.replay.contains_key("z"));
}

#[test]
fn signature_groups_expire_and_reads_do_not_start_the_ticker() {
    let (mut c, t0) = (Caches::default(), Instant::now());
    assert!(c.cached_signature("claude", "h", t0).is_none());
    assert!(c.ticker.is_none());
    let sig = b"\xff raw bytes".to_vec();
    c.cache_signature("claude", "h".into(), &sig, t0 + min(5));
    c.cache_signature("gemini", "g".into(), &sig, t0 + min(5));
    assert_eq!(c.ticker, Some((t0 + min(5), 0)));
    assert_eq!(c.cached_signature("claude", "h", t0 + min(100)), Some(sig.clone()));
    // gemini/g is 180m old at tick 18 (t0+185m): kept; past the TTL at tick 19.
    c.cached_signature("claude", "none", t0 + min(186));
    assert!(c.signatures.contains_key("gemini"));
    c.cached_signature("claude", "none", t0 + min(196));
    assert!(!c.signatures.contains_key("gemini"), "empty group removed");
    assert!(c.signatures.contains_key("claude"), "refreshed at t0+100m");
}

/// A replay hit through [`replay_get`] (the Gemini Responses translator's text signature
/// restore, with or without Antigravity accounts) copies the entry's items once; only
/// [`replay_get_with_snapshot`] adds the snapshot's copy.
#[test]
fn a_replay_hit_copies_its_items_once() {
    let allocated = |f: &mut dyn FnMut()| {
        let before = crate::alloc_count::bytes();
        f();
        crate::alloc_count::bytes() - before
    };
    // Ten signatures of 8 KiB each: about 80 KiB of items, far more than the key and the
    // bookkeeping a read allocates.
    let signatures: Vec<String> = (0..10).map(|i| format!("{i}-{}", "s".repeat(8 << 10))).collect();
    let stored = items(&signatures.iter().map(String::as_str).collect::<Vec<_>>());
    let size: usize = stored.iter().map(Vec::len).sum();
    assert!(replay_put("gemini-3-flash", "alloc-once", &stored));
    let mut hit = None;
    let one = allocated(&mut || hit = replay_get("gemini-3-flash", "alloc-once"));
    assert_eq!(hit.as_deref(), Some(&stored[..]));
    assert!(
        one >= size && one < size + size / 4,
        "replay_get allocated {one} B for {size} B of items"
    );
    let both = allocated(&mut || hit = replay_get_with_snapshot("gemini-3-flash", "alloc-once").0);
    assert!(both >= 2 * size, "with a snapshot: {both} B");
}
