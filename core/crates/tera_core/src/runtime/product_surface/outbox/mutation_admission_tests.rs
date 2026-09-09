use std::sync::{
    Mutex, Weak,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use std::time::Duration;

use radroots_nostr::signing::LocalSigner;
use radroots_signing::{Signer, signer::BoxFuture};
use tokio::sync::Notify;

use super::*;
use crate::runtime::product_surface::CreateUpdate;

struct PausedSigner {
    inner: LocalSigner,
    pause_status: AtomicBool,
    pause_sign: AtomicBool,
    entered: Notify,
    resume: Notify,
    statuses: AtomicUsize,
    signatures: AtomicUsize,
    reentrant: Mutex<Option<ReentrantCancellation>>,
}

struct ReentrantCancellation {
    runtime: Weak<TeraRuntime>,
    id: [u8; 16],
    revision: u64,
}

impl PausedSigner {
    fn new(pause_status: bool, pause_sign: bool) -> Arc<Self> {
        Arc::new(Self {
            inner: LocalSigner::new(
                radroots_nostr::key::SecretKey::parse(
                    "0000000000000000000000000000000000000000000000000000000000000001",
                )
                .unwrap(),
            )
            .unwrap(),
            pause_status: AtomicBool::new(pause_status),
            pause_sign: AtomicBool::new(pause_sign),
            entered: Notify::new(),
            resume: Notify::new(),
            statuses: AtomicUsize::new(0),
            signatures: AtomicUsize::new(0),
            reentrant: Mutex::new(None),
        })
    }

    async fn wait(&self) {
        tokio::time::timeout(Duration::from_secs(5), self.entered.notified())
            .await
            .expect("signer must reach the explicit gate");
    }
}

impl Signer for PausedSigner {
    fn status(
        &self,
    ) -> BoxFuture<'_, Result<radroots_signing::SignerStatus, radroots_signing::Error>> {
        Box::pin(async {
            self.statuses.fetch_add(1, Ordering::SeqCst);
            let callback = self.reentrant.lock().unwrap().take();
            if let Some(ReentrantCancellation {
                runtime,
                id,
                revision,
            }) = callback
            {
                assert_eq!(
                    runtime
                        .upgrade()
                        .unwrap()
                        .phase1_cancel_draft(id, revision, 1_900_000_000_000)
                        .await
                        .unwrap_err(),
                    Phase1DraftError::OperationInProgress
                );
            }
            if self.pause_status.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                self.resume.notified().await;
            }
            self.inner.status().await
        })
    }

    fn sign(
        &self,
        request: radroots_signing::SignRequest,
    ) -> BoxFuture<'_, Result<radroots_signing::SignReceipt, radroots_signing::Error>> {
        Box::pin(async move {
            self.signatures.fetch_add(1, Ordering::SeqCst);
            if self.pause_sign.swap(false, Ordering::SeqCst) {
                self.entered.notify_one();
                self.resume.notified().await;
            }
            self.inner.sign(request).await
        })
    }
}

fn runtime(signer: Arc<PausedSigner>) -> Arc<TeraRuntime> {
    Arc::new(
        TeraRuntime::from_client_builder(
            radroots_sdk::ClientBuilder::memory_default(),
            Some(
                PublicKey::from_hex(
                    "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
                )
                .unwrap(),
            ),
            None,
            Some(signer),
            None,
            None,
        )
        .unwrap(),
    )
}

async fn queued(runtime: &TeraRuntime, id: [u8; 16]) -> Phase1DraftStatus {
    let saved = runtime
        .phase1_save_draft(
            id,
            Phase1AddCommand::CreateUpdate(CreateUpdate::new("An admitted update").unwrap()),
            1_900_000_000,
            Vec::new(),
            None,
            1_900_000_000_000,
        )
        .await
        .unwrap();
    runtime
        .phase1_queue_draft(
            id,
            saved.draft().revision().get(),
            Phase1QueuePolicy::new(
                vec!["wss://relay.example".to_owned()],
                Phase1RelaySatisfaction::AllAccepted,
                2_000_000_000_000,
                Phase1CancellationPolicy::LocalCooperative,
            )
            .unwrap(),
            1_900_000_000_001,
        )
        .await
        .unwrap()
}

fn signing(
    runtime: &Arc<TeraRuntime>,
    id: [u8; 16],
    revision: u64,
) -> tokio::task::JoinHandle<Result<Phase1DraftStatus, Phase1DraftError>> {
    let runtime = Arc::clone(runtime);
    tokio::spawn(async move { runtime.phase1_sign_queued_draft(id, revision).await })
}

#[tokio::test]
async fn mutation_admission_precedes_signer_callbacks_and_preserves_independent_drafts() {
    let signer = PausedSigner::new(true, false);
    let runtime = runtime(Arc::clone(&signer));
    let first = queued(&runtime, [1; 16]).await;
    let second = queued(&runtime, [2; 16]).await;
    let revision = first.draft().revision().get();
    let owner = signing(&runtime, [1; 16], revision);
    signer.wait().await;
    assert_eq!(
        runtime
            .phase1_sign_queued_draft([1; 16], revision)
            .await
            .unwrap_err(),
        Phase1DraftError::OperationInProgress
    );
    assert_eq!(
        runtime
            .phase1_advance_draft([1; 16], revision)
            .await
            .unwrap_err(),
        Phase1DraftError::OperationInProgress
    );
    assert_eq!(
        runtime
            .phase1_cancel_draft([1; 16], revision, 1_900_000_000_002)
            .await
            .unwrap_err(),
        Phase1DraftError::OperationInProgress
    );
    assert_eq!(signer.statuses.load(Ordering::SeqCst), 1);
    let independent = runtime
        .phase1_sign_queued_draft([2; 16], second.draft().revision().get())
        .await
        .unwrap();
    assert!(independent.push().unwrap().artifact().signed().is_some());
    signer.resume.notify_one();
    let receipt = owner.await.unwrap().unwrap();
    assert!(receipt.push().unwrap().artifact().signed().is_some());
    let replay = runtime
        .phase1_sign_queued_draft([1; 16], revision)
        .await
        .unwrap();
    assert_eq!(
        replay.push().unwrap().artifact(),
        receipt.push().unwrap().artifact()
    );
    assert_eq!(signer.signatures.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn mutation_admission_releases_cancelled_preclaim_work_without_leaking_the_scope() {
    let signer = PausedSigner::new(true, false);
    let runtime = runtime(Arc::clone(&signer));
    let status = queued(&runtime, [3; 16]).await;
    let revision = status.draft().revision().get();
    // An unpolled, queued future has not admitted work or called the signer.
    drop(runtime.phase1_sign_queued_draft([3; 16], revision));
    assert_eq!(signer.statuses.load(Ordering::SeqCst), 0);
    let owner = signing(&runtime, [3; 16], revision);
    signer.wait().await;
    owner.abort();
    assert!(owner.await.unwrap_err().is_cancelled());
    assert_eq!(signer.signatures.load(Ordering::SeqCst), 0);
    let receipt = runtime
        .phase1_sign_queued_draft([3; 16], revision)
        .await
        .unwrap();
    assert!(receipt.push().unwrap().artifact().signed().is_some());
    assert_eq!(signer.signatures.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn mutation_admission_cancellation_preserves_the_durable_signing_claim() {
    let signer = PausedSigner::new(false, true);
    let runtime = runtime(Arc::clone(&signer));
    let status = queued(&runtime, [4; 16]).await;
    let revision = status.draft().revision().get();
    let owner = signing(&runtime, [4; 16], revision);
    signer.wait().await;
    let before = runtime.phase1_draft_status([4; 16]).await.unwrap();
    assert!(before.push().unwrap().artifact().signing_claim().is_some());
    owner.abort();
    assert!(owner.await.unwrap_err().is_cancelled());
    let after = runtime.phase1_draft_status([4; 16]).await.unwrap();
    assert_eq!(
        after.push().unwrap().artifact(),
        before.push().unwrap().artifact()
    );
    assert_eq!(
        runtime
            .phase1_sign_queued_draft([4; 16], revision)
            .await
            .unwrap_err(),
        Phase1DraftError::Operation
    );
    assert_eq!(signer.signatures.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn mutation_admission_reentrant_signer_cannot_cancel_its_own_transition() {
    let signer = PausedSigner::new(false, false);
    let runtime = runtime(Arc::clone(&signer));
    let status = queued(&runtime, [5; 16]).await;
    let revision = status.draft().revision().get();
    *signer.reentrant.lock().unwrap() = Some(ReentrantCancellation {
        runtime: Arc::downgrade(&runtime),
        id: [5; 16],
        revision,
    });
    let receipt = runtime
        .phase1_sign_queued_draft([5; 16], revision)
        .await
        .unwrap();
    assert_ne!(receipt.draft().stage(), AuthoredDraftStage::Cancelled);
    assert!(receipt.push().unwrap().artifact().signed().is_some());
    assert_eq!(signer.signatures.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn mutation_admission_upload_authorization_uses_the_existing_operation_identity() {
    use radroots_blossom::authorization::{AuthorizationContent, ServerDomain};
    let signer = PausedSigner::new(false, true);
    let runtime = runtime(Arc::clone(&signer));
    let claim = AuthoredUploadClaim::new(
        AuthorizationContent::parse("Upload exact Tera image").unwrap(),
        ServerDomain::parse("media.example").unwrap(),
        radroots_blossom::Sha256::digest(b"exact upload bytes"),
        1_900_000_000,
        60,
    )
    .unwrap();
    let owner = {
        let runtime = Arc::clone(&runtime);
        let claim = claim.clone();
        tokio::spawn(async move {
            runtime
                .phase1_authorize_blossom_upload(
                    [6; 16],
                    [7; 16],
                    claim,
                    u64::MAX,
                    Phase1CancellationPolicy::LocalCooperative,
                )
                .await
        })
    };
    signer.wait().await;
    // Both exact replay and a conflicting artifact share one operation scope.
    for artifact in [[7; 16], [8; 16]] {
        assert_eq!(
            runtime
                .phase1_authorize_blossom_upload(
                    [6; 16],
                    artifact,
                    claim.clone(),
                    u64::MAX,
                    Phase1CancellationPolicy::LocalCooperative
                )
                .await
                .unwrap_err(),
            Phase1DraftError::OperationInProgress
        );
    }
    signer.resume.notify_one();
    assert!(!owner.await.unwrap().unwrap().as_str().is_empty());
    assert_eq!(signer.signatures.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn shutdown_drains_signing_and_cancelled_close_never_reopens_admission() {
    use crate::runtime::lifecycle::RuntimeLifecycleError;
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    let signer = PausedSigner::new(false, true);
    let runtime = runtime(Arc::clone(&signer));
    let status = queued(&runtime, [8; 16]).await;
    let revision = status.draft().revision().get();
    let owner = signing(&runtime, [8; 16], revision);
    signer.wait().await;
    let mut close = Box::pin(runtime.shutdown());
    assert!(matches!(
        close.as_mut().poll(&mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    assert!(!runtime.client.is_closed());
    assert!(runtime.info().app.shutting_down);
    let crate::TeraAppError::Sdk { report } = runtime.shutdown().await.unwrap_err() else {
        panic!("typed close failure");
    };
    assert_eq!(report.code, "client_close_in_progress");
    assert_eq!(
        runtime
            .phase1_sign_queued_draft([8; 16], revision)
            .await
            .unwrap_err(),
        Phase1DraftError::Lifecycle(RuntimeLifecycleError::Closing)
    );
    assert!(matches!(
        runtime.phase1_settings().await,
        Err(super::super::SettingsError::Lifecycle(
            RuntimeLifecycleError::Closing
        ))
    ));
    assert!(runtime.sdk_storage_status().await.is_err());
    drop(close);
    assert_eq!(
        runtime.phase1_draft_heads(10).await.unwrap_err(),
        Phase1DraftError::Lifecycle(RuntimeLifecycleError::Closing)
    );
    signer.resume.notify_one();
    let signed = owner.await.unwrap().unwrap();
    assert!(signed.push().unwrap().artifact().signed().is_some());
    assert_eq!(signer.signatures.load(Ordering::SeqCst), 1);
    assert!(!runtime.shutdown().await.unwrap().already_closed);
    assert!(runtime.shutdown().await.unwrap().already_closed);
    assert_eq!(
        runtime.phase1_draft_status([8; 16]).await.unwrap_err(),
        Phase1DraftError::Lifecycle(RuntimeLifecycleError::Closed)
    );
}

#[tokio::test]
async fn shutdown_of_an_unpolled_future_does_not_close_command_admission() {
    let runtime = runtime(PausedSigner::new(false, false));
    drop(runtime.shutdown());
    assert!(!runtime.info().app.shutting_down);
    assert!(runtime.phase1_settings().await.is_ok());
    assert_eq!(runtime.shutdown().await.unwrap().state, "closed");
}
