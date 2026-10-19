//! Process-wide caches of internal/cache that both translators and executors use: the
//! Antigravity reasoning replay cache (antigravity_reasoning_replay_cache.go) and the
//! thinking signature cache (signature_cache.go).
//!
//! The replay cache keeps one ledger of Gemini thought signatures and native function
//! calls per (model, session): 1 h sliding TTL, 10,240 entries including absent-key
//! tombstones, the oldest 128 evicted at capacity. Reads return a snapshot (revision,
//! branch, eviction epoch) so a request publishes its new ledger only when nothing
//! changed it meanwhile or it extends the same branch (`...IfUnchanged`). Go's 10-minute
//! background purge runs lazily at the nominal tick times on the next cache access
//! (`Caches::sweep`).
// ponytail: Go's Home KV backends of both caches are not ported (Rust Home mode keeps
// these stores in process). Branch identifiers come from a process counter instead of
// crypto/rand: they are only compared within this process.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use sha2::{Digest, Sha256};

use crate::gostr::trim_space;
use crate::json::{self as gj, Kind};

const TTL: Duration = Duration::from_secs(3600);
const MAX_ENTRIES: usize = 10240;
const EVICT_BATCH: usize = 128;
const MIN_SIGNATURE_LEN: usize = 16;
/// `AntigravityReasoningReplayCacheMaxItemsPerEntry`.
pub const MAX_REPLAY_ITEMS: usize = 4096;
/// `AntigravityReasoningReplayCacheMaxBytesPerEntry`.
pub const MAX_REPLAY_BYTES: usize = 16 << 20;
/// CacheCleanupInterval: Go's purge ticker, started by the first write to the signature
/// cache or the first access to the replay cache (`cacheCleanupOnce`).
const CLEANUP_INTERVAL: Duration = Duration::from_secs(600);

/// One replay entry (`antigravityReasoningReplayEntry`); a tombstone is `deleted`.
struct Entry {
    items: Vec<Vec<u8>>,
    at: Instant,
    revision: u64,
    branch: String,
    deleted: bool,
}

/// The replay state one request read (`AntigravityReasoningReplaySnapshot`).
#[derive(Debug, Clone, Default)]
pub struct ReplaySnapshot {
    items: Vec<Vec<u8>>,
    loaded: bool,
    found: bool,
    revision: u64,
    branch: String,
    eviction_epoch: u64,
}

/// Both in-process stores and Go's shared purge ticker, behind one lock.
#[derive(Default)]
struct Caches {
    replay: HashMap<String, Entry>,
    next_revision: u64,
    eviction_epoch: u64,
    next_branch: u64,
    /// model group -> text hash -> (signature, last use).
    signatures: HashMap<String, HashMap<String, (Vec<u8>, Instant)>>,
    /// (ticker start, ticks already purged).
    ticker: Option<(Instant, u32)>,
}

static CACHES: LazyLock<Mutex<Caches>> = LazyLock::new(Default::default);

fn caches() -> std::sync::MutexGuard<'static, Caches> {
    CACHES.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Caches {
    /// purgeExpiredCaches for every ticker tick that passed since the last access. No
    /// access happens in between, so purging once at the latest missed tick time removes
    /// exactly what Go's ticks would have removed by now. `start`: whether this access
    /// starts Go's ticker (signature reads do not).
    fn sweep(&mut self, now: Instant, start: bool) {
        if self.ticker.is_none() && !start {
            return;
        }
        let (begin, done) = self.ticker.get_or_insert((now, 0));
        let ticks = (now.duration_since(*begin).as_secs() / CLEANUP_INTERVAL.as_secs()) as u32;
        if ticks <= *done {
            return;
        }
        *done = ticks;
        let tick_at = *begin + CLEANUP_INTERVAL * ticks;
        self.signatures.retain(|_, entries| {
            entries.retain(|_, (_, at)| tick_at.saturating_duration_since(*at) <= SIGNATURE_TTL);
            !entries.is_empty()
        });
        let before = self.replay.len();
        self.replay
            .retain(|_, e| tick_at.saturating_duration_since(e.at) <= TTL);
        self.eviction_epoch += (before - self.replay.len()) as u64;
    }

    /// `newAntigravityReasoningReplayGeneration`.
    fn branch(&mut self) -> String {
        self.next_branch += 1;
        format!("{:032x}", self.next_branch)
    }

    /// evictOldestAntigravityReasoningReplayEntries.
    fn evict_oldest(&mut self) {
        let mut oldest: Vec<(Instant, String)> = self.replay.iter().map(|(k, e)| (e.at, k.clone())).collect();
        oldest.sort();
        for (_, k) in oldest.into_iter().take(EVICT_BATCH) {
            self.eviction_epoch += 1;
            self.replay.remove(&k);
        }
    }

    /// Stores `items` (or a tombstone) under a new revision, then evicts at capacity.
    fn store(&mut self, key: String, items: Vec<Vec<u8>>, branch: Option<String>, now: Instant) {
        self.next_revision += 1;
        let deleted = items.is_empty();
        let branch = branch.unwrap_or_else(|| self.branch());
        let entry = Entry {
            items,
            at: now,
            revision: self.next_revision,
            branch,
            deleted,
        };
        self.replay.insert(key, entry);
        if self.replay.len() > MAX_ENTRIES {
            self.evict_oldest();
        }
    }

    /// reserveAntigravityReasoningReplayAbsentLocked: a miss is fenced with a tombstone,
    /// so evicting an unrelated key cannot invalidate it.
    fn reserve_absent(&mut self, key: String, now: Instant) -> ReplaySnapshot {
        if self.replay.len() >= MAX_ENTRIES {
            self.evict_oldest();
        }
        self.next_revision += 1;
        let branch = self.branch();
        let revision = self.next_revision;
        self.replay.insert(
            key,
            Entry {
                items: Vec::new(),
                at: now,
                revision,
                branch: branch.clone(),
                deleted: true,
            },
        );
        ReplaySnapshot {
            loaded: true,
            found: true,
            revision,
            branch,
            eviction_epoch: self.eviction_epoch,
            items: Vec::new(),
        }
    }

    /// GetAntigravityReasoningReplayItemsWithSnapshotRequired (in-process): a hit
    /// refreshes the entry (tombstones included); a miss or an expired entry reserves a
    /// tombstone that counts toward capacity. The snapshot carries no items: the one
    /// copy made under the lock is the returned items, and a caller that needs both
    /// copies them after the lock is released ([`replay_get_with_snapshot`]).
    fn get(&mut self, key: String, now: Instant) -> (Option<Vec<Vec<u8>>>, ReplaySnapshot) {
        self.sweep(now, true);
        match self.replay.get_mut(&key) {
            None => (None, self.reserve_absent(key, now)),
            Some(entry) if now.duration_since(entry.at) > TTL => {
                self.replay.remove(&key);
                self.eviction_epoch += 1;
                (None, self.reserve_absent(key, now))
            }
            Some(entry) => {
                entry.at = now;
                let snapshot = ReplaySnapshot {
                    loaded: true,
                    found: true,
                    revision: entry.revision,
                    branch: entry.branch.clone(),
                    eviction_epoch: self.eviction_epoch,
                    items: Vec::new(),
                };
                let items = (!entry.deleted && !entry.items.is_empty()).then(|| entry.items.clone());
                (items, snapshot)
            }
        }
    }

    /// Whether the entry is exactly the state `snapshot` read.
    fn unchanged(&self, key: &str, snapshot: &ReplaySnapshot) -> bool {
        match self.replay.get(key) {
            Some(entry) => snapshot.found && entry.revision == snapshot.revision,
            None => !snapshot.found && snapshot.eviction_epoch == self.eviction_epoch,
        }
    }

    /// ReplaceAntigravityReasoningReplayItemsIfUnchanged (in-process).
    fn replace(&mut self, key: String, snapshot: &ReplaySnapshot, items: Vec<Vec<u8>>, now: Instant) -> bool {
        self.sweep(now, true);
        let matches = self.unchanged(&key, snapshot);
        let descendant = self.replay.get(&key).is_some_and(|entry| {
            !entry.deleted
                && !snapshot.branch.is_empty()
                && entry.branch == snapshot.branch
                && prefix(&entry.items, &items)
        });
        if !matches && !descendant {
            return false;
        }
        let branch = if snapshot.branch.is_empty() || (matches && !prefix(&snapshot.items, &items)) {
            None
        } else {
            Some(snapshot.branch.clone())
        };
        self.store(key, items, branch, now);
        true
    }

    /// `CacheAntigravityReasoningReplayItemsBestEffort` (in-process).
    fn put(&mut self, key: String, items: Vec<Vec<u8>>, now: Instant) {
        self.sweep(now, true);
        self.store(key, items, None, now);
    }

    /// `DeleteAntigravityReasoningReplayItemsIfUnchanged` (in-process); an unloaded
    /// snapshot deletes unconditionally.
    fn delete_if_unchanged(&mut self, key: String, snapshot: &ReplaySnapshot, now: Instant) -> bool {
        self.sweep(now, true);
        if snapshot.loaded && !self.unchanged(&key, snapshot) {
            return false;
        }
        self.store(key, Vec::new(), None, now);
        true
    }

    fn cache_signature(&mut self, group: &str, hash: String, signature: &[u8], now: Instant) {
        self.sweep(now, true);
        self.signatures
            .entry(group.to_owned())
            .or_default()
            .insert(hash, (signature.to_vec(), now));
    }

    /// A hit refreshes the entry; an expired entry is removed.
    fn cached_signature(&mut self, group: &str, hash: &str, now: Instant) -> Option<Vec<u8>> {
        self.sweep(now, false);
        let entries = self.signatures.get_mut(group)?;
        let (signature, at) = entries.get_mut(hash)?;
        if now.duration_since(*at) > SIGNATURE_TTL {
            entries.remove(hash);
            return None;
        }
        *at = now;
        Some(signature.clone())
    }
}

/// `antigravityReasoningReplayItemsPrefix`.
fn prefix(prefix: &[Vec<u8>], items: &[Vec<u8>]) -> bool {
    prefix.len() <= items.len() && prefix.iter().zip(items).all(|(a, b)| a == b)
}

fn key(model: &str, session: &str) -> Option<String> {
    let (model, session) = (model.trim(), session.trim());
    (!model.is_empty() && !session.is_empty()).then(|| ["antigravity-reasoning-replay", model, session].join("\x00"))
}

fn trimmed(r: &gj::Res<'_>) -> Vec<u8> {
    trim_space(&r.bytes()).to_vec()
}

/// The position and context fields both item kinds keep.
fn copy_positions(r: &gj::Res<'_>, out: &mut Vec<u8>, kinds: bool) {
    for field in ["contentIndex", "partIndex"] {
        let v = r.get(field);
        if v.kind == Kind::Number {
            gj::set_int(out, field, v.int());
        }
    }
    if kinds {
        let target_kind = trimmed(&r.get("targetKind"));
        if target_kind == b"text" || target_kind == b"thought" {
            gj::set_str(out, "targetKind", &target_kind);
        }
        let target_hash = trimmed(&r.get("targetHash"));
        if !target_hash.is_empty() {
            gj::set_str(out, "targetHash", &target_hash);
        }
    }
    let occurrence = r.get("targetOccurrence");
    if occurrence.kind == Kind::Number && occurrence.int() >= 0 {
        gj::set_int(out, "targetOccurrence", occurrence.int());
    }
    let context_hash = trimmed(&r.get("contextHash"));
    if !context_hash.is_empty() {
        gj::set_str(out, "contextHash", &context_hash);
    }
}

/// normalizeAntigravityReasoningReplayItem: a thought signature or a native function
/// call, reduced to the fields replay reads.
fn normalize_item(item: &[u8]) -> Option<Vec<u8>> {
    let r = gj::parse(item);
    match trimmed(&r.get("type")).as_slice() {
        b"thought_signature" => {
            let mut sig = trimmed(&r.get("thoughtSignature"));
            if sig.is_empty() {
                sig = trimmed(&r.get("thought_signature"));
            }
            if sig.is_empty() || sig == b"skip_thought_signature_validator" || sig.len() < MIN_SIGNATURE_LEN {
                return None;
            }
            let mut out = br#"{"type":"thought_signature"}"#.to_vec();
            gj::set_str(&mut out, "thoughtSignature", &sig);
            copy_positions(&r, &mut out, true);
            Some(out)
        }
        b"function_call_part" => {
            let mut call_id = trimmed(&r.get("call_id"));
            if call_id.is_empty() {
                call_id = trimmed(&r.get("id"));
            }
            let mut name = trimmed(&r.get("name"));
            let mut args = r.get("args");
            if name.is_empty() || !args.exists() {
                let call = r.get("functionCall");
                if call.exists() {
                    if call_id.is_empty() {
                        call_id = trimmed(&call.get("id"));
                    }
                    if name.is_empty() {
                        name = trimmed(&call.get("name"));
                    }
                    if !args.exists() {
                        args = call.get("args");
                    }
                }
            }
            if name.is_empty() || !args.exists() {
                return None;
            }
            let mut out = br#"{"type":"function_call_part"}"#.to_vec();
            if !call_id.is_empty() {
                gj::set_str(&mut out, "call_id", &call_id);
            }
            gj::set_str(&mut out, "name", &name);
            if args.kind == Kind::String {
                gj::set_str(&mut out, "args", args.bytes());
            } else {
                gj::set_raw(&mut out, "args", args.raw());
            }
            let sig = trimmed(&r.get("thoughtSignature"));
            if !sig.is_empty() && sig != b"skip_thought_signature_validator" {
                gj::set_str(&mut out, "thoughtSignature", &sig);
            }
            copy_positions(&r, &mut out, false);
            Some(out)
        }
        _ => None,
    }
}

/// normalizeAntigravityReasoningReplayItems: `None` when nothing is kept or a bound
/// is exceeded.
fn normalize(items: &[Vec<u8>]) -> Option<Vec<Vec<u8>>> {
    if items.len() > MAX_REPLAY_ITEMS {
        return None;
    }
    let mut out = vec![];
    let mut total = 0;
    for item in items.iter().filter_map(|i| normalize_item(i)) {
        total += item.len();
        if total > MAX_REPLAY_BYTES {
            return None;
        }
        out.push(item);
    }
    (!out.is_empty()).then_some(out)
}

/// `CacheAntigravityReasoningReplayItems`.
pub fn replay_put(model: &str, session: &str, items: &[Vec<u8>]) -> bool {
    let (Some(key), Some(items)) = (key(model, session), normalize(items)) else {
        return false;
    };
    caches().put(key, items, Instant::now());
    true
}

/// `GetAntigravityReasoningReplayItems`: one copy of the items, made under the lock.
pub fn replay_get(model: &str, session: &str) -> Option<Vec<Vec<u8>>> {
    let key = key(model, session)?;
    caches().get(key, Instant::now()).0
}

/// `GetAntigravityReasoningReplayItemsWithSnapshotRequired`. The snapshot's copy of the
/// items is made after the lock is released.
pub fn replay_get_with_snapshot(model: &str, session: &str) -> (Option<Vec<Vec<u8>>>, ReplaySnapshot) {
    let Some(key) = key(model, session) else {
        return (None, ReplaySnapshot::default());
    };
    let read = caches().get(key, Instant::now());
    with_items(read)
}

/// A read whose snapshot also holds the items read, for the branch rules of a later
/// conditional write.
fn with_items((items, mut snapshot): (Option<Vec<Vec<u8>>>, ReplaySnapshot)) -> (Option<Vec<Vec<u8>>>, ReplaySnapshot) {
    if let Some(items) = &items {
        snapshot.items.clone_from(items);
    }
    (items, snapshot)
}

/// `ReplaceAntigravityReasoningReplayItemsIfUnchanged`: publishes `items` unless a
/// newer request changed the entry off this snapshot's branch.
pub fn replay_replace_if_unchanged(
    model: &str,
    session: &str,
    snapshot: &ReplaySnapshot,
    items: &[Vec<u8>],
) -> Result<bool, String> {
    let Some(key) = key(model, session) else {
        return Ok(false);
    };
    let normalized = normalize(items).ok_or("invalid antigravity reasoning replay items")?;
    if !snapshot.loaded {
        return Ok(replay_put(model, session, &normalized));
    }
    Ok(caches().replace(key, snapshot, normalized, Instant::now()))
}

/// `DeleteAntigravityReasoningReplayItemsIfUnchanged`.
pub fn replay_delete_if_unchanged(model: &str, session: &str, snapshot: &ReplaySnapshot) -> bool {
    let Some(key) = key(model, session) else {
        return false;
    };
    caches().delete_if_unchanged(key, snapshot, Instant::now())
}

// ---------------------------------------------------------------------------------------
// Thinking signature cache (signature_cache.go)

const SIGNATURE_TTL: Duration = Duration::from_secs(3 * 3600);
const MIN_VALID_SIGNATURE_LEN: usize = 50;
const GEMINI_BYPASS: &str = "skip_thought_signature_validator";

static SIGNATURE_CACHE_ENABLED: AtomicBool = AtomicBool::new(true);
static SIGNATURE_BYPASS_STRICT: AtomicBool = AtomicBool::new(false);

/// `SetSignatureCacheEnabled` and `SetSignatureBypassStrictMode` (Go applies config
/// `antigravity-signature-cache-enabled`, default true, and
/// `antigravity-signature-bypass-strict`, default false, on load and reload).
pub fn set_signature_cache_config(enabled: bool, bypass_strict: bool) {
    SIGNATURE_CACHE_ENABLED.store(enabled, Ordering::Relaxed);
    SIGNATURE_BYPASS_STRICT.store(bypass_strict, Ordering::Relaxed);
}

/// `SignatureCacheEnabled`.
pub fn signature_cache_enabled() -> bool {
    SIGNATURE_CACHE_ENABLED.load(Ordering::Relaxed)
}

/// `SignatureBypassStrictMode`.
pub fn signature_bypass_strict() -> bool {
    SIGNATURE_BYPASS_STRICT.load(Ordering::Relaxed)
}

/// `GetModelGroup`.
pub fn model_group(model: &str) -> &str {
    if model.contains("gpt") {
        "gpt"
    } else if model.contains("claude") {
        "claude"
    } else if model.contains("gemini") {
        "gemini"
    } else {
        model
    }
}

fn text_hash(text: &[u8]) -> String {
    let sum = Sha256::digest(text);
    sum[..8].iter().map(|b| format!("{b:02x}")).collect()
}

/// `CacheSignatureBestEffort`.
pub fn cache_signature(model: &str, text: &[u8], signature: &[u8]) -> bool {
    if text.is_empty() || signature.is_empty() || signature.len() < MIN_VALID_SIGNATURE_LEN {
        return false;
    }
    caches().cache_signature(model_group(model), text_hash(text), signature, Instant::now());
    true
}

/// `GetCachedSignatureRequired`: a Gemini-group miss is the bypass sentinel.
pub fn cached_signature(model: &str, text: &[u8]) -> Vec<u8> {
    let group = model_group(model);
    let hit = if text.is_empty() {
        None
    } else {
        caches().cached_signature(group, &text_hash(text), Instant::now())
    };
    match hit {
        Some(signature) => signature,
        None if group == "gemini" => GEMINI_BYPASS.as_bytes().to_vec(),
        None => vec![],
    }
}

/// `HasValidSignature`.
pub fn has_valid_signature(model: &str, signature: &[u8]) -> bool {
    (!signature.is_empty() && signature.len() >= MIN_VALID_SIGNATURE_LEN)
        || (signature == GEMINI_BYPASS.as_bytes() && model_group(model) == "gemini")
}

#[cfg(test)]
#[path = "cache_tests.rs"]
mod tests;
