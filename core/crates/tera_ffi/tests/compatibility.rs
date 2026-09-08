//! Frozen compatibility across application FFI history transfer. All stores are
//! isolated fixtures; signer keys are generated in memory and never serialized.

use secp256k1::{Keypair, Message, Secp256k1};
use serde_json::Value;
use std::path::Path;
use tera_ffi::{
    FfiAddCommandType, FfiAddDraftInput, FfiCancellationPolicy, FfiDraftStatusRecord,
    FfiQueuePolicyRecord, FfiRelaySatisfaction, HostSigningOutcome, HostSigningRequest,
    HostSigningResult, MOBILE_FFI_SCHEMA_VERSION, ProtectedDataAvailability, RadrootsHostSigner,
    RadrootsRuntime, SignerAvailabilityRecord, SignerStatusRecord,
};

fn fixture() -> Value {
    serde_json::from_str(include_str!(
        "../../../../test-fixtures/tera-compatibility.v1.json"
    ))
    .expect("checked synthetic fixture")
}

fn field<'a>(fixture: &'a Value, key: &str) -> &'a str {
    fixture[key].as_str().expect("fixture string")
}

fn input(fixture: &Value) -> FfiAddDraftInput {
    FfiAddDraftInput {
        schema_version: MOBILE_FFI_SCHEMA_VERSION,
        command_type: FfiAddCommandType::CreateUpdate,
        content: field(fixture, "content").to_owned(),
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
    }
}

async fn runtime(root: &Path, public_key: &str, signer: Option<Keypair>) -> RadrootsRuntime {
    std::fs::create_dir_all(root.join("radroots/users").join(public_key))
        .expect("isolated application owner directory");
    let fixture = fixture();
    let generation = field(&fixture["queued_update"], "source_generation").to_owned();
    if let Some(keypair) = signer {
        RadrootsRuntime::with_host_signer(
            root.to_string_lossy().into_owned(),
            public_key.to_owned(),
            generation,
            1_800_000_000_000,
            ProtectedDataAvailability::Available,
            Box::new(EphemeralSigner(keypair)),
        )
        .await
        .expect("runtime with ephemeral fixture signer")
    } else {
        RadrootsRuntime::new(
            root.to_string_lossy().into_owned(),
            public_key.to_owned(),
            generation,
            1_800_000_000_000,
            ProtectedDataAvailability::Available,
        )
        .await
        .expect("runtime using current persistent reader")
    }
}

async fn queue(runtime: &RadrootsRuntime, fixture: &Value) -> FfiDraftStatusRecord {
    let id = field(fixture, "draft_id");
    let persisted = fixture["persisted_at_unix_ms"].as_u64().unwrap();
    let saved = runtime
        .phase1_save_draft(
            id.to_owned(),
            input(fixture),
            fixture["authored_at_unix_s"].as_u64().unwrap(),
            None,
            persisted,
        )
        .await
        .expect("save synthetic draft through real FFI");
    let policy = FfiQueuePolicyRecord {
        schema_version: MOBILE_FFI_SCHEMA_VERSION,
        relay_urls: fixture["relay_urls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap().to_owned())
            .collect(),
        satisfaction: FfiRelaySatisfaction::AllAccepted,
        delivery_deadline_unix_ms: fixture["delivery_deadline_unix_ms"].as_u64().unwrap(),
        cancellation: FfiCancellationPolicy::LocalCooperative,
    };
    runtime
        .phase1_queue_draft(id.to_owned(), saved.revision, policy, persisted + 1)
        .await
        .expect("queue locally without starting relay delivery")
}

#[tokio::test]
async fn queued_update_reopens_with_frozen_operation_and_card_identity() {
    let fixture = fixture()["queued_update"].clone();
    let root = tempfile::tempdir().unwrap();
    let public_key = field(&fixture, "public_key");
    let first = runtime(root.path(), public_key, None).await;
    let queued = queue(&first, &fixture).await;
    assert_eq!(
        queued.operation_id.as_deref(),
        Some(field(&fixture, "expected_operation_id"))
    );
    assert_eq!(queued.card_id, field(&fixture, "expected_card_id"));
    first.shutdown().await.unwrap();
    drop(first);
    let reopened = runtime(root.path(), public_key, None).await;
    let restored = reopened
        .phase1_draft_status(field(&fixture, "draft_id").to_owned())
        .await
        .expect("existing persisted draft reader");
    assert_eq!(restored, queued);
    let recovered = reopened
        .phase1_recover_draft_queue(field(&fixture, "draft_id").to_owned(), 1_900_000_000_002)
        .await
        .expect("existing queued operation reader");
    assert_eq!(recovered, queued);
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn signed_operation_reopens_without_replacing_its_author_or_identity() {
    let fixture = fixture()["queued_update"].clone();
    let root = tempfile::tempdir().unwrap();
    let keypair = Keypair::new(&Secp256k1::new(), &mut secp256k1::rand::thread_rng());
    let public_key = keypair.x_only_public_key().0.to_string();
    let first = runtime(root.path(), &public_key, Some(keypair)).await;
    let queued = queue(&first, &fixture).await;
    let signed = first
        .phase1_sign_queued_draft(queued.draft_id.clone(), queued.revision)
        .await
        .expect("sign and durably admit the fixed operation");
    assert_eq!(signed.operation_id, queued.operation_id);
    assert_eq!(signed.card_id, queued.card_id);
    assert_eq!(signed.author_public_key, public_key);
    assert_eq!(signed.settlement.unwrap().signed, 1);
    first.shutdown().await.unwrap();
    drop(first);
    // Reopening has no signer. Reading must preserve the already signed facts.
    let reopened = runtime(root.path(), &public_key, None).await;
    let restored = reopened
        .phase1_draft_status(signed.draft_id.clone())
        .await
        .unwrap();
    assert_eq!(restored, signed);
    reopened.shutdown().await.unwrap();
}

struct EphemeralSigner(Keypair);

#[async_trait::async_trait]
impl RadrootsHostSigner for EphemeralSigner {
    async fn signer_status(&self) -> SignerStatusRecord {
        SignerStatusRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            availability: SignerAvailabilityRecord::Ready,
        }
    }

    async fn sign(&self, request: HostSigningRequest) -> HostSigningResult {
        assert_eq!(request.public_key, self.0.x_only_public_key().0.to_string());
        let digest: [u8; 32] = request
            .event_id_digest
            .try_into()
            .expect("exact event digest");
        let signature =
            Secp256k1::new().sign_schnorr_no_aux_rand(&Message::from_digest(digest), &self.0);
        HostSigningResult {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            outcome: HostSigningOutcome::Signed,
            operation_id: request.operation_id,
            signer_request_id: request.signer_request_id,
            public_key: request.public_key,
            purpose: request.purpose,
            signature_hex: Some(signature.to_string()),
            completed_at_unix_ms: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis()
                .try_into()
                .unwrap(),
        }
    }
}
