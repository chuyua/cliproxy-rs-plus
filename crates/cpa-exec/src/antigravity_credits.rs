//! Antigravity AI credits (antigravity_executor_credits.go and
//! sdk/cliproxy/auth/antigravity_credits.go).
//!
//! With `quota-exceeded.antigravity-credits` (v8
//! `oauth.providers.antigravity.antigravity-credits`) on, a Claude-model request whose
//! normal rounds all failed on quota is retried once per Antigravity credential with
//! Google One AI credits enabled (`enabledCreditTypes`). The dispatcher picks the
//! credentials from the per-credential hints kept here, which the executor learns from
//! `loadCodeAssist` and from credits-exhausted rejections.
// ponytail: Go's Home KV copies of hints and balances, and the registration-epoch
// check on hint refreshes, are not ported (the Rust Home client has no credits use).

use std::collections::HashMap;
use std::sync::{LazyLock, Mutex, PoisonError};
use std::time::{Duration, Instant};

use cpa_common::gostr::trim_space;
use cpa_common::json as gj;
use cpa_core::config::Config;
use cpa_core::credential::Credential;

/// The runtime-only attribute the credits fallback sets on its copy of a credential
/// (Go `WithAntigravityCredits` on the attempt's context). Never persisted.
pub const REQUESTED_ATTRIBUTE: &str = "antigravity_credits";

/// `antigravityCreditsHintRefreshInterval`: hint lookups per credential.
const REFRESH_INTERVAL: Duration = Duration::from_secs(600);
/// `antigravityCreditsHintRefreshTimeout`: a request-triggered lookup's budget.
pub(crate) const REQUEST_REFRESH_TIMEOUT: Duration = Duration::from_secs(5);

/// The latest known credits state of one credential (`AntigravityCreditsHint`).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Hint {
    pub known: bool,
    pub available: bool,
    pub credit_amount: f64,
    pub min_credit_amount: f64,
    pub paid_tier_id: String,
}

static HINTS: LazyLock<Mutex<HashMap<String, Hint>>> = LazyLock::new(Mutex::default);
/// When each credential's last hint lookup started, and whether one is running.
static REFRESHES: LazyLock<Mutex<HashMap<String, (Instant, bool)>>> = LazyLock::new(Mutex::default);

/// `GetAntigravityCreditsHint`.
pub fn hint(id: &str) -> Option<Hint> {
    let id = id.trim();
    if id.is_empty() {
        return None;
    }
    HINTS.lock().unwrap_or_else(PoisonError::into_inner).get(id).cloned()
}

/// `SetAntigravityCreditsHint`.
pub fn set_hint(id: &str, hint: Hint) {
    let id = id.trim();
    if id.is_empty() {
        return;
    }
    HINTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(id.to_owned(), hint);
}

/// `antigravityCreditsRetryEnabled`.
pub fn enabled(cfg: &Config) -> bool {
    cfg.document
        .get("oauth")
        .and_then(|o| o.get("providers"))
        .and_then(|p| p.get("antigravity"))
        .and_then(|a| a.get("antigravity-credits"))
        .and_then(serde_yaml_ng::Value::as_bool)
        .unwrap_or(false)
}

/// The credential copy a credits attempt runs with.
pub fn request_on(credential: &Credential) -> Credential {
    let mut copy = credential.clone();
    copy.attributes.insert(REQUESTED_ATTRIBUTE.into(), "true".into());
    copy
}

/// Whether this attempt uses credits: the fallback asked for it and the config allows
/// it (`AntigravityCreditsRequested(ctx) && antigravityCreditsRetryEnabled(cfg)`).
pub(crate) fn requested(credential: &Credential, cfg: &Config) -> bool {
    credential.attributes.get(REQUESTED_ATTRIBUTE).map(String::as_str) == Some("true") && enabled(cfg)
}

/// `findAllAntigravityCreditsCandidateAuths`' order: credentials known to have credits
/// first, then those not yet known, each by ID; known-empty ones are left out.
pub fn order_candidates<'a>(ids: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let (mut known, mut unknown) = (Vec::new(), Vec::new());
    for id in ids {
        match hint(id) {
            Some(h) if h.known => {
                if h.available {
                    known.push(id.to_owned());
                }
            }
            _ => unknown.push(id.to_owned()),
        }
    }
    known.sort();
    unknown.sort();
    known.extend(unknown);
    known
}

/// `injectEnabledCreditTypes`: `None` for an empty or invalid payload.
pub(crate) fn inject(payload: &[u8]) -> Option<Vec<u8>> {
    if payload.is_empty() || !gj::valid(payload) {
        return None;
    }
    gj::try_set_raw(payload, "enabledCreditTypes", r#"["GOOGLE_ONE_AI"]"#).ok()
}

/// `antigravityHasExplicitCreditsBalanceExhaustedReason`.
pub(crate) fn balance_exhausted(body: &[u8]) -> bool {
    let details = gj::get(body, "error.details");
    details.is_array()
        && details.array().iter().any(|d| {
            *d.get("@type").bytes() == *b"type.googleapis.com/google.rpc.ErrorInfo"
                && trim_space(&d.get("reason").bytes()).eq_ignore_ascii_case(b"INSUFFICIENT_G1_CREDITS_BALANCE")
        })
}

/// `markAntigravityCreditsPermanentlyDisabled`: the credential is known to be out of
/// credits until a lookup finds some again.
pub(crate) fn mark_exhausted(id: &str) {
    set_hint(
        id,
        Hint {
            known: true,
            available: false,
            credit_amount: 0.0,
            min_credit_amount: 1.0,
            paid_tier_id: String::new(),
        },
    );
}

/// The `queueAntigravityCreditsRefresh` gate: at most one lookup per credential every
/// ten minutes, and never two at once. Returns whether the caller should start one.
pub(crate) fn begin_refresh(id: &str) -> bool {
    let mut refreshes = REFRESHES.lock().unwrap_or_else(PoisonError::into_inner);
    let now = Instant::now();
    if let Some((started, running)) = refreshes.get(id)
        && (*running || now.duration_since(*started) < REFRESH_INTERVAL)
    {
        return false;
    }
    refreshes.insert(id.to_owned(), (now, true));
    true
}

/// Whether a lookup for `id` is running.
#[cfg(test)]
pub(crate) fn refreshing(id: &str) -> bool {
    REFRESHES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(id)
        .is_some_and(|(_, running)| *running)
}

/// A lookup ended. Its start still counts: Go retries a failed lookup only after the
/// interval, like a successful one.
pub(crate) fn end_refresh(id: &str) {
    if let Some(entry) = REFRESHES.lock().unwrap_or_else(PoisonError::into_inner).get_mut(id) {
        entry.1 = false;
    }
}

/// The hint a `loadCodeAssist` answer gives (`updateAntigravityCreditsBalanceForTask`):
/// the Google One AI credit of `paidTier.availableCredits`, or no credits at all when
/// that list is missing. `None` when the list has no usable Google One AI entry.
pub(crate) fn hint_from_load_code_assist(body: &[u8]) -> Option<Hint> {
    let paid_tier_id = String::from_utf8_lossy(trim_space(&gj::get(body, "paidTier.id").bytes())).into_owned();
    let credits = gj::get(body, "paidTier.availableCredits");
    if !credits.is_array() {
        return Some(Hint {
            known: true,
            available: false,
            paid_tier_id,
            ..Hint::default()
        });
    }
    let number = |r: gj::Res<'_>| cpa_common::json::go_parse_float(trim_space(&r.bytes())).ok();
    credits.array().iter().find_map(|credit| {
        if !credit.get("creditType").bytes().eq_ignore_ascii_case(b"GOOGLE_ONE_AI") {
            return None;
        }
        let amount = number(credit.get("creditAmount"))?;
        let minimum = number(credit.get("minimumCreditAmountForUsage"))?;
        Some(Hint {
            known: true,
            available: amount >= minimum,
            credit_amount: amount,
            min_credit_amount: minimum,
            paid_tier_id: paid_tier_id.clone(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Go TestFindAllAntigravityCreditsCandidateAuths_*: known-available first, then
    /// unknown, each sorted; known-unavailable excluded.
    #[test]
    fn candidates_follow_go_order() {
        let hinted = |id: &str, available: bool| {
            set_hint(
                id,
                Hint {
                    known: true,
                    available,
                    ..Hint::default()
                },
            )
        };
        hinted("cc-known-b", true);
        hinted("cc-known-a", true);
        hinted("cc-empty", false);
        set_hint("cc-unknown-hint", Hint::default());
        let order = order_candidates([
            "cc-unknown-z",
            "cc-known-b",
            "cc-empty",
            "cc-unknown-hint",
            "cc-known-a",
        ]);
        assert_eq!(order, ["cc-known-a", "cc-known-b", "cc-unknown-hint", "cc-unknown-z"]);
    }

    #[test]
    fn load_code_assist_hints() {
        let body = |credits: &str| format!(r#"{{"paidTier":{{"id":" g1-pro ","availableCredits":{credits}}}}}"#);
        let hint = hint_from_load_code_assist(
            body(r#"[{"creditType":"OTHER","creditAmount":"9","minimumCreditAmountForUsage":"1"},{"creditType":"google_one_ai","creditAmount":" 2.5 ","minimumCreditAmountForUsage":"3"}]"#).as_bytes(),
        )
        .unwrap();
        assert_eq!(
            (hint.known, hint.available, hint.credit_amount, hint.min_credit_amount),
            (true, false, 2.5, 3.0)
        );
        assert_eq!(hint.paid_tier_id, "g1-pro");
        // An unparsable amount skips the entry; no usable entry leaves the hint unknown.
        assert_eq!(
            hint_from_load_code_assist(
                body(r#"[{"creditType":"GOOGLE_ONE_AI","creditAmount":"x","minimumCreditAmountForUsage":"1"}]"#)
                    .as_bytes()
            ),
            None
        );
        let none = hint_from_load_code_assist(br#"{"paidTier":{"id":"free"}}"#).unwrap();
        assert_eq!(
            (none.known, none.available, none.paid_tier_id.as_str()),
            (true, false, "free")
        );
        assert!(balance_exhausted(
            br#"{"error":{"details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":" insufficient_g1_credits_balance "}]}}"#
        ));
        assert_eq!(
            inject(b"{\"a\":1}").unwrap(),
            br#"{"a":1,"enabledCreditTypes":["GOOGLE_ONE_AI"]}"#
        );
        assert_eq!(inject(b"{"), None);
    }
}
