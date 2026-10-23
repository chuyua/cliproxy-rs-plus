//! Antigravity egress identity and account health, ported from
//! gcl2api-plus `src/antigravity_account_health.py`.
//!
//! This module owns deterministic parsing, TTL, gating, and state transitions.
//! Network probes and the persistent health store are separate integration work.
//! The pure helpers let the probe layer and its tests share exactly one policy.

use std::net::IpAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, bail, ensure};
use cpa_core::config::Config;
use cpa_core::credential::{Credential, MetadataPatch};
use cpa_core::gostr::GoStr;
use cpa_exec::proxy::{GoClients, Proxy};
use serde_json::{Map, Value};

use crate::runtime::{CredentialStore, PatchError};

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

/// Invalidate a bound IP in the same CAS/file write that changes the account
/// proxy. The next explicit health check must establish its new binding.
pub(crate) fn reset_on_proxy_change(previous: &Credential, next: &mut Credential) {
    if previous.provider != "antigravity" {
        return;
    }
    let old_proxy = previous.str("proxy_url").unwrap_or_default().trim();
    let new_proxy = next.str("proxy_url").unwrap_or_default().trim();
    if old_proxy == new_proxy {
        return;
    }
    let Some(Value::Object(health)) = next.metadata.get_mut(CREDENTIAL_HEALTH_FIELD) else {
        return;
    };
    for field in [
        "egress_ip",
        "observed_egress_ip",
        "egress_country",
        "eligibility_reason",
        "eligibility_checked_at",
    ] {
        health.insert(field.into(), Value::Null);
    }
    health.insert("binding_status".into(), Value::from("unchecked"));
    health.insert("eligibility_status".into(), Value::from("unchecked"));
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
        .map(|value| value.trim().go_upper());
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
        .is_some_and(|previous| previous.parse::<IpAddr>() != Ok(ip));
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

fn validate_health_update(updates: &Map<String, Value>) -> anyhow::Result<()> {
    ensure!(!updates.is_empty(), "empty health update");
    for (key, value) in updates {
        match key.as_str() {
            "binding_status" => ensure!(
                value.is_null()
                    || matches!(
                        value.as_str(),
                        Some("unchecked" | "healthy" | "proxy_drift" | "proxy_failed")
                    ),
                "invalid binding_status"
            ),
            "eligibility_status" => ensure!(
                value.is_null()
                    || matches!(
                        value.as_str(),
                        Some(
                            "unchecked"
                                | "eligible"
                                | "geo_blocked"
                                | "account_blocked"
                                | "validation_required"
                                | "error"
                        )
                    ),
                "invalid eligibility_status"
            ),
            "egress_ip" | "observed_egress_ip" => ensure!(
                value.is_null() || value.as_str().is_some_and(|ip| ip.parse::<IpAddr>().is_ok()),
                "invalid egress_ip"
            ),
            "egress_country" => ensure!(
                value.is_null() || value.as_str().is_some_and(|s| s.len() <= 32),
                "invalid egress_country"
            ),
            "eligibility_reason" | "last_403_reason" => ensure!(
                value.is_null() || value.as_str().is_some_and(|s| s.chars().count() <= 1000),
                "invalid health reason"
            ),
            "eligibility_checked_at" | "last_403_at" => ensure!(
                value.is_null() || numeric_timestamp(value).is_some(),
                "invalid health timestamp"
            ),
            "last_403_category" => ensure!(
                value.is_null()
                    || matches!(
                        value.as_str(),
                        Some(
                            "proxy_drift"
                                | "proxy_failed"
                                | "validation_required"
                                | "geo_blocked"
                                | "model_forbidden"
                                | "account_forbidden"
                                | "unknown_403"
                        )
                    ),
                "invalid last_403_category"
            ),
            _ => bail!("unknown health update field"),
        }
    }
    Ok(())
}

/// Apply only account-health fields to an Antigravity credential, preserving
/// every unrelated field (including refresh/access tokens).
///
/// Reuses CredentialStore's atomic file write, private permissions and CAS
/// revision guard. A concurrent token refresh is re-read rather than lost.
/// Call from a blocking task in async contexts, as with `apply_patch`.
///
/// This is a persistence foundation: the network probe and management caller
/// must still invoke it explicitly after validating an actual health result.
pub fn persist_health_update(
    store: &CredentialStore,
    credential_id: &str,
    updates: &Map<String, Value>,
) -> anyhow::Result<Arc<Credential>> {
    persist_health_update_at_revision(store, credential_id, updates, None)
}

/// Drop a stale network result if the credential changed while it was in flight.
fn persist_health_update_at_revision(
    store: &CredentialStore,
    credential_id: &str,
    updates: &Map<String, Value>,
    expected_revision: Option<u64>,
) -> anyhow::Result<Arc<Credential>> {
    validate_health_update(updates)?;
    for _ in 0..4 {
        let current = store.get(credential_id).context("Antigravity credential not found")?;
        ensure!(
            current.provider == "antigravity",
            "health updates require Antigravity credentials"
        );
        ensure!(
            expected_revision.is_none_or(|revision| revision == current.revision),
            "stale account health probe"
        );
        let mut fields = cached_health(&current)
            .and_then(Value::as_object)
            .cloned()
            .unwrap_or_default();
        fields.extend(updates.iter().map(|(key, value)| (key.clone(), value.clone())));
        let mut patch = MetadataPatch::default();
        patch.set.insert(CREDENTIAL_HEALTH_FIELD.into(), Value::Object(fields));
        match store.apply_patch(credential_id, current.revision, &patch) {
            Ok(updated) => return Ok(updated),
            Err(PatchError::Stale { .. }) if expected_revision.is_some() => {
                bail!("stale account health probe")
            }
            Err(PatchError::Stale { .. }) => continue,
            Err(err) => bail!("failed to persist Antigravity account health: {err:?}"),
        }
    }
    bail!("Antigravity account health changed concurrently too many times")
}

/// Convert an IP echo into a persisted account binding decision.
pub fn record_egress(
    store: &CredentialStore,
    id: &str,
    ip: IpAddr,
    country: Option<String>,
    timestamp: f64,
) -> anyhow::Result<Arc<Credential>> {
    record_egress_at_revision(store, id, ip, country, timestamp, None)
}

fn record_egress_at_revision(
    store: &CredentialStore,
    id: &str,
    ip: IpAddr,
    country: Option<String>,
    timestamp: f64,
    expected_revision: Option<u64>,
) -> anyhow::Result<Arc<Credential>> {
    ensure!(timestamp.is_finite(), "invalid timestamp");
    let current = store.get(id).context("account not found")?;
    let prior = cached_health(&current)
        .and_then(|h| h.get("egress_ip"))
        .and_then(Value::as_str);
    let observed = observe_egress(prior, ip, country);
    let mut fields = Map::new();
    // Preserve the accepted IP on drift so repeat probes cannot clear it.
    if observed.should_check_eligibility {
        fields.insert("egress_ip".into(), Value::from(observed.ip.to_string()));
        fields.insert("observed_egress_ip".into(), Value::Null);
    } else {
        fields.insert("observed_egress_ip".into(), Value::from(observed.ip.to_string()));
    }
    fields.insert(
        "egress_country".into(),
        observed.country.map_or(Value::Null, Value::from),
    );
    fields.insert("binding_status".into(), Value::from(observed.binding_status));
    if !observed.should_check_eligibility {
        fields.insert("eligibility_status".into(), Value::from("unchecked"));
        fields.insert(
            "eligibility_reason".into(),
            Value::from("Egress IP changed; rebind required"),
        );
    }
    fields.insert("eligibility_checked_at".into(), Value::from(timestamp));
    persist_health_update_at_revision(store, id, &fields, expected_revision)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EchoOutcome {
    Identity(IpAddr, Option<String>),
    TemporarilyUnavailable(u16),
}

/// A grouped account must use a request-scoped proxy lease, never a proxy
/// inherited from auth JSON, global configuration, or the environment.
fn health_proxy(credential: &Credential, config: &Config) -> anyhow::Result<Proxy> {
    if credential.str("proxy_mode") == Some("group") {
        let Some(Proxy::Url(url)) = credential.attributes.get("proxy_url").map(|url| Proxy::parse(url)) else {
            bail!("Antigravity group proxy binding unavailable");
        };
        ensure!(
            credential.str("proxy_url").is_none_or(|url| url.trim().is_empty()),
            "explicit account proxy conflicts with group mode"
        );
        return Ok(Proxy::Url(url));
    }
    let proxy = Proxy::effective(credential, config);
    ensure!(!matches!(proxy, Proxy::Invalid), "invalid account proxy");
    Ok(proxy)
}

/// Check an account's egress through its configured transport, without
/// falling back to a direct connection when proxy configuration is invalid.
pub async fn fetch_egress(
    clients: &GoClients,
    config: &Config,
    credential: &Credential,
    url: &str,
    user_agent: &str,
) -> anyhow::Result<EchoOutcome> {
    ensure!(!url.trim().is_empty(), "missing IP echo URL");
    let proxy = health_proxy(credential, config)?;
    let client = clients.try_get(&proxy).context("account proxy unavailable")?;
    let response = client
        .get(url)
        .header("User-Agent", user_agent)
        .timeout(Duration::from_secs(15))
        .send()
        .await?;
    if response.status().as_u16() != 200 {
        return Ok(EchoOutcome::TemporarilyUnavailable(response.status().as_u16()));
    }
    let payload: Value = serde_json::from_slice(&read_bounded(response).await?)?;
    ensure!(payload.is_object(), "IP echo must be an object");
    let (ip, country) = parse_egress_identity(&payload).map_err(anyhow::Error::msg)?;
    Ok(EchoOutcome::Identity(ip, country))
}

async fn read_bounded(response: wreq::Response) -> anyhow::Result<Vec<u8>> {
    use futures_util::StreamExt;
    const MAX: usize = 64 * 1024;
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        ensure!(chunk.len() <= MAX - bytes.len(), "IP echo response too large");
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

/// A bounded, credential-backed loadCodeAssist eligibility result.
/// The response body is used only for classification and is never logged or
/// stored. Persist only the generic status reason, not upstream JSON.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EligibilityObservation {
    pub http_status: u16,
    pub eligibility_status: &'static str,
    pub reason: String,
}

fn warmup_project_id(payload: &Value) -> Option<&str> {
    ["cloudaicompanionProject", "projectId"]
        .iter()
        .filter_map(|key| payload.get(*key))
        .filter_map(|value| match value {
            Value::String(value) => Some(value.as_str()),
            Value::Object(fields) => fields.get("id").and_then(Value::as_str),
            _ => None,
        })
        .find(|id| !id.trim().is_empty() && id.len() <= 256)
}

/// Python's low-frequency Antigravity environment warmup. Failures must not
/// replace the result of loadCodeAssist; neither bodies nor tokens are logged.
async fn warmup_antigravity_environment(
    client: &wreq::Client,
    api_base_url: &str,
    project_id: &str,
    access_token: &str,
    user_agent: &str,
) {
    let requests = [
        ("fetchUserInfo", serde_json::json!({"project": project_id})),
        (
            "listExperiments",
            serde_json::json!({"project": project_id, "metadata": {"updateChannel": "stable"}}),
        ),
    ];
    for (path, body) in requests {
        let url = format!("{}/v1internal:{path}", api_base_url.trim_end_matches('/'));
        let request = client
            .post(url.as_str())
            .header("Authorization", format!("Bearer {access_token}"))
            .header("User-Agent", user_agent)
            .header("Content-Type", "application/json")
            .body(body.to_string())
            .timeout(Duration::from_secs(15))
            .send();
        let _ = tokio::time::timeout(Duration::from_secs(15), request).await;
    }
}

pub async fn fetch_eligibility(
    clients: &GoClients,
    config: &Config,
    credential: &Credential,
    api_base_url: &str,
    access_token: &str,
    user_agent: &str,
) -> anyhow::Result<EligibilityObservation> {
    ensure!(!api_base_url.trim().is_empty(), "missing eligibility API URL");
    ensure!(!access_token.trim().is_empty(), "missing eligibility access token");
    ensure!(
        credential.str("access_token") == Some(access_token),
        "eligibility token does not match the bound account"
    );
    let proxy = health_proxy(credential, config)?;
    let client = clients.try_get(&proxy).context("account proxy unavailable")?;
    let url = format!("{}/v1internal:loadCodeAssist", api_base_url.trim_end_matches('/'));
    let response = client
        .post(url.as_str())
        .header("User-Agent", user_agent)
        .header("Authorization", format!("Bearer {access_token}"))
        .header("Content-Type", "application/json")
        .timeout(Duration::from_secs(30))
        .body(r#"{"metadata":{"ideType":"ANTIGRAVITY"}}"#)
        .send()
        .await?;
    let status = response.status().as_u16();
    let body = read_bounded(response).await?;
    let payload = serde_json::from_slice::<Value>(&body).unwrap_or(Value::Null);
    let text = String::from_utf8_lossy(&body);
    let eligibility_status = eligibility_from_response(status, &payload, &text);
    if status == 200
        && let Some(project_id) = warmup_project_id(&payload)
    {
        warmup_antigravity_environment(&client, api_base_url, project_id, access_token, user_agent).await;
    }
    Ok(EligibilityObservation {
        http_status: status,
        eligibility_status,
        reason: format!("Eligibility check HTTP {status}"),
    })
}

/// Optional full check: the same effective account proxy is used for the IP
/// echo and eligibility POST. A changed IP skips the POST entirely, and every
/// result is persisted with the pre-network revision to reject stale probes.
/// This is not yet scheduled automatically on the request path.
#[expect(
    clippy::too_many_arguments,
    reason = "account-scoped health probe requires explicit independent network and identity parameters"
)]
pub async fn check_account_health_and_persist(
    store: Arc<CredentialStore>,
    clients: &GoClients,
    config: &Config,
    id: &str,
    ip_check_url: &str,
    api_base_url: &str,
    access_token: &str,
    user_agent: &str,
    timestamp: f64,
) -> anyhow::Result<Arc<Credential>> {
    let current = store.get(id).context("account not found")?;
    ensure!(current.provider == "antigravity", "wrong provider");
    ensure!(timestamp.is_finite(), "invalid timestamp");
    ensure!(
        current.str("access_token") == Some(access_token),
        "eligibility token does not match the bound account"
    );
    let revision = current.revision;
    let echo = fetch_egress(clients, config, &current, ip_check_url, user_agent).await;
    let mut verified = None;
    if let Ok(EchoOutcome::Identity(ip, country)) = &echo {
        let prior_ip = cached_health(&current)
            .and_then(|h| h.get("egress_ip"))
            .and_then(Value::as_str);
        if observe_egress(prior_ip, *ip, country.clone()).should_check_eligibility {
            let result = fetch_eligibility(clients, config, &current, api_base_url, access_token, user_agent).await;
            verified = Some((*ip, country.clone(), result));
        }
    }
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || {
        if let Some((ip, country, result)) = verified {
            let mut fields = Map::new();
            fields.insert("egress_ip".into(), Value::from(ip.to_string()));
            fields.insert("observed_egress_ip".into(), Value::Null);
            fields.insert("egress_country".into(), country.map_or(Value::Null, Value::from));
            fields.insert("binding_status".into(), Value::from("healthy"));
            fields.insert("eligibility_checked_at".into(), Value::from(timestamp));
            match result {
                Ok(outcome) => {
                    fields.insert("eligibility_status".into(), Value::from(outcome.eligibility_status));
                    fields.insert("eligibility_reason".into(), Value::from(outcome.reason));
                }
                Err(_) => {
                    // An eligibility infrastructure error is not evidence of
                    // account blocking; preserve the previous eligibility verdict.
                    fields.insert(
                        "eligibility_reason".into(),
                        Value::from("Eligibility check temporarily failed"),
                    );
                }
            }
            persist_health_update_at_revision(&store, &id, &fields, Some(revision))
        } else {
            persist_egress_result_at_revision(&store, &id, echo, timestamp, Some(revision))
        }
    })
    .await
    .context("health worker failed")?
}

/// Explicitly accept a previously observed egress IP only after a new echo
/// through this credential's effective proxy and a successful eligibility
/// probe. This does not select another proxy node or run automatically.
/// The caller must explicitly authorize the exact observed IP.
#[expect(
    clippy::too_many_arguments,
    reason = "security-sensitive explicit rebind keeps the expected IP, account and egress configuration visible"
)]
pub async fn confirm_egress_rebind_and_persist(
    store: Arc<CredentialStore>,
    clients: &GoClients,
    config: &Config,
    id: &str,
    expected_ip: IpAddr,
    ip_check_url: &str,
    api_base_url: &str,
    access_token: &str,
    user_agent: &str,
    timestamp: f64,
) -> anyhow::Result<Arc<Credential>> {
    let current = store.get(id).context("account not found")?;
    ensure!(current.provider == "antigravity", "wrong provider");
    ensure!(timestamp.is_finite(), "invalid timestamp");
    ensure!(
        current.str("access_token") == Some(access_token),
        "rebind token does not match the bound account"
    );
    let health = cached_health(&current).context("account has no drift evidence")?;
    ensure!(
        health.get("binding_status").and_then(Value::as_str) == Some("proxy_drift"),
        "explicit rebind requires a drifted binding"
    );
    ensure!(
        health
            .get("observed_egress_ip")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<IpAddr>().ok())
            == Some(expected_ip),
        "rebind IP does not match the observed drift"
    );
    let revision = current.revision;
    let (verified_ip, country) = match fetch_egress(clients, config, &current, ip_check_url, user_agent).await? {
        EchoOutcome::Identity(ip, country) if ip == expected_ip => (ip, country),
        _ => bail!("rebind echo did not confirm the observed IP"),
    };
    let eligibility = fetch_eligibility(clients, config, &current, api_base_url, access_token, user_agent).await?;
    ensure!(
        eligibility.http_status == 200 && eligibility.eligibility_status == "eligible",
        "rebind eligibility was not confirmed"
    );
    let mut fields = Map::new();
    fields.insert("egress_ip".into(), Value::from(verified_ip.to_string()));
    fields.insert("observed_egress_ip".into(), Value::Null);
    fields.insert("egress_country".into(), country.map_or(Value::Null, Value::from));
    fields.insert("binding_status".into(), Value::from("healthy"));
    fields.insert("eligibility_status".into(), Value::from("eligible"));
    fields.insert(
        "eligibility_reason".into(),
        Value::from("Rebind confirmed by eligibility check"),
    );
    fields.insert("eligibility_checked_at".into(), Value::from(timestamp));
    let id = id.to_owned();
    tokio::task::spawn_blocking(move || persist_health_update_at_revision(&store, &id, &fields, Some(revision)))
        .await
        .context("health worker failed")?
}

/// Run the bound IP check and atomically record its outcome.
pub async fn check_egress_and_persist(
    store: Arc<CredentialStore>,
    clients: &GoClients,
    config: &Config,
    id: &str,
    url: &str,
    user_agent: &str,
    timestamp: f64,
) -> anyhow::Result<Arc<Credential>> {
    let current = store.get(id).context("account not found")?;
    ensure!(current.provider == "antigravity", "wrong provider");
    ensure!(timestamp.is_finite(), "invalid timestamp");
    let result = fetch_egress(clients, config, &current, url, user_agent).await;
    let id = id.to_owned();
    let revision = current.revision;
    tokio::task::spawn_blocking(move || {
        persist_egress_result_at_revision(&store, &id, result, timestamp, Some(revision))
    })
    .await
    .context("health worker failed")?
}

#[cfg(test)]
fn persist_egress_result(
    store: &CredentialStore,
    id: &str,
    result: anyhow::Result<EchoOutcome>,
    timestamp: f64,
) -> anyhow::Result<Arc<Credential>> {
    persist_egress_result_at_revision(store, id, result, timestamp, None)
}

fn persist_egress_result_at_revision(
    store: &CredentialStore,
    id: &str,
    result: anyhow::Result<EchoOutcome>,
    timestamp: f64,
    expected_revision: Option<u64>,
) -> anyhow::Result<Arc<Credential>> {
    match result {
        Ok(EchoOutcome::Identity(ip, country)) => {
            record_egress_at_revision(store, id, ip, country, timestamp, expected_revision)
        }
        Ok(EchoOutcome::TemporarilyUnavailable(status)) => {
            let mut fields = Map::new();
            fields.insert("eligibility_checked_at".into(), Value::from(timestamp));
            fields.insert("eligibility_reason".into(), Value::from(echo_failure_reason(status)));
            persist_health_update_at_revision(store, id, &fields, expected_revision)
        }
        Err(_) => persist_echo_error(store, id, timestamp, expected_revision),
    }
}

fn persist_echo_error(
    store: &CredentialStore,
    id: &str,
    timestamp: f64,
    expected_revision: Option<u64>,
) -> anyhow::Result<Arc<Credential>> {
    let mut fields = Map::new();
    // A transient echo failure must not erase evidence of an unaccepted IP drift.
    let drift = store
        .get(id)
        .and_then(|credential| {
            cached_health(&credential)
                .and_then(|health| health.get("binding_status").and_then(Value::as_str).map(str::to_owned))
        })
        .as_deref()
        == Some("proxy_drift");
    if !drift {
        fields.insert("binding_status".into(), Value::from("proxy_failed"));
    }
    fields.insert("eligibility_status".into(), Value::from("error"));
    fields.insert("eligibility_reason".into(), Value::from("IP check failed"));
    fields.insert("eligibility_checked_at".into(), Value::from(timestamp));
    persist_health_update_at_revision(store, id, &fields, expected_revision)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[tokio::test]
    async fn group_health_probes_never_send_unbound_oauth_directly() {
        use axum::{
            Router,
            routing::{get, post},
        };
        use std::sync::atomic::{AtomicUsize, Ordering};

        let hits = Arc::new(AtomicUsize::new(0));
        let get_hits = Arc::clone(&hits);
        let post_hits = Arc::clone(&hits);
        let app = Router::new()
            .route(
                "/ip",
                get(move || {
                    let hits = Arc::clone(&get_hits);
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        axum::Json(json!({"ip": "203.0.113.8"}))
                    }
                }),
            )
            .route(
                "/v1internal:loadCodeAssist",
                post(move || {
                    let hits = Arc::clone(&post_hits);
                    async move {
                        hits.fetch_add(1, Ordering::SeqCst);
                        axum::Json(json!({"currentTier": {"id": "free-tier"}}))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let data = json!({
            "type": "antigravity", "proxy_mode": "group", "proxy_group_id": 9,
            "access_token": "dummy-access"
        });
        let credential = Credential::from_file(
            std::path::Path::new("/mock"),
            std::path::Path::new("/mock/group.json"),
            data.as_object().unwrap().clone(),
        )
        .unwrap();
        let clients = GoClients::new(Default::default());
        let base = format!("http://{address}");
        assert!(
            fetch_egress(
                &clients,
                &Config::default(),
                &credential,
                &format!("{base}/ip"),
                "mock-test"
            )
            .await
            .is_err()
        );
        assert!(
            fetch_eligibility(
                &clients,
                &Config::default(),
                &credential,
                &base,
                "dummy-access",
                "mock-test"
            )
            .await
            .is_err()
        );
        assert_eq!(
            hits.load(Ordering::SeqCst),
            0,
            "unbound group must never send a network request"
        );
        let mut explicit_direct = credential.clone();
        explicit_direct.metadata.insert("proxy_url".into(), json!("direct"));
        assert!(health_proxy(&explicit_direct, &Config::default()).is_err());
        let mut bound = credential.clone();
        bound
            .attributes
            .insert("proxy_url".into(), "http://mock-proxy.invalid:1234".into());
        assert!(matches!(health_proxy(&bound, &Config::default()), Ok(Proxy::Url(_))));
        server.abort();
    }

    #[tokio::test]
    async fn ip_echo_uses_only_loopback_mock() {
        use axum::{Json, Router, routing::get};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let app = Router::new().route(
            "/ip",
            get(|| async { Json(json!({"ip":"203.0.113.8","country_code":"us"})) }),
        );
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let data = json!({"type":"antigravity","proxy_url":"direct"});
        let credential = Credential::from_file(
            std::path::Path::new("/mock"),
            std::path::Path::new("/mock/a.json"),
            data.as_object().unwrap().clone(),
        )
        .unwrap();
        let clients = GoClients::new(Default::default());
        let result = fetch_egress(
            &clients,
            &Config::default(),
            &credential,
            &format!("http://{address}/ip"),
            "mock-test",
        )
        .await
        .unwrap();
        assert_eq!(
            result,
            EchoOutcome::Identity("203.0.113.8".parse().unwrap(), Some("US".into()))
        );
        server.abort();
    }

    #[tokio::test]
    async fn ip_then_eligibility_post_uses_loopback_mock_and_stops_on_drift() {
        use axum::{
            Json, Router,
            http::HeaderMap,
            routing::{get, post},
        };
        use std::sync::atomic::{AtomicUsize, Ordering};

        let post_count = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&post_count);
        let warmup_count = Arc::new(AtomicUsize::new(0));
        let user_info_count = Arc::clone(&warmup_count);
        let experiments_count = Arc::clone(&warmup_count);
        let app = Router::new()
            .route(
                "/ip",
                get(|headers: HeaderMap| async move {
                    assert!(
                        !headers.contains_key("authorization"),
                        "IP echo must never receive OAuth"
                    );
                    Json(json!({"ip":"203.0.113.8","country_code":"US"}))
                }),
            )
            .route(
                "/drift",
                get(|| async { Json(json!({"ip":"203.0.113.9","country_code":"US"})) }),
            )
            .route(
                "/v1internal:loadCodeAssist",
                post(move |headers: HeaderMap, body: String| {
                    let count = Arc::clone(&count);
                    async move {
                        assert_eq!(headers.get("authorization").unwrap(), "Bearer dummy-access");
                        assert_eq!(body, r#"{"metadata":{"ideType":"ANTIGRAVITY"}}"#);
                        count.fetch_add(1, Ordering::SeqCst);
                        Json(json!({"currentTier":{"id":"free-tier"},"cloudaicompanionProject":"mock-project"}))
                    }
                }),
            )
            .route(
                "/v1internal:fetchUserInfo",
                post(move |headers: HeaderMap, Json(body): Json<Value>| {
                    let count = Arc::clone(&user_info_count);
                    async move {
                        assert_eq!(headers.get("authorization").unwrap(), "Bearer dummy-access");
                        assert_eq!(body, json!({"project":"mock-project"}));
                        count.fetch_add(1, Ordering::SeqCst);
                        Json(json!({}))
                    }
                }),
            )
            .route(
                "/v1internal:listExperiments",
                post(move |headers: HeaderMap, Json(body): Json<Value>| {
                    let count = Arc::clone(&experiments_count);
                    async move {
                        assert_eq!(headers.get("authorization").unwrap(), "Bearer dummy-access");
                        assert_eq!(
                            body,
                            json!({"project":"mock-project","metadata":{"updateChannel":"stable"}})
                        );
                        count.fetch_add(1, Ordering::SeqCst);
                        Json(json!({}))
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("gcl2api-eligibility-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("account.json");
        let data = json!({
            "type":"antigravity",
            "proxy_url":"direct",
            "access_token":"dummy-access",
            "refresh_token":"dummy-refresh"
        });
        std::fs::write(&file, serde_json::to_vec(&data).unwrap()).unwrap();
        let credential = Credential::from_file(&dir, &file, data.as_object().unwrap().clone()).unwrap();
        let store = Arc::new(CredentialStore::new(vec![credential]));
        let clients = GoClients::new(Default::default());
        let base = format!("http://{address}");
        let healthy = check_account_health_and_persist(
            Arc::clone(&store),
            &clients,
            &Config::default(),
            "account.json",
            &format!("{base}/ip"),
            &base,
            "dummy-access",
            "mock-test",
            1000.0,
        )
        .await
        .unwrap();
        assert_eq!(cached_statuses(&healthy), (Some("healthy"), Some("eligible")));
        assert_eq!(post_count.load(Ordering::SeqCst), 1);
        assert_eq!(warmup_count.load(Ordering::SeqCst), 2);
        assert!(
            check_account_health_and_persist(
                Arc::clone(&store),
                &clients,
                &Config::default(),
                "account.json",
                &format!("{base}/ip"),
                &base,
                "wrong-account-token",
                "mock-test",
                1000.5,
            )
            .await
            .is_err(),
            "never send a different account token through this bound proxy"
        );
        assert_eq!(post_count.load(Ordering::SeqCst), 1);
        let drift = check_account_health_and_persist(
            Arc::clone(&store),
            &clients,
            &Config::default(),
            "account.json",
            &format!("{base}/drift"),
            &base,
            "dummy-access",
            "mock-test",
            1001.0,
        )
        .await
        .unwrap();
        assert_eq!(cached_statuses(&drift), (Some("proxy_drift"), Some("unchecked")));
        assert_eq!(cached_health(&drift).unwrap()["egress_ip"], "203.0.113.8");
        assert_eq!(cached_health(&drift).unwrap()["observed_egress_ip"], "203.0.113.9");
        assert_eq!(
            post_count.load(Ordering::SeqCst),
            1,
            "drift must skip credential-backed POST"
        );
        assert_eq!(warmup_count.load(Ordering::SeqCst), 2);
        let drift_ip: IpAddr = "203.0.113.9".parse().unwrap();
        assert!(
            confirm_egress_rebind_and_persist(
                Arc::clone(&store),
                &clients,
                &Config::default(),
                "account.json",
                "203.0.113.10".parse().unwrap(),
                &format!("{base}/drift"),
                &base,
                "dummy-access",
                "mock-test",
                1001.5,
            )
            .await
            .is_err(),
            "an unobserved IP cannot be approved"
        );
        assert!(
            confirm_egress_rebind_and_persist(
                Arc::clone(&store),
                &clients,
                &Config::default(),
                "account.json",
                drift_ip,
                &format!("{base}/ip"),
                &base,
                "dummy-access",
                "mock-test",
                1001.6,
            )
            .await
            .is_err(),
            "the new echo must still match the approved IP"
        );
        assert_eq!(post_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            cached_statuses(&store.get("account.json").unwrap()),
            (Some("proxy_drift"), Some("unchecked"))
        );
        let confirmed = confirm_egress_rebind_and_persist(
            Arc::clone(&store),
            &clients,
            &Config::default(),
            "account.json",
            drift_ip,
            &format!("{base}/drift"),
            &base,
            "dummy-access",
            "mock-test",
            1002.0,
        )
        .await
        .unwrap();
        assert_eq!(cached_statuses(&confirmed), (Some("healthy"), Some("eligible")));
        assert_eq!(warmup_count.load(Ordering::SeqCst), 4);
        assert_eq!(cached_health(&confirmed).unwrap()["egress_ip"], "203.0.113.9");
        assert!(cached_health(&confirmed).unwrap()["observed_egress_ip"].is_null());
        assert_eq!(post_count.load(Ordering::SeqCst), 2);
        assert!(
            confirm_egress_rebind_and_persist(
                Arc::clone(&store),
                &clients,
                &Config::default(),
                "account.json",
                drift_ip,
                &format!("{base}/drift"),
                &base,
                "dummy-access",
                "mock-test",
                1003.0,
            )
            .await
            .is_err(),
            "a healthy binding must not be rebound without new drift evidence"
        );
        assert_eq!(post_count.load(Ordering::SeqCst), 2);
        let disk: Value = serde_json::from_slice(&std::fs::read(&file).unwrap()).unwrap();
        assert_eq!(disk["access_token"], "dummy-access");
        assert_eq!(disk["refresh_token"], "dummy-refresh");
        server.abort();
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

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
    fn warmup_project_id_accepts_only_bounded_known_shapes() {
        assert_eq!(
            warmup_project_id(&json!({"cloudaicompanionProject":"project-1"})),
            Some("project-1")
        );
        assert_eq!(
            warmup_project_id(&json!({"projectId":{"id":"project-2"}})),
            Some("project-2")
        );
        assert_eq!(warmup_project_id(&json!({"projectId":42})), None);
        assert_eq!(warmup_project_id(&json!({"projectId":""})), None);
        assert_eq!(warmup_project_id(&json!({"projectId":"x".repeat(257)})), None);
        assert_eq!(
            warmup_project_id(&json!({"cloudaicompanionProject":null,"projectId":"fallback"})),
            Some("fallback")
        );
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

    #[test]
    fn persists_health_and_keeps_tokens_and_other_metadata() {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("gcl2api-health-{}-{unique}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("account.json");
        let initial = json!({
            "type": "antigravity",
            "access_token": "dummy-access-token",
            "refresh_token": "dummy-refresh-token",
            "account_data": {"keep": true}
        });
        std::fs::write(&path, serde_json::to_vec(&initial).unwrap()).unwrap();
        let credential = Credential::from_file(&dir, &path, initial.as_object().unwrap().clone()).unwrap();
        let store = CredentialStore::new(vec![credential]);

        let first = json!({
            "egress_ip": "203.0.113.8",
            "egress_country": "US",
            "binding_status": "healthy",
            "eligibility_status": "eligible",
            "eligibility_checked_at": 1234.0
        });
        let updated = persist_health_update(&store, "account.json", first.as_object().unwrap()).unwrap();
        assert!(updated.revision > 0);
        assert_eq!(cached_health(&updated).unwrap()["binding_status"], "healthy");

        let second = json!({
            "last_403_category": "model_forbidden",
            "last_403_reason": "Model not available",
            "last_403_at": 1235.0
        });
        let second = persist_health_update(&store, "account.json", second.as_object().unwrap()).unwrap();
        assert!(second.revision > updated.revision);
        assert_eq!(cached_health(&second).unwrap()["egress_ip"], "203.0.113.8");
        assert_eq!(cached_health(&second).unwrap()["last_403_category"], "model_forbidden");

        let disk: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(disk["access_token"], "dummy-access-token");
        assert_eq!(disk["refresh_token"], "dummy-refresh-token");
        assert_eq!(disk["account_data"], json!({"keep":true}));
        assert_eq!(disk[CREDENTIAL_HEALTH_FIELD]["eligibility_status"], "eligible");
        assert_eq!(disk[CREDENTIAL_HEALTH_FIELD]["last_403_category"], "model_forbidden");

        let invalid = json!({"access_token":"overwritten"});
        assert!(persist_health_update(&store, "account.json", invalid.as_object().unwrap()).is_err());
        let invalid = json!({"binding_status":"not-a-state"});
        assert!(persist_health_update(&store, "account.json", invalid.as_object().unwrap()).is_err());
        let invalid = json!({"egress_ip":"not an ip"});
        assert!(persist_health_update(&store, "account.json", invalid.as_object().unwrap()).is_err());
        assert!(persist_health_update(&store, "missing.json", first.as_object().unwrap()).is_err());

        let after: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(disk, after, "invalid health updates cannot mutate auth files");
        let stable = record_egress(
            &store,
            "account.json",
            "203.0.113.8".parse().unwrap(),
            Some("US".into()),
            1236.0,
        )
        .unwrap();
        assert_eq!(cached_statuses(&stable).0, Some("healthy"));
        let drift = record_egress(&store, "account.json", "203.0.113.9".parse().unwrap(), None, 1237.0).unwrap();
        assert_eq!(cached_statuses(&drift), (Some("proxy_drift"), Some("unchecked")));
        assert_eq!(cached_health(&drift).unwrap()["egress_ip"], "203.0.113.8");
        assert_eq!(cached_health(&drift).unwrap()["observed_egress_ip"], "203.0.113.9");
        let repeat = record_egress(&store, "account.json", "203.0.113.9".parse().unwrap(), None, 1237.5).unwrap();
        assert_eq!(cached_statuses(&repeat), (Some("proxy_drift"), Some("unchecked")));
        assert_eq!(cached_health(&repeat).unwrap()["egress_ip"], "203.0.113.8");
        let unavailable = persist_egress_result(
            &store,
            "account.json",
            Ok(EchoOutcome::TemporarilyUnavailable(429)),
            1238.0,
        )
        .unwrap();
        assert_eq!(cached_statuses(&unavailable), (Some("proxy_drift"), Some("unchecked")));
        let failed = persist_egress_result(
            &store,
            "account.json",
            Err(anyhow::anyhow!("mock transport failure")),
            1239.0,
        )
        .unwrap();
        assert_eq!(cached_statuses(&failed), (Some("proxy_drift"), Some("error")));
        assert_eq!(cached_health(&failed).unwrap()["observed_egress_ip"], "203.0.113.9");
        // Editing the account's proxy must atomically invalidate both accepted
        // and observed IPs. Neither a new proxy nor a repeated probe may carry
        // the previous healthy eligibility verdict forward.
        let mut network_patch = MetadataPatch::default();
        network_patch.set.insert("proxy_url".into(), Value::from("direct"));
        let rebound = store
            .apply_patch("account.json", failed.revision, &network_patch)
            .unwrap();
        assert_eq!(cached_statuses(&rebound), (Some("unchecked"), Some("unchecked")));
        // A probe launched against the old proxy must never re-establish its
        // former binding after the proxy edit. Cover every network outcome.
        for outcome in [
            Ok(EchoOutcome::Identity("203.0.113.8".parse().unwrap(), None)),
            Ok(EchoOutcome::TemporarilyUnavailable(429)),
            Err(anyhow::anyhow!("mock old-proxy failure")),
        ] {
            assert!(
                persist_egress_result_at_revision(&store, "account.json", outcome, 1239.5, Some(failed.revision),)
                    .is_err(),
                "stale network probe must not overwrite the new proxy binding"
            );
        }
        assert_eq!(
            cached_statuses(&store.get("account.json").unwrap()),
            (Some("unchecked"), Some("unchecked"))
        );
        let rebound_health = cached_health(&rebound).unwrap();
        for key in [
            "egress_ip",
            "observed_egress_ip",
            "egress_country",
            "eligibility_checked_at",
        ] {
            assert!(rebound_health[key].is_null(), "stale {key} after proxy change");
        }
        let new_echo = record_egress(&store, "account.json", "203.0.113.9".parse().unwrap(), None, 1240.0).unwrap();
        assert_eq!(cached_statuses(&new_echo), (Some("healthy"), Some("unchecked")));
        assert_eq!(cached_health(&new_echo).unwrap()["egress_ip"], "203.0.113.9");
        // Idempotent proxy edits must not destroy the newly learned IP.
        let same_proxy = store
            .apply_patch("account.json", new_echo.revision, &network_patch)
            .unwrap();
        assert_eq!(cached_health(&same_proxy).unwrap()["egress_ip"], "203.0.113.9");
        let disk: Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(disk["refresh_token"], "dummy-refresh-token");
        drop(store);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn rejected_non_antigravity_metadata_is_unchanged() {
        let mut data = json!({"type": "claude", "access_token": "dummy"});
        let metadata = data.as_object_mut().unwrap().clone();
        let credential = Credential::from_file(
            std::path::Path::new("/mock"),
            std::path::Path::new("/mock/not-antigravity.json"),
            metadata,
        )
        .unwrap();
        let store = CredentialStore::new(vec![credential]);
        let health = json!({"binding_status":"healthy"});
        assert!(persist_health_update(&store, "not-antigravity.json", health.as_object().unwrap()).is_err());
        assert!(cached_health(&store.get("not-antigravity.json").unwrap()).is_none());
    }
}
