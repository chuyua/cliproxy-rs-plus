//! The capability probe against local mocks, case for case from Go's
//! antigravity_models_dedup_test.go and antigravity_models_timeout_test.go.

use std::sync::atomic::{AtomicUsize, Ordering};

use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};

use super::*;

/// What the mock answers a request with the given `Authorization` header and path.
#[derive(Clone)]
enum Reply {
    Answer(u16, &'static str),
    /// Answers once the gate is opened.
    Gated(Arc<tokio::sync::Notify>, u16, &'static str),
    /// A redirect to this path.
    Redirect(u16, &'static str),
    /// Sends the status line and headers, then never the body.
    Hang,
}

#[derive(Clone)]
struct Upstream {
    base: String,
    hits: Arc<AtomicUsize>,
    /// The request head of the last request.
    last: Arc<Mutex<String>>,
}

async fn upstream(reply: impl Fn(&str) -> Reply + Send + Sync + 'static) -> Upstream {
    routed(move |authorization, _| reply(authorization)).await
}

async fn routed(reply: impl Fn(&str, &str) -> Reply + Send + Sync + 'static) -> Upstream {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let (hits, last) = (Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(String::new())));
    let reply = Arc::new(reply);
    let (counter, sink) = (hits.clone(), last.clone());
    tokio::spawn(async move {
        while let Ok((socket, _)) = listener.accept().await {
            let (reply, counter, sink) = (reply.clone(), counter.clone(), sink.clone());
            tokio::spawn(async move {
                let (read, mut write) = socket.into_split();
                let mut reader = BufReader::new(read);
                let (mut head, mut length, mut authorization) = (String::new(), 0, String::new());
                let mut path = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    let lower = line.to_ascii_lowercase();
                    if let Some(v) = lower.strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    if lower.starts_with("authorization:") {
                        authorization = line["authorization:".len()..].trim().to_owned();
                    }
                    if head.is_empty() {
                        path = line.split(' ').nth(1).unwrap_or_default().to_owned();
                    }
                    head.push_str(&line);
                    if line == "\r\n" {
                        break;
                    }
                }
                let mut body = vec![0; length];
                let _ = reader.read_exact(&mut body).await;
                counter.fetch_add(1, Ordering::SeqCst);
                *lock(&sink) = format!("{head}{}", String::from_utf8_lossy(&body));
                let mut answer = reply(&authorization, &path);
                if let Reply::Gated(gate, status, body) = answer {
                    gate.notified().await;
                    answer = Reply::Answer(status, body);
                }
                match answer {
                    Reply::Gated(..) => unreachable!(),
                    Reply::Redirect(status, location) => {
                        let out = format!(
                            "HTTP/1.1 {status} X\r\nLocation: {location}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                        );
                        let _ = write.write_all(out.as_bytes()).await;
                    }
                    Reply::Answer(status, body) => {
                        let out = format!(
                            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                            body.len()
                        );
                        let _ = write.write_all(out.as_bytes()).await;
                    }
                    Reply::Hang => {
                        let _ = write.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 100\r\n\r\n").await;
                        std::future::pending::<()>().await;
                    }
                }
            });
        }
    });
    Upstream { base, hits, last }
}

fn credential(id: &str, token: &str, attributes: &[(&str, &str)]) -> Credential {
    let mut c = Credential::from_file(
        std::path::Path::new("/fixture"),
        std::path::Path::new("/fixture/antigravity.json"),
        serde_json::json!({"type": "antigravity", "access_token": token})
            .as_object()
            .unwrap()
            .clone(),
    )
    .unwrap();
    c.id = id.to_owned();
    for (k, v) in attributes {
        c.attributes.insert((*k).to_owned(), (*v).to_owned());
    }
    c
}

/// A manually advanced clock.
fn clock() -> (Capabilities, Arc<Mutex<Instant>>) {
    let now = Arc::new(Mutex::new(Instant::now()));
    let read = now.clone();
    (Capabilities::new(Arc::new(move || *lock(&read))), now)
}

fn advance(now: &Mutex<Instant>, secs: u64) {
    *lock(now) += Duration::from_secs(secs);
}

fn client() -> wreq::Client {
    GoClients::new(Default::default()).get(&Proxy::Inherit)
}

fn ids(list: &[&str]) -> Hints {
    list.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn base_urls_follow_go() {
    // TestAntigravityModelBaseURLs_DefaultDaily.
    assert_eq!(base_urls(&credential("a", "t", &[])), vec![DAILY_BASE_URL]);
    assert_eq!(
        base_urls(&credential(
            "a",
            "t",
            &[("base_urls", " http://a/ , ,http://b//"), ("base_url", "http://c")]
        )),
        vec!["http://a", "http://b"]
    );
    assert_eq!(
        base_urls(&credential(
            "a",
            "t",
            &[("base_urls", " , "), ("base_url", " http://c/ ")]
        )),
        vec!["http://c"]
    );
    let mut from_metadata = credential("a", "t", &[]);
    from_metadata.metadata.insert("base_url".into(), " http://m/ ".into());
    assert_eq!(base_urls(&from_metadata), vec!["http://m"]);
}

#[test]
fn parse_follows_json_unmarshal() {
    assert_eq!(
        parse(br#"{"webSearchModelIds":[" Gemini-3.1-Flash-Lite ",""]}"#),
        Some(ids(&["gemini-3.1-flash-lite"]))
    );
    // Keys match case-insensitively; the last match wins, and null clears.
    assert_eq!(
        parse(br#"{"webSearchModelIds":["a"],"WEBSEARCHMODELIDS":["b"]}"#),
        Some(ids(&["b"]))
    );
    assert_eq!(
        parse(br#"{"webSearchModelIds":["a"],"websearchmodelids":null}"#),
        Some(ids(&[]))
    );
    assert_eq!(parse(b"null"), Some(ids(&[])));
    assert_eq!(parse(br#"{"webSearchModelIds":[null,"x"]}"#), Some(ids(&["x"])));
    assert_eq!(parse(br#"{"other":1}"#), Some(ids(&[])));
    for invalid in [
        &br#"{not-valid-json"#[..],
        br#"{"webSearchModelIds":[1]}"#,
        br#"{"webSearchModelIds":"x"}"#,
        br#"["x"]"#,
        br#"{"webSearchModelIds":[]} x"#,
    ] {
        assert_eq!(parse(invalid), None, "{}", String::from_utf8_lossy(invalid));
    }
}

#[tokio::test]
async fn probe_request_matches_go() {
    let up = upstream(|_| Reply::Answer(200, r#"{"webSearchModelIds":["gemini-3.1-flash-lite"]}"#)).await;
    let (caps, _) = clock();
    let c = credential("ag-req", " ya29.req ", &[("base_url", &format!("{}/", up.base))]);
    assert_eq!(caps.fetch(&client(), &c, "").await, ids(&["gemini-3.1-flash-lite"]));
    let head = lock(&up.last).clone();
    assert!(
        head.starts_with("POST /v1internal:fetchAvailableModels HTTP/1.1\r\n"),
        "{head}"
    );
    for header in [
        "Content-Type: application/json\r\n",
        "Authorization: Bearer ya29.req\r\n",
        "Connection: close\r\n",
        &format!("User-Agent: {}\r\n", crate::antigravity_version::user_agent()),
    ] {
        assert!(head.contains(header), "{header:?} missing: {head}");
    }
    assert!(head.ends_with("\r\n\r\n{}"), "{head}");
    // No token, no probe.
    assert!(
        caps.fetch(&client(), &credential("x", " ", &[("base_url", &up.base)]), "")
            .await
            .is_empty()
    );
    assert_eq!(up.hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn concurrent_probes_share_one_request() {
    // TestAntigravityCapabilityProbe_DeduplicatesConcurrentRequests.
    let up = upstream(|_| Reply::Answer(200, r#"{"webSearchModelIds":["gemini-3.1-flash-lite"]}"#)).await;
    let (caps, _) = clock();
    let caps = Arc::new(caps);
    let mut tasks = Vec::new();
    for i in 0..5 {
        let (caps, base) = (caps.clone(), up.base.clone());
        tasks.push(tokio::spawn(async move {
            let c = credential(&format!("dedup-{i}"), &format!("ya29.{i}"), &[("base_url", &base)]);
            caps.fetch(&client(), &c, "").await
        }));
    }
    for task in tasks {
        assert_eq!(task.await.unwrap(), ids(&["gemini-3.1-flash-lite"]));
    }
    assert_eq!(up.hits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn results_are_cached_for_five_minutes_and_failures_for_one() {
    // TestAntigravityCapabilityProbe_CachedWithinTTL,
    // TestAntigravityCapabilityProbe_CacheExpiresAfterTTL and
    // TestAntigravityCapabilityProbe_CachesEmptyWebSearchModelIDs.
    let up = upstream(|_| Reply::Answer(200, r#"{"webSearchModelIds":[]}"#)).await;
    let (caps, now) = clock();
    let first = credential("ttl-1", "ya29.1", &[("base_url", &up.base)]);
    let other = credential("ttl-2", "ya29.2", &[("base_url", &up.base)]);
    assert!(caps.fetch(&client(), &first, "").await.is_empty());
    advance(&now, 299);
    assert!(caps.fetch(&client(), &other, "").await.is_empty());
    assert_eq!(up.hits.load(Ordering::SeqCst), 1, "served from the cache");
    advance(&now, 1);
    assert!(caps.fetch(&client(), &first, "").await.is_empty());
    assert_eq!(up.hits.load(Ordering::SeqCst), 2, "expired at 5 minutes");
    // The proxy is part of the cache key.
    assert_eq!(
        caps.cached(&format!("{}#", up.base), (caps.clock)()),
        Some(Hints::new())
    );
    assert_eq!(caps.cached(&format!("{}#http://p", up.base), (caps.clock)()), None);

    // TestAntigravityCapabilityProbe_CachesFailureWithBackoff.
    let failing = upstream(|_| Reply::Answer(500, "")).await;
    let (caps, now) = clock();
    let a = credential("fail-1", "ya29.1", &[("base_url", &failing.base)]);
    let b = credential("fail-2", "ya29.2", &[("base_url", &failing.base)]);
    assert!(caps.fetch(&client(), &a, "").await.is_empty());
    advance(&now, 30);
    assert!(caps.fetch(&client(), &b, "").await.is_empty());
    assert_eq!(failing.hits.load(Ordering::SeqCst), 1);
    advance(&now, 31);
    assert!(caps.fetch(&client(), &b, "").await.is_empty());
    assert_eq!(failing.hits.load(Ordering::SeqCst), 2);

    // TestAntigravityCapabilityProbe_InvalidJSONHandledAsFailure.
    let invalid = upstream(|_| Reply::Answer(200, "{not-valid-json")).await;
    let (caps, now) = clock();
    let c = credential("bad-json", "ya29", &[("base_url", &invalid.base)]);
    assert!(caps.fetch(&client(), &c, "").await.is_empty());
    advance(&now, 70);
    caps.fetch(&client(), &c, "").await;
    assert_eq!(invalid.hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn rejected_tokens_back_off_without_poisoning_the_cache() {
    // TestAntigravityCapabilityProbe_AuthErrorDoesNotPoisonCache.
    let up = upstream(|authorization| match authorization {
        "Bearer ya29.bad-token" => Reply::Answer(401, r#"{"error":"unauthorized"}"#),
        "Bearer ya29.forbidden" => Reply::Answer(403, ""),
        _ => Reply::Answer(200, r#"{"webSearchModelIds":["gemini-3.1-flash-lite"]}"#),
    })
    .await;
    let (caps, now) = clock();
    let bad = credential("ag-bad", "ya29.bad-token", &[("base_url", &up.base)]);
    let good = credential("ag-good", "ya29.good-token", &[("base_url", &up.base)]);
    assert!(caps.fetch(&client(), &bad, "").await.is_empty());
    assert_eq!(up.hits.load(Ordering::SeqCst), 1);
    assert_eq!(caps.fetch(&client(), &good, "").await, ids(&["gemini-3.1-flash-lite"]));
    assert_eq!(up.hits.load(Ordering::SeqCst), 2);
    // Within the backoff the bad token gets nothing, even though a good answer is cached.
    assert!(caps.fetch(&client(), &bad, "").await.is_empty());
    assert_eq!(up.hits.load(Ordering::SeqCst), 2);
    // A 403 backs off too; once the backoff ends the cached good answer serves it.
    let forbidden = credential(
        "ag-forbidden",
        "ya29.forbidden",
        &[("base_url", &format!("{}/", up.base))],
    );
    advance(&now, 301);
    assert!(caps.fetch(&client(), &forbidden, "").await.is_empty());
    assert_eq!(up.hits.load(Ordering::SeqCst), 3);
    assert!(caps.fetch(&client(), &forbidden, "").await.is_empty());
    assert_eq!(up.hits.load(Ordering::SeqCst), 3);
    assert_eq!(caps.fetch(&client(), &good, "").await, ids(&["gemini-3.1-flash-lite"]));
    assert_eq!(up.hits.load(Ordering::SeqCst), 4);
    advance(&now, 61);
    assert_eq!(
        caps.fetch(&client(), &forbidden, "").await,
        ids(&["gemini-3.1-flash-lite"])
    );
    assert_eq!(up.hits.load(Ordering::SeqCst), 4);
}

/// Waits until `count` callers are inside the probe.
async fn in_flight(caps: &Capabilities, count: usize) {
    for _ in 0..500 {
        if caps.in_flight.load(Ordering::SeqCst) >= count {
            return;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    panic!("callers never joined the probe");
}

#[tokio::test]
async fn a_rejected_probe_does_not_block_a_waiting_account() {
    // TestAntigravityCapabilityProbe_ConcurrentAuthErrorDoesNotBlockHealthyAccount: the
    // healthy caller joins the rejected probe in flight, then probes with its own token.
    let gate = Arc::new(tokio::sync::Notify::new());
    let held = gate.clone();
    let up = upstream(move |authorization| match authorization {
        "Bearer ya29.bad-token" => Reply::Gated(held.clone(), 401, r#"{"error":"unauthorized"}"#),
        _ => Reply::Answer(200, r#"{"webSearchModelIds":["gemini-3.1-flash-lite"]}"#),
    })
    .await;
    let caps = Arc::new(clock().0);
    let bad = credential("ag-bad-concurrent", "ya29.bad-token", &[("base_url", &up.base)]);
    let good = credential("ag-good-concurrent", "ya29.good-token", &[("base_url", &up.base)]);
    let first = tokio::spawn({
        let caps = caps.clone();
        async move { caps.fetch(&client(), &bad, "").await }
    });
    in_flight(&caps, 1).await;
    let second = tokio::spawn({
        let caps = caps.clone();
        async move { caps.fetch(&client(), &good, "").await }
    });
    in_flight(&caps, 2).await;
    gate.notify_one();
    assert!(first.await.unwrap().is_empty());
    assert_eq!(second.await.unwrap(), ids(&["gemini-3.1-flash-lite"]));
    assert_eq!(up.hits.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn callers_with_the_rejected_token_share_the_rejection() {
    // Go's singleflight hands the follower the leader's result: same token, no retry.
    let gate = Arc::new(tokio::sync::Notify::new());
    let held = gate.clone();
    let up = upstream(move |_| Reply::Gated(held.clone(), 403, "")).await;
    let caps = Arc::new(clock().0);
    let callers: Vec<_> = (0..2)
        .map(|i| credential(&format!("same-token-{i}"), "ya29.shared-bad", &[("base_url", &up.base)]))
        .collect();
    let mut tasks = Vec::new();
    for (i, c) in callers.iter().cloned().enumerate() {
        let task_caps = caps.clone();
        tasks.push(tokio::spawn(async move { task_caps.fetch(&client(), &c, "").await }));
        in_flight(&caps, i + 1).await;
    }
    gate.notify_one();
    for task in tasks {
        assert!(task.await.unwrap().is_empty());
    }
    assert_eq!(up.hits.load(Ordering::SeqCst), 1);
    // Both credentials back off now.
    for c in &callers {
        assert!(caps.fetch(&client(), c, "").await.is_empty());
    }
    assert_eq!(up.hits.load(Ordering::SeqCst), 1);
    assert_eq!(lock(&caps.failures).len(), 2);
}

#[tokio::test]
async fn probes_follow_redirects() {
    // Go probes with a plain http.Client, which follows a 307 with the same POST.
    let up = routed(|_, path| match path {
        "/v1internal:fetchAvailableModels" => Reply::Redirect(307, "/moved/v1internal:fetchAvailableModels"),
        _ => Reply::Answer(200, r#"{"webSearchModelIds":["gemini-3.1-flash-lite"]}"#),
    })
    .await;
    let caps = clock().0;
    let c = credential("redirect", "ya29", &[("base_url", &up.base)]);
    assert_eq!(caps.fetch(&client(), &c, "").await, ids(&["gemini-3.1-flash-lite"]));
    assert_eq!(up.hits.load(Ordering::SeqCst), 2);
    let head = lock(&up.last).clone();
    assert!(
        head.starts_with("POST /moved/v1internal:fetchAvailableModels "),
        "{head}"
    );
    assert!(head.ends_with("\r\n\r\n{}"), "{head}");
}

/// A manually advanced clock and a short probe timeout.
fn quick() -> Capabilities {
    let mut caps = clock().0;
    caps.timeout = Duration::from_millis(300);
    caps
}

#[tokio::test]
async fn probes_time_out() {
    // TestAntigravityCapabilityProbe_HardTimeout.
    assert_eq!(clock().0.timeout, Duration::from_secs(5));
    let up = upstream(|_| Reply::Hang).await;
    let caps = quick();
    let started = Instant::now();
    let c = credential("hang", "ya29", &[("base_url", &up.base)]);
    assert!(caps.fetch(&client(), &c, "").await.is_empty());
    let elapsed = started.elapsed();
    assert!(
        elapsed >= caps.timeout && elapsed < Duration::from_secs(2),
        "{elapsed:?}"
    );
    // A timeout is a transient failure: cached for a minute.
    assert_eq!(
        caps.cached(&format!("{}#", up.base), (caps.clock)()),
        Some(Hints::new())
    );
}

#[tokio::test]
async fn base_urls_are_probed_concurrently() {
    // TestAntigravityCapabilityProbe_ConcurrentBaseURLs.
    let slow = upstream(|_| Reply::Hang).await;
    let fast = upstream(|_| Reply::Answer(200, r#"{"webSearchModelIds":["gemini-3.1-flash-lite"]}"#)).await;
    let caps = clock().0;
    let started = Instant::now();
    let c = credential(
        "multi",
        "ya29",
        &[("base_urls", &format!("{},{}", slow.base, fast.base))],
    );
    assert_eq!(caps.fetch(&client(), &c, "").await, ids(&["gemini-3.1-flash-lite"]));
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(slow.hits.load(Ordering::SeqCst), 1);
    // A success without models waits for the others, then wins over a rejection.
    let empty = upstream(|_| Reply::Answer(200, r#"{"webSearchModelIds":[]}"#)).await;
    let failing = upstream(|_| Reply::Answer(401, "")).await;
    let caps = quick();
    let c = credential(
        "multi-2",
        "ya29",
        &[("base_urls", &format!("{},{}", failing.base, empty.base))],
    );
    assert!(caps.fetch(&client(), &c, "").await.is_empty());
    let key = format!("{}|{}#", failing.base, empty.base);
    assert_eq!(caps.cached(&key, (caps.clock)()), Some(Hints::new()));
    // Only rejections: an auth error, nothing cached.
    let caps = quick();
    let other = upstream(|_| Reply::Answer(403, "")).await;
    let c = credential(
        "multi-4",
        "ya29",
        &[("base_urls", &format!("{},{}", failing.base, other.base))],
    );
    assert!(caps.fetch(&client(), &c, "").await.is_empty());
    assert_eq!(
        caps.cached(&format!("{}|{}#", failing.base, other.base), (caps.clock)()),
        None
    );
    assert_eq!(lock(&caps.failures).len(), 1);
}

#[test]
fn registered_models_carry_the_hints_through_aliases_and_prefixes() {
    // TestAntigravityAsyncProbe_AppliesCapabilitiesToAliasedAndPrefixedModels, on models
    // the static catalog does not flag.
    let cfg = Config::parse(
        "oauth-model-alias:\n  antigravity:\n    - name: claude-sonnet-4-6\n      alias: aliased-sonnet\n    - name: gpt-oss-120b-medium\n      alias: google/oss\n      fork: true\n",
    )
    .unwrap();
    let mut c = credential("ag-alias-registry", "ya29", &[]);
    c.attributes.insert("prefix".into(), "my-prefix".into());
    let aliases = cpa_core::registry::dynamic::global_aliases(&cfg);
    let flagged = || -> HashSet<String> {
        cpa_core::registry::dynamic::models_for(&cfg, &aliases, &c, 0)
            .into_iter()
            .filter(|m| m.supports_web_search)
            .map(|m| m.id)
            .collect()
    };
    let before = flagged();
    assert!(cpa_core::registry::dynamic::set_web_search_hints(
        &c.id,
        ids(&["claude-sonnet-4-6", "gpt-oss-120b-medium"])
    ));
    assert!(!cpa_core::registry::dynamic::set_web_search_hints(
        &c.id,
        ids(&["claude-sonnet-4-6", "gpt-oss-120b-medium"])
    ));
    let added: HashSet<String> = flagged().difference(&before).cloned().collect();
    assert_eq!(
        added,
        ids(&[
            "aliased-sonnet",
            "my-prefix/aliased-sonnet",
            "google/oss",
            "my-prefix/google/oss",
            "gpt-oss-120b-medium",
            "my-prefix/gpt-oss-120b-medium",
        ])
    );
    assert!(cpa_core::registry::dynamic::set_web_search_hints(&c.id, Hints::new()));
    assert_eq!(flagged(), before);
}

#[tokio::test]
async fn registration_probes_each_new_revision_once() {
    let up = upstream(|_| Reply::Answer(200, r#"{"webSearchModelIds":["CLAUDE-SONNET-4-6"]}"#)).await;
    let probes = Arc::new(Probes::default());
    let cfg = Config::default();
    let mut c = credential("ag-registration", "ya29.registration", &[("base_url", &up.base)]);
    c.provider = crate::antigravity::PROVIDER.into();
    let mut disabled = credential("ag-disabled", "ya29.disabled", &[("base_url", &up.base)]);
    disabled.provider = crate::antigravity::PROVIDER.into();
    disabled.disabled = true;
    let creds = vec![Arc::new(c.clone()), Arc::new(disabled)];
    let generation = cpa_core::registry::dynamic::capabilities_generation();
    probes.registered(&GoClients::new(Default::default()), &creds, &cfg);
    probes.registered(&GoClients::new(Default::default()), &creds, &cfg);
    let flagged = || {
        cpa_core::registry::dynamic::models_for(&cfg, &HashMap::new(), &c, 0)
            .into_iter()
            .any(|m| m.id == "claude-sonnet-4-6" && m.supports_web_search)
    };
    for _ in 0..200 {
        if flagged() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(flagged());
    assert!(cpa_core::registry::dynamic::capabilities_generation() > generation);
    // The same revision is not probed again; the shared cache would answer anyway.
    assert_eq!(up.hits.load(Ordering::SeqCst), 1);
    // A credential that left the set loses its hints.
    probes.registered(&GoClients::new(Default::default()), &[], &cfg);
    assert!(!flagged());
}

#[tokio::test]
async fn a_stale_probe_never_replaces_a_newer_one() {
    // TestAntigravityAsyncProbe_DiscardsStaleProbeWhenAuthReRegistered: the probe of an
    // older revision finishes last and is dropped.
    let gate = Arc::new(tokio::sync::Notify::new());
    let held = gate.clone();
    let old =
        upstream(move |_| Reply::Gated(held.clone(), 200, r#"{"webSearchModelIds":["gpt-oss-120b-medium"]}"#)).await;
    let new = upstream(|_| Reply::Answer(200, r#"{"webSearchModelIds":["claude-sonnet-4-6"]}"#)).await;
    let probes = Arc::new(Probes::default());
    let cfg = Config::default();
    let mut c = credential("ag-stale-probe", "ya29.stale", &[("base_url", &old.base)]);
    c.provider = crate::antigravity::PROVIDER.into();
    probes.registered(&GoClients::new(Default::default()), &[Arc::new(c.clone())], &cfg);
    for _ in 0..500 {
        if old.hits.load(Ordering::SeqCst) == 1 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    c.revision += 1;
    c.attributes.insert("base_url".into(), new.base.clone());
    probes.registered(&GoClients::new(Default::default()), &[Arc::new(c.clone())], &cfg);
    let flagged = |id: &str| {
        cpa_core::registry::dynamic::models_for(&cfg, &HashMap::new(), &c, 0)
            .into_iter()
            .any(|m| m.id == id && m.supports_web_search)
    };
    for _ in 0..500 {
        if flagged("claude-sonnet-4-6") {
            break;
        }
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
    assert!(flagged("claude-sonnet-4-6"));
    gate.notify_one();
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(flagged("claude-sonnet-4-6") && !flagged("gpt-oss-120b-medium"));
    probes.registered(&GoClients::new(Default::default()), &[], &cfg);
}
