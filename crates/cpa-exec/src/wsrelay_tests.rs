//! The relay protocol against `tests/fixtures/aistudio_go.json`: what gorilla's
//! `ReadJSON` and `WriteJSON` (encoding/json) make of frames and messages, then session
//! behaviour from internal/wsrelay/session.go.

use std::time::Duration;

use serde_json::Value;

use super::*;

fn fixture() -> Value {
    serde_json::from_str(include_str!("../tests/fixtures/aistudio_go.json")).unwrap()
}

#[test]
fn frames_decode_like_go_read_json() {
    let fixture = fixture();
    let vectors = fixture["decode"].as_array().unwrap();
    assert!(vectors.len() >= 20);
    for v in vectors {
        let frame = v["frame"].as_str().unwrap();
        let got = Message::decode(frame.as_bytes());
        if v["error"] == true {
            assert!(got.is_err(), "{frame}: {got:?}");
            continue;
        }
        let msg = got.unwrap_or_else(|e| panic!("{frame}: {e}"));
        assert_eq!(msg.id, v["id"].as_str().unwrap_or_default(), "{frame}: id");
        assert_eq!(msg.kind, v["type"].as_str().unwrap_or_default(), "{frame}: type");
        let payload = msg
            .payload
            .map(|p| String::from_utf8(GoValue::Object(p).marshal()).unwrap());
        assert_eq!(payload.as_deref(), v["payload"].as_str(), "{frame}: payload");
    }
}

#[test]
fn messages_encode_like_go_write_json() {
    let fixture = fixture();
    for v in fixture["encode"].as_array().unwrap() {
        let payload = v
            .get("payload")
            .and_then(Value::as_object)
            .map(|p| GoValue::from_json(&Value::Object(p.clone())).into_object().unwrap());
        let msg = Message {
            id: v["id"].as_str().unwrap().into(),
            kind: v["type"].as_str().unwrap().into(),
            payload,
        };
        assert_eq!(msg.encode(), v["text"].as_str().unwrap());
    }
}

/// A frame the route would write, decoded.
async fn next(frames: &mut mpsc::UnboundedReceiver<String>) -> Message {
    let frame = tokio::time::timeout(Duration::from_secs(5), frames.recv())
        .await
        .unwrap()
        .unwrap();
    Message::decode(frame.as_bytes()).unwrap()
}

#[tokio::test]
async fn ping_gets_pong_and_requests_complete_on_terminal_messages() {
    let relay = Arc::new(Relay::default());
    let (session, mut frames) = relay.connect_as("AIStudio-X".into());
    assert_eq!(session.provider(), "aistudio-x");
    session.receive(br#"{"id":"p","type":"ping"}"#).await.unwrap();
    assert_eq!(
        next(&mut frames).await,
        Message {
            id: "p".into(),
            kind: PONG.into(),
            payload: None
        }
    );
    let request = HttpRequest {
        method: "POST".into(),
        url: "https://example.invalid/".into(),
        ..HttpRequest::default()
    };
    // Lookup trims and lower-cases the provider name.
    let relay2 = relay.clone();
    let call = tokio::spawn(async move { relay2.non_stream(" AISTUDIO-x ", &request).await });
    let sent = next(&mut frames).await;
    assert_eq!(sent.kind, HTTP_REQUEST);
    // Messages for other IDs are ignored; a stream answer collects until stream_end.
    session
        .receive(br#"{"id":"other","type":"http_response"}"#)
        .await
        .unwrap();
    for frame in [
        format!(
            r#"{{"id":"{}","type":"stream_start","payload":{{"status":201,"headers":{{"x-a":"1"}}}}}}"#,
            sent.id
        ),
        format!(
            r#"{{"id":"{}","type":"stream_chunk","payload":{{"data":"ab"}}}}"#,
            sent.id
        ),
        format!(
            r#"{{"id":"{}","type":"stream_chunk","payload":{{"data":"c"}}}}"#,
            sent.id
        ),
        format!(r#"{{"id":"{}","type":"stream_end"}}"#, sent.id),
    ] {
        session.receive(frame.as_bytes()).await.unwrap();
    }
    let resp = call.await.unwrap().unwrap();
    assert_eq!(resp.status, 201);
    assert_eq!(resp.headers.get("X-A"), Some(&vec!["1".to_owned()]));
    assert_eq!(resp.body, b"abc");
    assert!(
        lock(&session.pending).is_empty(),
        "the terminal message closed the request"
    );
}

#[tokio::test]
async fn closing_fails_waiting_requests_and_notifies_the_observer() {
    let relay = Arc::new(Relay::default());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    relay.set_observer(Arc::new(move |provider, cause| {
        lock(&log).push((provider.to_owned(), cause.map(str::to_owned)));
    }));
    let (first, _frames) = relay.connect_as("aistudio-a".into());
    // A new session of the same name replaces the old one.
    let (second, mut frames) = relay.connect_as("aistudio-a".into());
    assert!(first.is_closed());
    let relay2 = relay.clone();
    let call = tokio::spawn(async move { relay2.non_stream("aistudio-a", &HttpRequest::default()).await });
    next(&mut frames).await;
    // A frame Go cannot decode ends the session and fails the waiting request.
    assert!(second.receive(b"[1]").await.is_err());
    let error = call.await.unwrap().unwrap_err();
    assert!(error.ends_with("(status=0)"), "{error}");
    assert!(relay.providers().is_empty());
    let seen = lock(&seen).clone();
    assert_eq!(seen[0], ("aistudio-a".into(), None));
    assert_eq!(
        seen[1],
        ("aistudio-a".into(), Some("replaced by new connection".into()))
    );
    assert_eq!(seen[2], ("aistudio-a".into(), None));
    assert_eq!(seen.len(), 4);
    assert_eq!(
        relay
            .non_stream("aistudio-a", &HttpRequest::default())
            .await
            .unwrap_err(),
        "wsrelay: provider aistudio-a not connected"
    );
}

/// Bounds a read so a regression fails instead of hanging.
async fn bounded<T>(f: impl std::future::Future<Output = T>) -> T {
    tokio::time::timeout(Duration::from_secs(5), f)
        .await
        .expect("timed out")
}

/// Spawns `fut` and returns once it has been polled and is waiting (`Pending`).
async fn spawn_waiting<F>(fut: F) -> tokio::task::JoinHandle<F::Output>
where
    F: std::future::Future + Send + 'static,
    F::Output: Send,
{
    let (waiting, polled) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let mut fut = std::pin::pin!(fut);
        let mut waiting = Some(waiting);
        std::future::poll_fn(|cx| {
            let poll = fut.as_mut().poll(cx);
            if poll.is_pending()
                && let Some(waiting) = waiting.take()
            {
                let _ = waiting.send(());
            }
            poll
        })
        .await
    });
    bounded(polled).await.expect("the future finished instead of waiting");
    task
}

fn chunk(id: &str, data: &str) -> Vec<u8> {
    format!(r#"{{"id":"{id}","type":"stream_chunk","payload":{{"data":"{data}"}}}}"#).into_bytes()
}

/// Go's `cancelWithError` evicts a buffered message when a request's channel is
/// full, so the session's error still arrives and a half-read answer is not a success.
#[tokio::test]
async fn closing_with_a_full_buffer_still_fails_the_request() {
    let relay = Arc::new(Relay::default());
    let (session, mut frames) = relay.connect_as("aistudio-full".into());
    let request = HttpRequest::default();
    let call = relay.non_stream("aistudio-full", &request);
    tokio::pin!(call);
    // One poll sends the request; the call then reads nothing until it is awaited.
    assert!(tokio::time::timeout(Duration::ZERO, &mut call).await.is_err());
    let id = next(&mut frames).await.id;
    let start = format!(r#"{{"id":"{id}","type":"stream_start","payload":{{"status":200}}}}"#);
    bounded(session.receive(start.as_bytes())).await.unwrap();
    for _ in 1..PENDING_BUFFER {
        bounded(session.receive(&chunk(&id, "x"))).await.unwrap();
    }
    session.close("browser went away");
    assert_eq!(bounded(call).await.unwrap_err(), "browser went away (status=0)");

    // A stream sees every buffered message but the oldest queued one, then the error.
    let (session, mut frames) = relay.connect_as("aistudio-full".into());
    let mut events = relay.stream("aistudio-full", &HttpRequest::default()).await.unwrap();
    let id = next(&mut frames).await.id;
    // The forwarding task holds chunk 1 and the outer channel chunk 0, so the
    // request's queue holds 2..=65 and loses 2.
    for n in 0..PENDING_BUFFER + 2 {
        bounded(session.receive(&chunk(&id, &format!("{n} ")))).await.unwrap();
    }
    session.close("browser went away");
    let mut got = vec![];
    let error = loop {
        let event = bounded(events.recv()).await.unwrap();
        match event.error {
            Some(error) => break error,
            None => got.push(
                std::str::from_utf8(&event.payload)
                    .unwrap()
                    .trim()
                    .parse::<usize>()
                    .unwrap(),
            ),
        }
    };
    let want: Vec<usize> = [0, 1].into_iter().chain(3..PENDING_BUFFER + 2).collect();
    assert_eq!(got, want);
    assert_eq!(error, "browser went away (status=0)");
    assert!(bounded(events.recv()).await.is_none());
}

/// Delivery and the close decide under one lock, as Go's `pr.mu` makes them: a
/// terminal message blocked on a full queue when the session closes never lands after
/// the eviction, so the request fails instead of ending as a truncated success.
#[tokio::test]
async fn a_terminal_message_blocked_at_close_never_follows_the_eviction() {
    for _ in 0..32 {
        let relay = Arc::new(Relay::default());
        let (session, mut frames) = relay.connect_as("aistudio-race".into());
        let request = HttpRequest::default();
        let call = relay.non_stream("aistudio-race", &request);
        tokio::pin!(call);
        assert!(tokio::time::timeout(Duration::ZERO, &mut call).await.is_err());
        let id = next(&mut frames).await.id;
        for _ in 0..PENDING_BUFFER {
            bounded(session.receive(&chunk(&id, "x"))).await.unwrap();
        }
        let end = format!(r#"{{"id":"{id}","type":"stream_end"}}"#);
        let blocked = session.clone();
        // stream_end waits for a free slot.
        let delivery = spawn_waiting(async move { blocked.receive(end.as_bytes()).await }).await;
        session.close("browser went away");
        assert_eq!(bounded(call).await.unwrap_err(), "browser went away (status=0)");
        bounded(delivery).await.unwrap().unwrap();
    }

    // A terminal message queued before the close wins: no error follows it.
    let relay = Arc::new(Relay::default());
    let (session, mut frames) = relay.connect_as("aistudio-race".into());
    let request = HttpRequest::default();
    let call = relay.non_stream("aistudio-race", &request);
    tokio::pin!(call);
    assert!(tokio::time::timeout(Duration::ZERO, &mut call).await.is_err());
    let id = next(&mut frames).await.id;
    session.receive(&chunk(&id, "ab")).await.unwrap();
    session
        .receive(format!(r#"{{"id":"{id}","type":"stream_end"}}"#).as_bytes())
        .await
        .unwrap();
    session.close("browser went away");
    assert_eq!(bounded(call).await.unwrap().body, b"ab");
}

/// Ending a request wakes every delivery waiting for a slot, as dropping an mpsc
/// receiver released every blocked sender.
#[tokio::test]
async fn dropping_the_reader_releases_every_waiting_delivery() {
    let relay = Arc::new(Relay::default());
    let (session, mut frames) = relay.connect_as("aistudio-drop".into());
    let responses = relay
        .send("aistudio-drop", Relay::http_message(&HttpRequest::default()))
        .unwrap();
    let id = next(&mut frames).await.id;
    for _ in 0..PENDING_BUFFER {
        bounded(session.receive(&chunk(&id, "x"))).await.unwrap();
    }
    let mut deliveries = vec![];
    for _ in 0..2 {
        let (session, frame) = (session.clone(), chunk(&id, "y"));
        deliveries.push(spawn_waiting(async move { session.receive(&frame).await }).await);
    }
    drop(responses);
    for delivery in deliveries {
        bounded(delivery).await.unwrap().unwrap();
    }
    assert!(!lock(&session.pending).contains_key(&id));
}

/// Dropping a stream's receiver ends its request even while the browser sends nothing.
#[tokio::test]
async fn dropping_a_stream_receiver_frees_its_request() {
    let relay = Arc::new(Relay::default());
    let (session, mut frames) = relay.connect_as("aistudio-stream-drop".into());
    let events = relay
        .stream("aistudio-stream-drop", &HttpRequest::default())
        .await
        .unwrap();
    let id = next(&mut frames).await.id;
    assert!(lock(&session.pending).contains_key(&id));
    drop(events);
    bounded(async {
        while lock(&session.pending).contains_key(&id) {
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
    })
    .await;
}
