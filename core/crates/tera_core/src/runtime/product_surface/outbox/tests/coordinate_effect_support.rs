use super::*;
use radroots_signing::{Signer, signer::BoxFuture};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tokio::sync::Notify;

pub(super) struct PausedSigner {
    inner: radroots_nostr::signing::LocalSigner,
    pub calls: AtomicUsize,
    pub pause: AtomicBool,
    pub entered: Notify,
    pub resume: Notify,
}

impl PausedSigner {
    pub(super) fn new() -> Arc<Self> {
        Arc::new(Self {
            inner: radroots_nostr::signing::LocalSigner::new(
                radroots_nostr::key::SecretKey::parse(SECRET).unwrap(),
            )
            .unwrap(),
            calls: AtomicUsize::new(0),
            pause: AtomicBool::new(true),
            entered: Notify::new(),
            resume: Notify::new(),
        })
    }
    pub(super) async fn wait(&self) {
        tokio::time::timeout(std::time::Duration::from_secs(5), self.entered.notified())
            .await
            .unwrap();
    }
}

impl Signer for PausedSigner {
    fn status(
        &self,
    ) -> BoxFuture<'_, Result<radroots_signing::SignerStatus, radroots_signing::Error>> {
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

pub(super) fn runtime(signer: Arc<PausedSigner>, relay: &str) -> Arc<TeraRuntime> {
    use radroots_sdk::transport::{
        RelayAccess, RelayEndpoint, RelayProfile, RelayProfileKind, RelayUrlPolicy,
    };
    let profile = RelayProfile::explicit(
        RelayProfileKind::Simulator,
        [RelayEndpoint::new(relay, RelayUrlPolicy::Local, RelayAccess::ReadWrite).unwrap()],
    )
    .unwrap();
    Arc::new(
        TeraRuntime::from_client_builder(
            ClientBuilder::memory_default(),
            Some(PublicKey::from_hex(AUTHOR).unwrap()),
            None,
            Some(signer),
            Some(profile),
            None,
        )
        .unwrap(),
    )
}

pub(super) async fn scoped(
    runtime: &TeraRuntime,
    key: u8,
    identifier: &str,
) -> crate::runtime::product_surface::SubmissionReservationRequest {
    use crate::runtime::product_surface::*;
    let scope = ComposerScope::new(
        PublicKey::from_hex(AUTHOR).unwrap(),
        LocalNetworkId::new("nearby".into()).unwrap(),
    );
    let request = SubmissionReservationRequest::new(
        SubmissionCommandId::new([key; 16]).unwrap(),
        scope.clone(),
        ComposerId::new([key + 1; 16]).unwrap(),
        ComposerRevision::INITIAL,
    );
    let mut input = ComposerFormInput::empty(AddCommandType::CreateEvent);
    input.identifier = Some(identifier.into());
    input.title = Some("Scoped calendar".into());
    input.event_timing = Some(Phase1DraftEventTiming::Timed);
    input.event_start_unix_s = Some(1_900_003_600);
    runtime
        .composer_create(
            &scope,
            request.composer_id(),
            ComposerEditSequence::INITIAL,
            ComposerPartialForm::new(input).unwrap(),
        )
        .await
        .unwrap();
    runtime.submission_prepare(&request, vec![]).await.unwrap();
    request
}
