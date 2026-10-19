//! Antigravity egress identity and account health, ported from
//! gcl2api-plus `src/antigravity_account_health.py`.
//!
//! This module owns deterministic parsing, TTL, gating, and state transitions.
//! Network probes and the persistent health store are separate integration work.
//! The pure helpers let the probe layer and its tests share exactly one policy.

use std::net::IpAddr;

use cpa_core::credential::Credential;
use serde_json::Value;

pub const CREDENTIAL_HEALTH_FIELD: &str = "gcl2api_account_health";

/// Cached per-account health, if provided by a credential's local metadata.
/// This field is an opt-in bridge, not a claim that the Python SQLite health
/// table has already been imported.
pub fn cached_health(credential: &Credential) -> Option<&Value> {
    credential
        .metadata
        .get(CREDENTIAL_HEALTH_FIELD)
        .filter(|v| v.is_object())
}

pub fn cached_statuses(credential: &Credential) -> (Option<&str>, Option<&str>) {
    let health = cached_health(credential);
    (
        health.and_then(|h| h.get("binding_status")).and_then(Value::as_str),
        health.and_then(|h| h.get("eligibility_status")).and_then(Value::as_str),
    )
}

/// Python `parse_egress_identity`: prefer `ip`, `query`, `origin`,
/// `address`, take the first comma-separated address and validate it.
pub fn parse_egress_identity(payload: &Value) -> Result<(IpAddr, Option<String>), &'static str> {
    let value = ["ip", "query", "origin", "address"]
        .iter()
        .filter_map(|key| payload.get(*key))
        .find(|value| !value.is_null() && *value != "");
    let Some(value) = value else {
        return Err("IP echo response does not contain an IP address");
    };
    let Some(raw) = value.as_str() else {
        return Err("IP echo response contains an invalid IP address");
    };
    let ip = raw
        .split(',')
        .next()
        .unwrap_or_default()
        .trim()
        .parse::<IpAddr>()
        .map_err(|_| "IP echo response contains an invalid IP address")?;
    let country = ["country_code", "countryCode", "country", "country_code2"]
        .iter()
        .filter_map(|key| payload.get(*key).and_then(Value::as_str))
        .find(|v| !v.trim().is_empty())
        .map(|value| value.trim().to_uppercase());
    Ok((ip, country))
}

fn numeric_timestamp(value: &Value) -> Option<f64> {
    let parsed = match value {
        Value::Number(v) => v.as_f64(),
        Value::String(v) => v.trim().parse::<f64>().ok(),
        _ => None,
    }?;
    parsed.is_finite().then_some(parsed)
}

/// Python `account_health_check_is_fresh`: a future-dated or malformed
/// result is not reusable; a zero TTL accepts an exactly matching timestamp.
pub fn check_is_fresh(health: &Value, ttl_seconds: i64, now_seconds: f64) -> bool {
    let Some(checked_at) = health.get("eligibility_checked_at").and_then(numeric_timestamp) else {
        return false;
    };
    let age = now_seconds - checked_at;
    age.is_finite() && (0.0..=(ttl_seconds.max(0) as f64)).contains(&age)
}

pub fn is_fresh_eligible(health: &Value, ttl_seconds: i64, now_seconds: f64) -> bool {
    check_is_fresh(health, ttl_seconds, now_seconds)
        && health.get("binding_status").and_then(Value::as_str) == Some("healthy")
        && health.get("eligibility_status").and_then(Value::as_str) == Some("eligible")
}

/// Python `account_health_has_negative_evidence`: only known proxy IP drift
/// and explicitly blocked accounts should gate requests. Geo-blocked cache,
/// proxy failures, and temporary API failures are not proof of an unusable
/// credential and must not lock a user out.
pub fn has_negative_evidence(health: &Value) -> bool {
    health.get("binding_status").and_then(Value::as_str) == Some("proxy_drift")
        || health.get("eligibility_status").and_then(Value::as_str) == Some("account_blocked")
}

/// The result of a successful IP echo, before the eligibility POST.
/// An IP change skips the POST and calls for a proxy rebind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EgressObservation {
    pub ip: IpAddr,
    pub country: Option<String>,
    pub binding_status: &'static str,
    pub eligibility_status: Option<&'static str>,
    pub should_check_eligibility: bool,
}

pub fn observe_egress(previous_ip: Option<&str>, ip: IpAddr, country: Option<String>) -> EgressObservation {
    let changed = previous_ip
        .filter(|s| !s.is_empty())
        .is_some_and(|previous| previous.parse::<IpAddr>().map_or(true, |saved| saved != ip));
    EgressObservation {
        ip,
        country,
        binding_status: if changed { "proxy_drift" } else { "healthy" },
        eligibility_status: changed.then_some("unchecked"),
        should_check_eligibility: !changed,
    }
}

/// Non-200 from an IP echo is an infrastructure outcome, not an account ban.
/// Keep prior binding and eligibility status as in Python.
pub fn echo_failure_reason(status: u16) -> String {
    format!("IP echo temporarily unavailable: HTTP {status}")
}

fn python_truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(value) => *value,
        Value::Number(value) => value.as_f64().is_some_and(|n| n != 0.0),
        Value::String(value) => !value.is_empty(),
        Value::Array(value) => !value.is_empty(),
        Value::Object(value) => !value.is_empty(),
    }
}

/// Decide how an eligibility probe changes status after a successful IP echo.
/// The caller persists the result together with the timestamp and egress IP.
pub fn eligibility_from_response(status: u16, body: &Value, text: &str) -> &'static str {
    use crate::antigravity_403::{Category, classify};

    if classify(text, None, None).category == Category::GeoBlocked {
        return "geo_blocked";
    }
    if status == 200
        && body.is_object()
        && ["currentTier", "cloudaicompanionProject"]
            .iter()
            .filter_map(|key| body.get(*key))
            .any(python_truthy)
    {
        return "eligible";
    }
    // Python checks account_forbidden, but its health-independent classifier
    // does not return that category without confirmed health evidence.
    "error"
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn ip_echo_formats_follow_python_reference() {
        assert_eq!(
            parse_egress_identity(&json!({"ip":"203.0.113.8","country_code":"US"})).unwrap(),
            ("203.0.113.8".parse().unwrap(), Some("US".into()))
        );
        assert_eq!(
            parse_egress_identity(&json!({"origin":"203.0.113.9, 10.0.0.1"})).unwrap(),
            ("203.0.113.9".parse().unwrap(), None)
        );
        assert_eq!(
            parse_egress_identity(&json!({"query":"2001:db8::1","countryCode":"us"})).unwrap(),
            ("2001:db8::1".parse().unwrap(), Some("US".into()))
        );
        for payload in [json!({}), json!({"ip":"bad-ip"}), json!({"ip":"203.0.113.1,invalid"})] {
            let outcome = parse_egress_identity(&payload);
            if payload.get("ip") == Some(&json!("203.0.113.1,invalid")) {
                assert!(outcome.is_ok(), "first IP of a proxy chain is accepted");
            } else {
                assert!(outcome.is_err(), "reject malformed IP echo");
            }
        }
    }

    #[test]
    fn freshness_and_negative_evidence_follow_python() {
        let health = json!({
            "binding_status": "healthy",
            "eligibility_status": "geo_blocked",
            "eligibility_checked_at": 900.0
        });
        assert!(check_is_fresh(&health, 300, 1000.0));
        assert!(!is_fresh_eligible(&health, 300, 1000.0));
        assert!(!has_negative_evidence(&health)); // cached geo doesn't gate
        assert!(!check_is_fresh(&health, 99, 1000.0));
        assert!(!check_is_fresh(&health, 300, 899.0));
        assert!(!check_is_fresh(&json!({}), 300, 1000.0));
        assert!(!check_is_fresh(&json!({"eligibility_checked_at":"bad"}), 300, 1000.0));
        let healthy = json!({
            "binding_status": "healthy",
            "eligibility_status": "eligible",
            "eligibility_checked_at": "1000"
        });
        assert!(is_fresh_eligible(&healthy, 0, 1000.0));
        for state in ["proxy_failed", "unchecked", "healthy"] {
            assert!(!has_negative_evidence(
                &json!({"binding_status":state,"eligibility_status":"error"})
            ));
        }
        assert!(has_negative_evidence(&json!({"binding_status":"proxy_drift"})));
        assert!(has_negative_evidence(&json!({"eligibility_status":"account_blocked"})));
    }

    #[test]
    fn egress_drift_prevents_eligibility_probe() {
        let ip = "203.0.113.8".parse().unwrap();
        let drift = observe_egress(Some("203.0.113.7"), ip, Some("US".into()));
        assert_eq!(drift.binding_status, "proxy_drift");
        assert_eq!(drift.eligibility_status, Some("unchecked"));
        assert!(!drift.should_check_eligibility);
        assert!(observe_egress(Some("203.0.113.8"), ip, None).should_check_eligibility);
        assert!(observe_egress(None, ip, None).should_check_eligibility);
        assert_eq!(echo_failure_reason(429), "IP echo temporarily unavailable: HTTP 429");
    }

    #[test]
    fn eligibility_distinguishes_geo_and_temporary_failures() {
        let entitled = json!({"currentTier":{"id":"free-tier"},"cloudaicompanionProject":"project-1"});
        assert_eq!(eligibility_from_response(200, &entitled, ""), "eligible");
        assert_eq!(
            eligibility_from_response(403, &json!({"error":"unsupported country"}), "unsupported country"),
            "geo_blocked"
        );
        assert_eq!(
            eligibility_from_response(429, &json!({"error":"rate limited"}), "rate limited"),
            "error"
        );
        assert_eq!(eligibility_from_response(200, &json!({}), ""), "error");
        assert_eq!(
            eligibility_from_response(200, &json!({"currentTier":{}}), ""),
            "error",
            "Python considers empty tier objects false"
        );
        assert_eq!(
            eligibility_from_response(200, &json!({"cloudaicompanionProject":"project-1"}), ""),
            "eligible"
        );
    }
}
