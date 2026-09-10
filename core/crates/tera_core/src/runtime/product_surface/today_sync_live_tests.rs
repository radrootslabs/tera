use super::sync_tests::runtime;
use super::tests::{context, ingest, signed};
use super::*;
use futures_util::{SinkExt, StreamExt};
use radroots_transport_nostr::{
    Config, NostrTransport, RelayAccess, RelayEndpoint, RelayProfile, RelayProfileKind,
    RelayUrlPolicy,
};
use serde_json::Value;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::net::TcpListener;
use tokio::sync::oneshot;
use tokio_tungstenite::{accept_async, tungstenite::Message};

async fn listener() -> (TcpListener, String) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    (listener, url)
}

fn live_runtime(urls: &[String], connections: usize) -> TeraRuntime {
    let endpoints = urls
        .iter()
        .map(|url| RelayEndpoint::new(url, RelayUrlPolicy::Local, RelayAccess::ReadOnly).unwrap());
    let profile = RelayProfile::explicit(RelayProfileKind::Simulator, endpoints).unwrap();
    let config = Config::from_profile(profile)
        .with_timeouts(1000, 1000, 500)
        .unwrap()
        .with_max_connections(connections)
        .unwrap();
    runtime(Arc::new(NostrTransport::new(config)))
}

fn selected(urls: Vec<String>) -> LocalNetwork {
    LocalNetwork::new_for_relay_policy(
        "live".into(),
        "Live".into(),
        urls,
        None,
        vec![],
        1,
        super::super::LocalNetworkRelayPolicy::Simulator,
    )
    .unwrap()
}

#[derive(Clone, Copy)]
enum Reply {
    Stall,
    Complete,
    Continuous,
}

async fn serve(listener: TcpListener, reply: Reply, started: oneshot::Sender<()>) -> usize {
    let (stream, _) = listener.accept().await.unwrap();
    let mut socket = accept_async(stream).await.unwrap();
    let mut started = Some(started);
    let mut sent = 0;
    while let Some(message) = socket.next().await {
        let Ok(Message::Text(message)) = message else {
            continue;
        };
        let values: Value = serde_json::from_str(&message).unwrap();
        if values[0] == "CLOSE" {
            return sent;
        }
        if values[0] != "REQ" {
            continue;
        }
        assert_eq!(values.as_array().unwrap().len(), 3, "one remote filter");
        // The shared adapter uses its existing per-relay candidate cap;
        // the application-facing returned page remains limited to 500.
        assert_eq!(values[2]["limit"], 1000);
        assert_eq!(values[2]["kinds"], serde_json::json!(TODAY_SYNC_KINDS));
        started.take().unwrap().send(()).unwrap();
        match reply {
            Reply::Stall => {}
            Reply::Complete => {
                socket
                    .send(Message::Text(
                        serde_json::to_string(&("EOSE", &values[1])).unwrap().into(),
                    ))
                    .await
                    .unwrap();
            }
            Reply::Continuous => {
                // Valid small notifications never reach EOSE. The request must
                // still finish under its original deadline/work inventory.
                for _ in 0..10_000 {
                    if socket
                        .send(Message::Text("[\"NOTICE\",\"still active\"]".into()))
                        .await
                        .is_err()
                    {
                        break;
                    }
                    sent += 1;
                    tokio::task::yield_now().await;
                }
            }
        }
    }
    sent
}

#[tokio::test]
async fn a_never_responding_relay_finishes_and_preserves_cached_today() {
    let (listener, url) = listener().await;
    let (started, observed) = oneshot::channel();
    let server = tokio::spawn(serve(listener, Reply::Stall, started));
    let runtime = live_runtime(std::slice::from_ref(&url), 1);
    let selected = selected(vec![url]);
    ingest(
        &runtime,
        &selected,
        signed(1, vec![], "cached", 2_000_000_000),
        2_000_000_100,
    )
    .await;
    let began = Instant::now();
    let receipt = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.phase1_sync_today(&selected, 2_000_000_200, TodayProjectionUpdate::Incremental),
    )
    .await
    .unwrap()
    .unwrap();
    observed.await.unwrap();
    assert!(began.elapsed() < Duration::from_secs(5));
    assert_eq!(receipt.relay_state, TodayRelaySyncState::Partial);
    assert_eq!(
        receipt.targets[0].final_state,
        Some(TodayTargetSyncState::Cancelled)
    );
    assert_eq!(receipt.projection.visible_cards, 1);
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn continuously_active_relay_cannot_keep_a_today_refresh_open() {
    let (listener, url) = listener().await;
    let (started, observed) = oneshot::channel();
    let server = tokio::spawn(serve(listener, Reply::Continuous, started));
    let runtime = live_runtime(std::slice::from_ref(&url), 1);
    let selected = selected(vec![url]);
    let receipt = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.phase1_sync_today(&selected, 2_000_000_200, TodayProjectionUpdate::Incremental),
    )
    .await
    .unwrap()
    .unwrap();
    observed.await.unwrap();
    assert_ne!(receipt.relay_state, TodayRelaySyncState::Complete);
    assert_eq!(receipt.events_observed, 0);
    assert!(receipt.pages_fetched <= TODAY_SYNC_MAX_PAGES);
    let sent = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    assert!(sent > 0);
}

#[tokio::test]
async fn a_good_and_a_stalled_relay_keep_their_distinct_today_evidence() {
    let (good, good_url) = listener().await;
    let (slow, slow_url) = listener().await;
    let (good_started, good_observed) = oneshot::channel();
    let (slow_started, slow_observed) = oneshot::channel();
    let good_server = tokio::spawn(serve(good, Reply::Complete, good_started));
    let slow_server = tokio::spawn(serve(slow, Reply::Stall, slow_started));
    let selected = selected(vec![good_url, slow_url]);
    let runtime = live_runtime(&selected.relay_urls, 2);
    let receipt = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.phase1_sync_today(&selected, 2_000_000_200, TodayProjectionUpdate::Incremental),
    )
    .await
    .unwrap()
    .unwrap();
    good_observed.await.unwrap();
    slow_observed.await.unwrap();
    assert_eq!(receipt.relay_state, TodayRelaySyncState::Partial);
    assert_eq!(
        receipt
            .targets
            .iter()
            .filter(|target| target.final_state == Some(TodayTargetSyncState::Complete))
            .count(),
        1
    );
    assert_eq!(
        receipt
            .targets
            .iter()
            .filter(|target| target.final_state == Some(TodayTargetSyncState::Cancelled))
            .count(),
        1
    );
    tokio::time::timeout(Duration::from_secs(5), good_server)
        .await
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), slow_server)
        .await
        .unwrap()
        .unwrap();
}

#[tokio::test]
async fn dropping_today_refresh_closes_the_remote_request_and_keeps_local_reads_usable() {
    let (listener, url) = listener().await;
    let (started, observed) = oneshot::channel();
    let server = tokio::spawn(serve(listener, Reply::Stall, started));
    let runtime = live_runtime(std::slice::from_ref(&url), 1);
    let selected = selected(vec![url]);
    let mut refresh = Box::pin(runtime.phase1_sync_today(
        &selected,
        2_000_000_200,
        TodayProjectionUpdate::Incremental,
    ));
    tokio::time::timeout(Duration::from_secs(5), async {
        tokio::select! {
            _ = &mut refresh => panic!("refresh ended before published request"),
            value = observed => value.unwrap(),
        }
    })
    .await
    .unwrap();
    drop(refresh);
    tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap()
        .unwrap();
    runtime
        .phase1_refresh_today(
            &context(None, 1),
            2_000_000_200,
            TodayProjectionUpdate::Incremental,
        )
        .await
        .unwrap();
}
