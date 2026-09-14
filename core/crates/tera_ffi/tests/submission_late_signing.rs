use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use nostr_relay_builder::MockRelay;
use secp256k1::{Keypair, Message, Secp256k1, SecretKey};
use tera_core::runtime::product_surface::{AddCommandType, ComposerFormInput, ComposerPartialForm};
use tera_ffi::*;
use tokio::sync::Notify;

mod support;

#[derive(Clone, Default)]
struct HeldHost {
    requests: Arc<Mutex<Vec<HostSigningRequest>>>,
    entered: Arc<Notify>,
    resume: Arc<Notify>,
    completion_unavailable: bool,
}

#[async_trait::async_trait]
impl TeraHostSigner for HeldHost {
    async fn signer_status(&self) -> SignerStatusRecord {
        SignerStatusRecord {
            schema_version: 1,
            availability: SignerAvailabilityRecord::Ready,
        }
    }

    async fn sign(&self, request: HostSigningRequest) -> HostSigningResult {
        self.requests.lock().unwrap().push(request.clone());
        self.entered.notify_one();
        self.resume.notified().await;
        let mut secret = [0; 32];
        secret[31] = 1;
        let key =
            Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&secret).unwrap());
        let signature = Secp256k1::new().sign_schnorr_no_aux_rand(
            &Message::from_digest(request.event_id_digest.clone().try_into().unwrap()),
            &key,
        );
        HostSigningResult {
            schema_version: 1,
            outcome: HostSigningOutcome::Signed,
            operation_id: request.operation_id,
            signer_request_id: request.signer_request_id,
            public_key: request.public_key,
            purpose: request.purpose,
            signature_hex: Some(signature.to_string()),
            completed_at_unix_ms: if self.completion_unavailable {
                0
            } else {
                now()
            },
        }
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis()
        .try_into()
        .unwrap()
}

async fn prepare(
    runtime: &TeraRuntime,
) -> (
    FfiSubmissionReservationRequest,
    FfiSubmissionOperationRecord,
) {
    let mut form = ComposerFormInput::empty(AddCommandType::CreateUpdate);
    form.content = "PRIVATE retained authored result".into();
    let source = FfiComposerSaveRequest {
        schema_version: 1,
        scope: FfiComposerScopeRecord {
            schema_version: 1,
            author_public_key: support::PUBLIC_KEY.into(),
            local_network_id: "nearby".into(),
        },
        id: composer_reserve_id().unwrap().id,
        expected_revision: None,
        edit_sequence: 1,
        form: (&ComposerPartialForm::new(form).unwrap()).into(),
    };
    runtime.composer_save(source.clone()).await.unwrap();
    let request = FfiSubmissionReservationRequest {
        schema_version: 1,
        command_id: submission_reserve_id().unwrap().id,
        scope: source.scope,
        composer_id: source.id,
        expected_revision: 1,
    };
    let prepared = runtime
        .submission_prepare(request.clone(), vec![])
        .await
        .unwrap();
    (request, prepared)
}

async fn run_case(completion_unavailable: bool) {
    let relay = MockRelay::run().await.unwrap();
    let relay_url = relay.url().await.to_string();
    let (root, initial) = support::runtime().await;
    initial.shutdown().await.unwrap();
    drop(initial);
    let host = HeldHost {
        completion_unavailable,
        ..Default::default()
    };
    let runtime = Arc::new(
        TeraRuntime::with_host_signer(
            root.path().to_string_lossy().into_owned(),
            support::PUBLIC_KEY.into(),
            support::GENERATION.into(),
            1_800_000_000_000,
            ProtectedDataAvailability::Available,
            Box::new(host.clone()),
        )
        .await
        .unwrap(),
    );
    runtime
        .configure_simulator_relays(vec![relay_url.clone()])
        .await
        .unwrap();
    let (request, prepared) = prepare(&runtime).await;
    let mut task = {
        let runtime = runtime.clone();
        let request = request.clone();
        tokio::spawn(async move { runtime.submission_advance(request, 1).await })
    };
    tokio::time::timeout(Duration::from_secs(5), host.entered.notified())
        .await
        .unwrap();
    // A caller wait timeout must not be treated as a failed signing operation.
    assert!(
        tokio::time::timeout(Duration::from_millis(10), &mut task)
            .await
            .is_err()
    );
    let signing = runtime.submission_status(request.clone()).await.unwrap();
    assert_eq!(signing.settlement.signed, 0);
    assert_eq!(signing.captured, prepared.captured);
    assert!(
        runtime
            .submission_advance(request.clone(), signing.revision)
            .await
            .is_err()
    );
    let original = host.requests.lock().unwrap()[0].clone();
    if !completion_unavailable {
        let remaining = original.deadline_unix_ms.saturating_sub(now());
        assert!(remaining <= 30_000);
        tokio::time::sleep(Duration::from_millis(remaining + 10)).await;
        assert!(now() >= original.deadline_unix_ms);
    }
    host.resume.notify_one();
    let result = tokio::time::timeout(Duration::from_secs(10), task)
        .await
        .unwrap()
        .unwrap();
    if !completion_unavailable {
        assert!(result.is_err());
    }
    let retained = runtime.submission_status(request.clone()).await.unwrap();
    assert_eq!(retained.settlement.signed, 1);
    if !completion_unavailable {
        assert_eq!(retained.settlement.admitted, 0);
    }
    assert_eq!(retained.captured, prepared.captured);
    assert_eq!(retained.operation_id, prepared.operation_id);
    assert_eq!(host.requests.lock().unwrap().len(), 1);
    runtime.shutdown().await.unwrap();
    drop(runtime);

    // Reopen with no signer at all. Durable evidence permits admission and delivery.
    let recovered = TeraRuntime::new(
        root.path().to_string_lossy().into_owned(),
        support::PUBLIC_KEY.into(),
        support::GENERATION.into(),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .await
    .unwrap();
    recovered
        .configure_simulator_relays(vec![relay_url.clone()])
        .await
        .unwrap();
    assert_eq!(
        recovered.submission_status(request.clone()).await.unwrap(),
        retained
    );
    let delivered = recovered
        .submission_advance(request.clone(), retained.revision)
        .await
        .unwrap();
    assert_eq!(delivered.settlement.signed, 1);
    assert_eq!(delivered.settlement.admitted, 1);
    assert_eq!(delivered.settlement.delivery_satisfied, 1);
    assert_eq!(
        recovered
            .submission_advance(request.clone(), delivered.revision)
            .await
            .unwrap(),
        delivered
    );
    assert_eq!(host.requests.lock().unwrap().len(), 1);

    let observer = nostr_sdk::Client::default();
    observer.add_relay(relay_url).await.unwrap();
    observer.connect().await;
    let events = observer
        .fetch_events(
            nostr::Filter::new().id(nostr::EventId::from_hex(&original.expected_event_id).unwrap()),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    let event = events.into_iter().next().unwrap();
    event.verify().unwrap();
    assert_eq!(event.content, original.content);
    assert_eq!(event.created_at.as_secs(), original.created_at_unix_s);
    assert_eq!(event.pubkey.to_hex(), original.public_key);
    observer.shutdown().await;
    recovered.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn late_native_fact_survives_deadline_restart_and_duplicate_without_credentials() {
    run_case(false).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn native_signed_evidence_does_not_require_a_host_completion_timestamp() {
    run_case(true).await;
}
