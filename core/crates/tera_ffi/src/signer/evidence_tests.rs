use super::*;
use radroots_blossom::authorization::{AuthoredUploadClaim, AuthorizationContent, ServerDomain};
use radroots_event::contract::AuthorRole;
use radroots_signing::{
    Actor, AuthoredArtifactId, SigningIntentId, SigningOperationId,
    actor::ActorSource,
    request::{CancellationPolicy, CancellationSignal, SignPolicy},
};
use secp256k1::{Keypair, Message, Secp256k1, SecretKey};
use std::sync::atomic::{AtomicUsize, Ordering};
use tera_core::runtime::product_surface::{CreateUpdate, Phase1AddCommand};

enum Completion {
    Late,
    Cancelled(CancellationSignal),
    MissingTime,
}

struct Host {
    completion: Completion,
    calls: Arc<AtomicUsize>,
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
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut secret = [0; 32];
        secret[31] = 1;
        let key =
            Keypair::from_secret_key(&Secp256k1::new(), &SecretKey::from_slice(&secret).unwrap());
        let signature = Secp256k1::new()
            .sign_schnorr_no_aux_rand(
                &Message::from_digest(request.event_id_digest.clone().try_into().unwrap()),
                &key,
            )
            .to_string();
        match &self.completion {
            Completion::Late => {
                tokio::time::sleep(std::time::Duration::from_millis(
                    request.deadline_unix_ms.saturating_sub(now_unix_ms()) + 2,
                ))
                .await
            }
            Completion::Cancelled(signal) => signal.cancel(),
            Completion::MissingTime => {}
        }
        HostSigningResult {
            schema_version: 1,
            outcome: HostSigningOutcome::Signed,
            operation_id: request.operation_id,
            signer_request_id: request.signer_request_id,
            public_key: request.public_key,
            purpose: request.purpose,
            signature_hex: Some(signature),
            completed_at_unix_ms: if matches!(self.completion, Completion::MissingTime) {
                0
            } else {
                now_unix_ms()
            },
        }
    }
}

fn request(blossom: bool) -> SignRequest {
    let at = now_unix_ms();
    let plan =
        Phase1AddCommand::CreateUpdate(CreateUpdate::new("exact authored signature").unwrap())
            .authored_plan(
                at / 1000,
                "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
            )
            .unwrap();
    let author = *plan.author();
    let actor = Actor::new(author, ActorSource::ExplicitPublicKey, [AuthorRole::Any]).unwrap();
    let intent = SigningIntentId::new(
        SigningOperationId::new([1; 16]).unwrap(),
        AuthoredArtifactId::new([2; 16]).unwrap(),
    );
    // Decode the governed wire operation ID through its public Serde contract.
    let operation = serde_json::from_str("\"sync.push\"").unwrap();
    let policy = SignPolicy::new(at + 100, CancellationPolicy::LocalCooperative).unwrap();
    if blossom {
        let claim = AuthoredUploadClaim::new(
            AuthorizationContent::parse("Upload exact image").unwrap(),
            ServerDomain::parse("media.example").unwrap(),
            radroots_blossom::Sha256::digest(b"image"),
            at / 1000,
            60,
        )
        .unwrap();
        SignRequest::blossom_upload(
            operation,
            intent,
            actor,
            radroots_sdk::signing::BlossomAuthorizationPlan::for_upload(&claim, author).unwrap(),
            policy,
        )
        .unwrap()
    } else {
        SignRequest::new(operation, intent, actor, plan, policy).unwrap()
    }
}

#[tokio::test]
async fn authored_evidence_retains_facts_while_strict_receipts_reject_late_cancelled_and_untimed_results()
 {
    for (mode, expected) in [
        (0, Kind::DeadlineExceeded),
        (1, Kind::SignerCancelled),
        (2, Kind::SignerOutputInvalid),
    ] {
        for evidence in [false, true] {
            let request = request(false);
            let completion = match mode {
                0 => Completion::Late,
                1 => Completion::Cancelled(request.cancellation_signal().clone()),
                _ => Completion::MissingTime,
            };
            let calls = Arc::new(AtomicUsize::new(0));
            let adapter = HostSignerAdapter::new(Box::new(Host {
                completion,
                calls: calls.clone(),
            }));
            if evidence {
                let retained = adapter
                    .sign_authored_evidence(request.clone())
                    .await
                    .unwrap();
                assert_eq!(retained.signed_event().id(), request.expected_event_id());
                assert_eq!(retained.signed_event().created_at(), request.created_at());
                assert!(retained.observed_at_unix_ms() > 0);
                assert_eq!(
                    retained
                        .revalidate(&request, now_unix_ms())
                        .unwrap()
                        .signed_event(),
                    retained.signed_event()
                );
            } else {
                assert_eq!(adapter.sign(request).await.unwrap_err().kind(), expected);
            }
            assert_eq!(calls.load(Ordering::Relaxed), 1);
        }
    }
}

#[tokio::test]
async fn blossom_never_enters_the_evidence_hook_and_keeps_strict_expiry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let adapter = HostSignerAdapter::new(Box::new(Host {
        completion: Completion::Late,
        calls: calls.clone(),
    }));
    assert_eq!(
        adapter
            .sign_authored_evidence(request(true))
            .await
            .unwrap_err()
            .kind(),
        Kind::InvalidArgument
    );
    assert_eq!(calls.load(Ordering::Relaxed), 0);
    assert_eq!(
        adapter.sign(request(true)).await.unwrap_err().kind(),
        Kind::DeadlineExceeded
    );
    assert_eq!(calls.load(Ordering::Relaxed), 1);
}
