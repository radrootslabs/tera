use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use futures_util::{SinkExt, StreamExt};
use radroots_nostr::{key::SecretKey, signing::LocalSigner};
use radroots_sdk::{
    ClientBuilder,
    transport::{RelayAccess, RelayEndpoint, RelayProfile, RelayProfileKind, RelayUrlPolicy},
};
use radroots_signing::{Signer, signer::BoxFuture};
use tokio::{net::TcpListener, sync::Notify};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use super::{SubmissionReservationRequest, test_support::*};
use crate::{
    TeraRuntime,
    runtime::{
        builder::RuntimeBuilder,
        product_surface::{AddCommandType, ComposerEditSequence, ComposerPartialForm},
        store::{MobileUserStoreConfig, ProtectedDataAvailability},
    },
};

pub(super) struct CountingSigner {
    inner: LocalSigner,
    pub calls: AtomicUsize,
    pub statuses: AtomicUsize,
    pub pause: AtomicBool,
    pub entered: Notify,
    pub resume: Notify,
}

impl CountingSigner {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: LocalSigner::new(
                SecretKey::parse(
                    "0000000000000000000000000000000000000000000000000000000000000001",
                )
                .unwrap(),
            )
            .unwrap(),
            calls: AtomicUsize::new(0),
            statuses: AtomicUsize::new(0),
            pause: AtomicBool::new(false),
            entered: Notify::new(),
            resume: Notify::new(),
        })
    }
    pub fn count(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }
}

impl Signer for CountingSigner {
    fn status(
        &self,
    ) -> BoxFuture<'_, Result<radroots_signing::SignerStatus, radroots_signing::Error>> {
        self.statuses.fetch_add(1, Ordering::SeqCst);
        self.inner.status()
    }
    fn sign(
        &self,
        request: radroots_signing::SignRequest,
    ) -> BoxFuture<'_, Result<radroots_signing::SignReceipt, radroots_signing::Error>> {
        Box::pin(async move {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if self.pause.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                self.resume.notified().await;
            }
            self.inner.sign(request).await
        })
    }
}

pub(super) fn profile(url: &str) -> RelayProfile {
    RelayProfile::explicit(
        RelayProfileKind::Simulator,
        [RelayEndpoint::new(url, RelayUrlPolicy::Local, RelayAccess::ReadWrite).unwrap()],
    )
    .unwrap()
}

pub(super) async fn runtime(
    root: Option<&std::path::Path>,
    signer: Arc<CountingSigner>,
    relay: &str,
) -> Arc<TeraRuntime> {
    let blossom =
        radroots_sdk::transport::BlossomConfig::from_profile(blossom().profile().unwrap());
    Arc::new(if let Some(root) = root {
        let config = MobileUserStoreConfig::from_encoded(
            root,
            AUTHOR,
            &hex::encode([1; 32]),
            NOW,
            ProtectedDataAvailability::Available,
        )
        .unwrap();
        std::fs::create_dir_all(config.owner_directory()).unwrap();
        RuntimeBuilder::new(config)
            .signer(signer)
            .relay_profile(profile(relay))
            .blossom_config(blossom)
            .build()
            .await
            .unwrap()
    } else {
        TeraRuntime::from_client_builder(
            ClientBuilder::memory_default(),
            Some(scope(AUTHOR, "nearby").author()),
            None,
            Some(signer),
            Some(profile(relay)),
            Some(blossom),
        )
        .unwrap()
    })
}

pub(super) async fn prepare(
    runtime: &TeraRuntime,
    request: &SubmissionReservationRequest,
    media: bool,
) {
    let mut input = input(if media {
        AddCommandType::CreatePhotoUpdate
    } else {
        AddCommandType::CreateUpdate
    });
    let bytes = if media {
        let (photo, bytes) = photo();
        input.media.push(photo);
        vec![bytes]
    } else {
        vec![]
    };
    runtime
        .composer_create(
            request.scope(),
            request.composer_id(),
            ComposerEditSequence::INITIAL,
            ComposerPartialForm::new(input).unwrap(),
        )
        .await
        .unwrap();
    runtime.submission_prepare(request, bytes).await.unwrap();
}

pub(super) async fn relay() -> (String, tokio::task::JoinHandle<serde_json::Value>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("ws://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(15), async move {
            let (stream, _) = listener.accept().await.unwrap();
            let mut socket = accept_async(stream).await.unwrap();
            while let Some(message) = socket.next().await {
                let message = message.unwrap();
                if let Message::Text(text) = message {
                    let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                    if value[0] == "EVENT" {
                        let event = value[1].clone();
                        socket
                            .send(Message::Text(
                                serde_json::json!(["OK", event["id"], true, ""])
                                    .to_string()
                                    .into(),
                            ))
                            .await
                            .unwrap();
                        return event;
                    }
                }
            }
            panic!("relay closed without authored event");
        })
        .await
        .expect("bounded loopback relay")
    });
    (url, task)
}

pub(super) fn assert_redacted(status: &super::SubmissionOperationStatus) {
    let debug = format!("{status:?}");
    for private in [
        "PRIVATE",
        "harvest",
        "wire_json",
        "payload",
        "127.0.0.1",
        "nearby",
        AUTHOR,
    ] {
        assert!(
            !debug.contains(private),
            "operation diagnostics must redact captured data"
        );
    }
    assert!(!debug.contains(&format!("{:?}", status.intent().payload())));
}
