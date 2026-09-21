use super::*;
use futures_util::{SinkExt, StreamExt};
use radroots_sdk::transport::{
    RelayAccess, RelayEndpoint, RelayProfile, RelayProfileKind, RelayUrlPolicy,
};
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

pub(super) struct Relay {
    pub(super) url: String,
    pub(super) accept_replacement: Arc<AtomicBool>,
    pub(super) events: Arc<Mutex<Vec<serde_json::Value>>>,
    task: tokio::task::JoinHandle<()>,
}

impl Relay {
    pub(super) async fn start(accept_replacement: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let accept_replacement = Arc::new(AtomicBool::new(accept_replacement));
        let events = Arc::new(Mutex::new(Vec::new()));
        let accepted = accept_replacement.clone();
        let captured = events.clone();
        let task = tokio::spawn(async move {
            loop {
                let (stream, _) = listener.accept().await.unwrap();
                let mut socket = accept_async(stream).await.unwrap();
                while let Some(Ok(frame)) = socket.next().await {
                    let Message::Text(wire) = frame else {
                        continue;
                    };
                    let frame: serde_json::Value = serde_json::from_str(&wire).unwrap();
                    if frame[0] != "EVENT" {
                        continue;
                    }
                    captured.lock().unwrap().push(frame[1].clone());
                    // A received replacement without an OK is not acceptance.
                    if frame[1]["kind"] == 1 && !accepted.load(Ordering::SeqCst) {
                        break;
                    }
                    socket
                        .send(Message::Text(
                            serde_json::json!(["OK", frame[1]["id"], true, ""])
                                .to_string()
                                .into(),
                        ))
                        .await
                        .unwrap();
                    // Keep the connection alive for the next EVENT: the production
                    // client may reuse a relay socket across replacement and child.
                }
            }
        });
        Self {
            url,
            accept_replacement,
            events,
            task,
        }
    }

    pub(super) fn kinds(&self) -> Vec<u64> {
        self.events
            .lock()
            .unwrap()
            .iter()
            .map(|event| event["kind"].as_u64().unwrap())
            .collect()
    }
}

impl Drop for Relay {
    fn drop(&mut self) {
        self.task.abort();
    }
}

pub(super) async fn persistent_runtime(root: &std::path::Path, urls: &[String]) -> TeraRuntime {
    let store = MobileUserStoreConfig::from_encoded(
        root,
        AUTHOR,
        &hex::encode([4; 32]),
        1_900_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    std::fs::create_dir_all(store.owner_directory()).unwrap();
    let signer = radroots_nostr::signing::LocalSigner::new(
        radroots_nostr::key::SecretKey::parse(SECRET).unwrap(),
    )
    .unwrap();
    RuntimeBuilder::new(store)
        .signer(Arc::new(signer))
        .relay_profile(
            RelayProfile::explicit(
                RelayProfileKind::Simulator,
                urls.iter().map(|url| {
                    RelayEndpoint::new(url, RelayUrlPolicy::Local, RelayAccess::ReadWrite).unwrap()
                }),
            )
            .unwrap(),
        )
        .build()
        .await
        .unwrap()
}

pub(super) async fn saved_revision(runtime: &TeraRuntime) -> Phase1RevisionStatus {
    let event = "b".repeat(64);
    let target = Phase1RevisionTarget::new(
        AddCommandType::CreateUpdate,
        CardId::derive(
            TodayCardType::Update,
            &CardSourceIdentity::Event(radroots_event::EventId::parse(&event).unwrap()),
        ),
        event,
        1,
        None,
        AUTHOR,
    )
    .unwrap();
    runtime
        .prepare_revision_intent(
            [102; 16],
            Phase1ReviseIntent::new(
                target,
                Phase1AddCommand::CreateUpdate(CreateUpdate::new("Corrected harvest").unwrap()),
                vec![],
                Phase1DraftFormSnapshot {
                    content: "Corrected harvest".into(),
                    ..update_form()
                },
            )
            .unwrap(),
        )
        .await
        .unwrap()
}

pub(super) async fn wait_retry(runtime: &TeraRuntime, status: &Phase1DraftStatus) {
    let Some(push) = status.push() else {
        return;
    };
    let now = phase1_operation_now_unix_ms().unwrap();
    if let crate::runtime::product_surface::PublicationRetryDecision::DeferredUntil(at) =
        runtime.publication_retry_at(push, now).unwrap()
    {
        assert!(
            at.saturating_sub(now) < 10_000,
            "test must retain a bounded real wait"
        );
        tokio::time::sleep(Duration::from_millis(at.saturating_sub(now) + 10)).await;
    }
}

pub(super) fn raw_child(status: &Phase1RevisionStatus) -> String {
    status
        .retraction()
        .unwrap()
        .push()
        .unwrap()
        .artifact()
        .signed()
        .unwrap()
        .event()
        .raw_json()
        .to_owned()
}
