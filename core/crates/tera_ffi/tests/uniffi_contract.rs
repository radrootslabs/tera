use radroots_mobile_ffi::{
    FfiAddCommandType, FfiAddDraftInput, FfiCancellationPolicy, FfiMediaOperation,
    FfiQueuePolicyRecord, FfiRelaySatisfaction, FfiTradeEvidenceCoverage, FfiTradeEvidenceOutcome,
    HostSigningOutcome, HostSigningRequest, HostSigningResult, MOBILE_FFI_SCHEMA_VERSION,
    ProtectedDataAvailability, RadrootsAppError, RadrootsHostSigner, RadrootsRuntime,
    SignerAvailabilityRecord, SignerStatusRecord,
};
use secp256k1::{Keypair, Message, Secp256k1, SecretKey};
use std::sync::{Arc, Mutex};

mod support;

struct TestHostSigner(Arc<Mutex<HostSigningOutcome>>);

#[async_trait::async_trait]
impl RadrootsHostSigner for TestHostSigner {
    async fn signer_status(&self) -> SignerStatusRecord {
        SignerStatusRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            availability: SignerAvailabilityRecord::Ready,
        }
    }

    async fn sign(&self, request: HostSigningRequest) -> HostSigningResult {
        let outcome = *self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let signature_hex = (outcome == HostSigningOutcome::Signed).then(|| {
            let mut secret_bytes = [0; 32];
            secret_bytes[31] = 1;
            let secret = SecretKey::from_slice(&secret_bytes).expect("fixture secret key");
            let keypair = Keypair::from_secret_key(&Secp256k1::new(), &secret);
            let digest: [u8; 32] = request
                .event_id_digest
                .clone()
                .try_into()
                .expect("32-byte event ID digest");
            assert_eq!(hex::encode(digest), request.expected_event_id);
            assert_eq!(
                keypair.x_only_public_key().0.to_string(),
                request.public_key
            );
            let message = Message::from_digest(digest);
            let signature = Secp256k1::new().sign_schnorr_no_aux_rand(&message, &keypair);
            Secp256k1::new()
                .verify_schnorr(&signature, &message, &keypair.x_only_public_key().0)
                .expect("fixture host signature verifies");
            signature.to_string()
        });
        let completed_at_unix_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock after epoch")
            .as_millis()
            .try_into()
            .expect("current time fits u64");
        assert!(completed_at_unix_ms < request.deadline_unix_ms);
        HostSigningResult {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            outcome,
            operation_id: request.operation_id,
            signer_request_id: request.signer_request_id,
            public_key: request.public_key,
            purpose: request.purpose,
            signature_hex,
            completed_at_unix_ms,
        }
    }
}

#[test]
fn swift_module_names_preserve_the_host_contract() {
    let config = include_str!("../uniffi.toml");
    assert_eq!(
        config,
        "[bindings.swift]\nmodule_name = \"RadrootsKitBindings\"\nffi_module_name = \"RadrootsFFI\"\n"
    );
}

#[test]
fn final_evidence_vocabularies_are_exact_at_the_mobile_boundary() {
    assert_eq!(
        [
            FfiTradeEvidenceCoverage::Missing,
            FfiTradeEvidenceCoverage::Partial,
            FfiTradeEvidenceCoverage::ScopeSatisfied,
            FfiTradeEvidenceCoverage::Unsupported,
        ]
        .len(),
        4
    );
    assert_eq!(
        [
            FfiTradeEvidenceOutcome::Valid,
            FfiTradeEvidenceOutcome::Invalid,
            FfiTradeEvidenceOutcome::Indeterminate,
        ]
        .len(),
        3
    );
}

#[test]
fn media_cancellation_handle_owns_one_stable_opaque_operation_identity() {
    let operation = FfiMediaOperation::new().expect("media operation");
    let operation_id = operation.operation_id();
    assert_eq!(operation_id.len(), 32);
    assert!(!operation.is_cancelled());
    operation.cancel();
    assert!(operation.is_cancelled());
    assert_eq!(operation.operation_id(), operation_id);
}

#[tokio::test]
async fn protected_data_failure_is_typed_and_opens_no_store() {
    let root = tempfile::tempdir().expect("tempdir");
    support::prepare(root.path());
    let result = RadrootsRuntime::new(
        root.path().to_string_lossy().into_owned(),
        support::PUBLIC_KEY.to_owned(),
        support::GENERATION.to_owned(),
        1_800_000_000_000,
        ProtectedDataAvailability::Unavailable,
    )
    .await;
    let Err(RadrootsAppError::Failure { report }) = result else {
        panic!("protected data failure must remain typed across UniFFI");
    };
    assert_eq!(report.code, "protected_data_unavailable");
    assert!(report.retryable);
    assert!(
        !root
            .path()
            .join("radroots/users")
            .join(support::PUBLIC_KEY)
            .join("runtime.sqlite")
            .exists()
    );
}

#[tokio::test]
async fn final_mobile_abi_uses_async_sdk_dtos_and_versioned_errors() {
    let (_root, runtime) = support::runtime().await;
    let storage = runtime.sdk_storage_status().await.expect("storage status");
    assert_eq!(storage.backend, "sqlite");

    runtime.shutdown().await.expect("shutdown");
    let error = runtime
        .sdk_storage_status()
        .await
        .expect_err("closed client must reject operations");
    let RadrootsAppError::Failure { report } = error;
    assert_eq!(report.schema_version, 1);
    assert_eq!(report.code, "client_closed");
    assert_eq!(report.category, "runtime");
    assert!(!report.retryable);
    assert_eq!(report.safe_message, "SDK client is closed");
}

#[tokio::test]
async fn host_signer_constructor_exposes_only_an_opaque_configured_boundary() {
    let root = tempfile::tempdir().expect("tempdir");
    support::prepare(root.path());
    let outcome = Arc::new(Mutex::new(HostSigningOutcome::Rejected));
    let runtime = RadrootsRuntime::with_host_signer(
        root.path().to_string_lossy().into_owned(),
        support::PUBLIC_KEY.to_owned(),
        support::GENERATION.to_owned(),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
        Box::new(TestHostSigner(Arc::clone(&outcome))),
    )
    .await
    .expect("runtime with host signer");

    let identity = runtime.identity_status().expect("identity status");
    assert_eq!(identity.public_key, support::PUBLIC_KEY);
    assert!(identity.host_signer_configured);

    for (index, host_outcome) in [
        HostSigningOutcome::Signed,
        HostSigningOutcome::Locked,
        HostSigningOutcome::Cancelled,
        HostSigningOutcome::Rejected,
        HostSigningOutcome::TimedOut,
        HostSigningOutcome::Unavailable,
        HostSigningOutcome::Invalidated,
        HostSigningOutcome::Failed,
    ]
    .into_iter()
    .enumerate()
    {
        *outcome
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = host_outcome;
        let draft_id = format!("{:02x}", index + 17).repeat(16);
        let saved = runtime
            .phase1_save_draft(
                draft_id.clone(),
                FfiAddDraftInput {
                    schema_version: MOBILE_FFI_SCHEMA_VERSION,
                    command_type: FfiAddCommandType::CreateUpdate,
                    content: format!("Signer boundary test {index}"),
                    identifier: None,
                    title: None,
                    summary: None,
                    location: None,
                    event_timing: None,
                    event_start_date: None,
                    event_end_date: None,
                    event_start_unix_s: None,
                    event_end_unix_s: None,
                    event_timezone: None,
                    price_amount: None,
                    currency: None,
                    unit: None,
                    quantity: None,
                    food_published_at_unix_s: None,
                    food_status: None,
                    media: Vec::new(),
                },
                1_800_000_000 + index as u64,
                None,
                1_800_000_000_000 + index as u64,
            )
            .await
            .expect("saved draft");
        let queued = runtime
            .phase1_queue_draft(
                draft_id.clone(),
                saved.revision,
                FfiQueuePolicyRecord {
                    schema_version: MOBILE_FFI_SCHEMA_VERSION,
                    relay_urls: vec!["wss://relay.example".to_owned()],
                    satisfaction: FfiRelaySatisfaction::AnyAccepted,
                    delivery_deadline_unix_ms: u64::MAX,
                    cancellation: FfiCancellationPolicy::PreservePublishedRequest,
                },
                1_800_000_001_000 + index as u64,
            )
            .await
            .expect("queued draft");
        let signing_result = runtime
            .phase1_sign_queued_draft(draft_id, queued.revision)
            .await;
        if host_outcome == HostSigningOutcome::Signed {
            assert_eq!(
                signing_result.expect("valid host signature").state,
                radroots_mobile_ffi::FfiOutboxState::Signed
            );
        } else {
            let signing_error = signing_result.expect_err("host failure remains typed");
            assert_eq!(signing_error.report().category, "authoring");
            assert!(!signing_error.report().safe_message.contains("signature"));
        }
    }
    runtime.shutdown().await.expect("shutdown");
}
