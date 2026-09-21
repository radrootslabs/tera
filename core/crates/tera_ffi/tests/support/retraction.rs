//! Real locally admitted source for the native delegation test; no remote I/O.
use secp256k1::{Keypair, Message, Secp256k1, SecretKey};
use tera_ffi::*;

struct FixtureSigner;

#[async_trait::async_trait]
impl TeraHostSigner for FixtureSigner {
    async fn signer_status(&self) -> SignerStatusRecord {
        SignerStatusRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            availability: SignerAvailabilityRecord::Ready,
        }
    }

    async fn sign(&self, request: HostSigningRequest) -> HostSigningResult {
        let mut bytes = [0; 32];
        bytes[31] = 1;
        let keypair =
            Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&bytes).unwrap());
        assert_eq!(
            request.public_key,
            keypair.x_only_public_key().0.to_string()
        );
        let digest = request.event_id_digest.try_into().unwrap();
        let signature =
            Secp256k1::new().sign_schnorr_no_aux_rand(&Message::from_digest(digest), &keypair);
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

pub async fn seed(root: &std::path::Path, input: FfiAddDraftInput) -> (String, String) {
    use tera_core::runtime::product_surface::{CreateUpdate, Phase1AddCommand};
    let at = 1_700_000_000;
    let event = Phase1AddCommand::CreateUpdate(CreateUpdate::new(&input.content).unwrap())
        .authored_plan(at, super::support::PUBLIC_KEY)
        .unwrap()
        .expected_event_id()
        .to_hex();
    let runtime = TeraRuntime::with_host_signer(
        root.to_string_lossy().into_owned(),
        super::support::PUBLIC_KEY.into(),
        super::support::GENERATION.into(),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
        Box::new(FixtureSigner),
    )
    .await
    .unwrap();
    // A deliberately closed ephemeral loopback port bounds delivery; admission is local.
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay = format!("ws://{}", listener.local_addr().unwrap());
    drop(listener);
    runtime
        .configure_simulator_relays(vec![relay.clone()])
        .await
        .unwrap();
    let id = "ee".repeat(16);
    let saved = runtime
        .phase1_save_draft(id.clone(), input, at, None, at * 1000)
        .await
        .unwrap();
    let queued = runtime
        .phase1_queue_draft(
            id.clone(),
            saved.revision,
            FfiQueuePolicyRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                relay_urls: vec![relay],
                satisfaction: FfiRelaySatisfaction::AllAccepted,
                delivery_deadline_unix_ms: 2_000_000_000_000,
                cancellation: FfiCancellationPolicy::LocalCooperative,
            },
            at * 1000 + 1,
        )
        .await
        .unwrap();
    let _delivery = runtime
        .phase1_advance_draft(id.clone(), queued.revision)
        .await;
    let source = runtime.phase1_draft_status(id).await.unwrap();
    assert_eq!(source.settlement.unwrap().admitted, 1);
    runtime.shutdown().await.unwrap();
    (source.card_id, event)
}
