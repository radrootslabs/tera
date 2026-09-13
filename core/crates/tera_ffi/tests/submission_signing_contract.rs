use secp256k1::{Keypair, Message, Secp256k1, SecretKey};
use std::sync::{Arc, Mutex};
use tera_core::runtime::product_surface::{AddCommandType, ComposerFormInput, ComposerPartialForm};
use tera_ffi::*;

mod support;

#[derive(Clone, Copy, Debug)]
enum Fault {
    WrongKey,
    WrongContent,
    InvalidSignature,
    WrongAuthor,
    WrongOperation,
    WrongRequest,
}

struct Host {
    fault: Fault,
    requests: Arc<Mutex<Vec<HostSigningRequest>>>,
}

#[async_trait::async_trait]
impl TeraHostSigner for Host {
    async fn signer_status(&self) -> SignerStatusRecord {
        SignerStatusRecord {
            schema_version: 1,
            availability: SignerAvailabilityRecord::Ready,
        }
    }

    async fn sign(&self, request: HostSigningRequest) -> HostSigningResult {
        self.requests.lock().unwrap().push(request.clone());
        let mut secret = [0; 32];
        secret[31] = if matches!(self.fault, Fault::WrongKey) {
            2
        } else {
            1
        };
        let key =
            Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&secret).unwrap());
        let mut digest: [u8; 32] = request.event_id_digest.clone().try_into().unwrap();
        if matches!(self.fault, Fault::WrongContent) {
            digest = nostr::EventId::new(
                &nostr::PublicKey::from_hex(&request.public_key).unwrap(),
                &nostr::Timestamp::from(request.created_at_unix_s),
                &nostr::Kind::from(u16::try_from(request.kind).unwrap()),
                &nostr::Tags::parse(&request.tags).unwrap(),
                "different content from the committed request",
            )
            .to_bytes();
            assert_ne!(hex::encode(digest), request.expected_event_id);
        }
        let signature =
            Secp256k1::new().sign_schnorr_no_aux_rand(&Message::from_digest(digest), &key);
        let mut result = HostSigningResult {
            schema_version: 1,
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
        };
        match self.fault {
            Fault::InvalidSignature => result.signature_hex = Some("00".repeat(64)),
            Fault::WrongAuthor => result.public_key = "22".repeat(32),
            Fault::WrongOperation => {
                result.operation_id = "11111111-1111-1111-1111-111111111111".into()
            }
            Fault::WrongRequest => result.signer_request_id = "33".repeat(32),
            Fault::WrongKey | Fault::WrongContent => {}
        }
        result
    }
}

fn source() -> FfiComposerSaveRequest {
    let mut form = ComposerFormInput::empty(AddCommandType::CreateUpdate);
    form.content = "PRIVATE immutable native signing request".into();
    FfiComposerSaveRequest {
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
    }
}

#[tokio::test]
async fn untrusted_native_signatures_never_reach_delivery_or_survive_as_signed() {
    for fault in [
        Fault::WrongKey,
        Fault::WrongContent,
        Fault::InvalidSignature,
        Fault::WrongAuthor,
        Fault::WrongOperation,
        Fault::WrongRequest,
    ] {
        let (root, runtime) = support::runtime().await;
        let requests = Arc::new(Mutex::new(Vec::new()));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        runtime
            .configure_simulator_relays(vec![format!("ws://{}", listener.local_addr().unwrap())])
            .unwrap();
        let source = source();
        runtime.composer_save(source.clone()).await.unwrap();
        let request = FfiSubmissionReservationRequest {
            schema_version: 1,
            command_id: submission_reserve_id().unwrap().id,
            scope: source.scope.clone(),
            composer_id: source.id,
            expected_revision: 1,
        };
        let prepared = runtime
            .submission_prepare(request.clone(), vec![])
            .await
            .unwrap();
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let runtime = TeraRuntime::with_host_signer(
            root.path().to_string_lossy().into_owned(),
            support::PUBLIC_KEY.into(),
            support::GENERATION.into(),
            1_800_000_000_000,
            ProtectedDataAvailability::Available,
            Box::new(Host {
                fault,
                requests: requests.clone(),
            }),
        )
        .await
        .unwrap();
        runtime
            .configure_simulator_relays(vec![format!("ws://{}", listener.local_addr().unwrap())])
            .unwrap();
        assert_eq!(
            runtime.submission_status(request.clone()).await.unwrap(),
            prepared
        );
        assert!(
            runtime
                .submission_advance(request.clone(), 1)
                .await
                .is_err(),
            "{fault:?}"
        );
        assert_eq!(requests.lock().unwrap().len(), 1);
        let failed = runtime.submission_status(request.clone()).await.unwrap();
        assert_eq!(failed.captured, prepared.captured);
        assert_eq!(failed.intent_id, prepared.intent_id);
        assert_eq!(failed.operation_id, prepared.operation_id);
        assert_eq!(failed.settlement.signed, 0);
        assert_eq!(failed.settlement.admitted, 0);
        assert_eq!(failed.settlement.delivery_satisfied, 0);
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), listener.accept())
                .await
                .is_err()
        );
        runtime.shutdown().await.unwrap();
        drop(runtime);
        let reopened = TeraRuntime::new(
            root.path().to_string_lossy().into_owned(),
            support::PUBLIC_KEY.into(),
            support::GENERATION.into(),
            1_800_000_000_000,
            ProtectedDataAvailability::Available,
        )
        .await
        .unwrap();
        assert_eq!(reopened.submission_status(request).await.unwrap(), failed);
        reopened.shutdown().await.unwrap();
    }
}
