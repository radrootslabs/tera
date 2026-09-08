//! Opaque, secret-free host signing bridge.

use std::sync::Arc;

use radroots_event::{SignedEvent, wire::v1::Nip01EventWire};
use radroots_signing::{
    Error, SignReceipt, SignRequest, Signer, SignerStatus,
    capability::{CancellationSupport, SignerCapability, SignerKind},
    error::Kind,
    recovery::ReplayCapability,
    signer::BoxFuture,
    status::SignerAvailability,
};

use crate::MOBILE_FFI_SCHEMA_VERSION;

const SIGNED_EVENT_MAX_BYTES: usize = 1_048_576;

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum SignerAvailabilityRecord {
    Ready,
    Busy,
    Locked,
    Unavailable,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct SignerStatusRecord {
    pub schema_version: u16,
    pub availability: SignerAvailabilityRecord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum HostSigningPurpose {
    NostrEvent,
    BlossomUpload,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct HostSigningRequest {
    pub schema_version: u16,
    pub operation_kind: String,
    pub operation_id: String,
    pub artifact_id: String,
    pub signer_request_id: String,
    pub public_key: String,
    pub purpose: HostSigningPurpose,
    pub deadline_unix_ms: u64,
    pub event_id_digest: Vec<u8>,
    pub expected_event_id: String,
    pub created_at_unix_s: u64,
    pub kind: u32,
    pub tags: Vec<Vec<String>>,
    pub content: String,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum HostSigningOutcome {
    Signed,
    Locked,
    Cancelled,
    Rejected,
    TimedOut,
    Unavailable,
    Invalidated,
    Failed,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct HostSigningResult {
    pub schema_version: u16,
    pub outcome: HostSigningOutcome,
    pub operation_id: String,
    pub signer_request_id: String,
    pub public_key: String,
    pub purpose: HostSigningPurpose,
    pub signature_hex: Option<String>,
    pub completed_at_unix_ms: u64,
}

#[uniffi::export(callback_interface)]
#[async_trait::async_trait]
pub trait RadrootsHostSigner: Send + Sync {
    async fn signer_status(&self) -> SignerStatusRecord;
    async fn sign(&self, request: HostSigningRequest) -> HostSigningResult;
}

pub(crate) struct HostSignerAdapter {
    host: Arc<dyn RadrootsHostSigner>,
}

impl HostSignerAdapter {
    pub(crate) fn new(host: Box<dyn RadrootsHostSigner>) -> Self {
        Self {
            host: Arc::from(host),
        }
    }
}

impl Signer for HostSignerAdapter {
    fn status(&self) -> BoxFuture<'_, Result<SignerStatus, Error>> {
        Box::pin(async move {
            let status = self.host.signer_status().await;
            if status.schema_version != MOBILE_FFI_SCHEMA_VERSION {
                return Err(Error::new(Kind::SignerOutputInvalid));
            }
            let availability = match status.availability {
                SignerAvailabilityRecord::Ready => SignerAvailability::Ready,
                SignerAvailabilityRecord::Busy => SignerAvailability::Busy,
                SignerAvailabilityRecord::Locked => SignerAvailability::AwaitingAuthentication,
                SignerAvailabilityRecord::Unavailable => SignerAvailability::Unavailable,
            };
            Ok(SignerStatus::new(
                availability,
                vec![SignerCapability::new(
                    SignerKind::HostMediated,
                    ReplayCapability::ExactReplayByRequestId,
                    CancellationSupport::BeforeAndAfterPublication,
                    false,
                    true,
                )],
                None,
            ))
        })
    }

    fn sign(&self, request: SignRequest) -> BoxFuture<'_, Result<SignReceipt, Error>> {
        Box::pin(async move {
            request.ensure_active(now_unix_ms())?;
            let ffi_request = HostSigningRequest::from_request(&request)?;
            let result = self.host.sign(ffi_request.clone()).await;
            request.ensure_active(now_unix_ms())?;
            request.ensure_active(result.completed_at_unix_ms)?;
            validate_result_binding(&ffi_request, &result)?;
            match result.outcome {
                HostSigningOutcome::Signed => signed_receipt(&request, result),
                HostSigningOutcome::Locked | HostSigningOutcome::Unavailable => {
                    Err(Error::new(Kind::SignerUnavailable))
                }
                HostSigningOutcome::Cancelled => Err(Error::new(Kind::SignerCancelled)),
                HostSigningOutcome::Rejected => Err(Error::new(Kind::SignerRejected)),
                HostSigningOutcome::TimedOut => Err(Error::new(Kind::SignerTimeout)),
                HostSigningOutcome::Invalidated => Err(Error::new(Kind::SignerOutputInvalid)),
                HostSigningOutcome::Failed => Err(Error::new(Kind::InternalError)),
            }
        })
    }
}

impl HostSigningRequest {
    fn from_request(request: &SignRequest) -> Result<Self, Error> {
        let purpose = match request.purpose() {
            radroots_signing::SigningPurpose::AuthoredEvent => HostSigningPurpose::NostrEvent,
            radroots_signing::SigningPurpose::BlossomUploadAuthorization => {
                HostSigningPurpose::BlossomUpload
            }
            _ => return Err(Error::new(Kind::SignerOutputInvalid)),
        };
        Ok(Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            operation_kind: request.operation_kind().as_str().to_owned(),
            operation_id: uuid_string(request.intent_id().operation_id().as_bytes()),
            artifact_id: hex::encode(request.intent_id().artifact_id().as_bytes()),
            signer_request_id: request.signer_request_id().to_hex(),
            public_key: request.expected_author().to_hex(),
            purpose,
            deadline_unix_ms: request.policy().deadline_unix_ms(),
            event_id_digest: request.expected_event_id().as_bytes().to_vec(),
            expected_event_id: request.expected_event_id().to_hex(),
            created_at_unix_s: request.created_at(),
            kind: request.kind(),
            tags: request.tags().to_vec(),
            content: request.content().to_owned(),
        })
    }
}

fn validate_result_binding(
    request: &HostSigningRequest,
    result: &HostSigningResult,
) -> Result<(), Error> {
    if result.schema_version != MOBILE_FFI_SCHEMA_VERSION
        || result.operation_id != request.operation_id
        || result.signer_request_id != request.signer_request_id
        || result.public_key != request.public_key
        || result.purpose != request.purpose
        || result.completed_at_unix_ms == 0
        || (result.outcome == HostSigningOutcome::Signed) != result.signature_hex.is_some()
    {
        return Err(Error::new(Kind::SignerOutputInvalid));
    }
    Ok(())
}

fn signed_receipt(request: &SignRequest, result: HostSigningResult) -> Result<SignReceipt, Error> {
    let signature = result
        .signature_hex
        .filter(|value| {
            value.len() == 128
                && value
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        })
        .ok_or_else(|| Error::new(Kind::SignerOutputInvalid))?;
    let wire = Nip01EventWire {
        id: request.expected_event_id().to_hex(),
        pubkey: request.expected_author().to_hex(),
        created_at: request.created_at(),
        kind: request.kind(),
        tags: request.tags().to_vec(),
        content: request.content().to_owned(),
        sig: signature,
        extra: Default::default(),
    };
    let raw_json = serde_json::to_string(&wire).map_err(|_| Error::new(Kind::InternalError))?;
    if raw_json.len() > SIGNED_EVENT_MAX_BYTES {
        return Err(Error::new(Kind::SignerOutputInvalid));
    }
    let signed = SignedEvent::from_wire_verified_id(wire, raw_json)
        .map_err(|_| Error::new(Kind::SignerOutputInvalid))?;
    SignReceipt::from_signed_event(request, signed, result.completed_at_unix_ms)
}

fn uuid_string(bytes: &[u8; 16]) -> String {
    let hex = hex::encode(bytes);
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_millis().try_into().unwrap_or(u64::MAX)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    struct StatusHost {
        schema_version: u16,
        availability: SignerAvailabilityRecord,
    }

    #[async_trait::async_trait]
    impl RadrootsHostSigner for StatusHost {
        async fn signer_status(&self) -> SignerStatusRecord {
            SignerStatusRecord {
                schema_version: self.schema_version,
                availability: self.availability,
            }
        }

        async fn sign(&self, request: HostSigningRequest) -> HostSigningResult {
            HostSigningResult {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                outcome: HostSigningOutcome::Failed,
                operation_id: request.operation_id,
                signer_request_id: request.signer_request_id,
                public_key: request.public_key,
                purpose: request.purpose,
                signature_hex: None,
                completed_at_unix_ms: 1,
            }
        }
    }

    #[test]
    fn opaque_operation_identity_has_canonical_uuid_shape() {
        assert_eq!(
            uuid_string(&[0xabu8; 16]),
            "abababab-abab-abab-abab-abababababab"
        );
    }

    #[test]
    fn result_binding_rejects_signature_on_failure() {
        let request = HostSigningRequest {
            schema_version: 1,
            operation_kind: "sync.push".to_owned(),
            operation_id: "11111111-1111-1111-1111-111111111111".to_owned(),
            artifact_id: "22".repeat(16),
            signer_request_id: "33".repeat(32),
            public_key: "44".repeat(32),
            purpose: HostSigningPurpose::NostrEvent,
            deadline_unix_ms: 10,
            event_id_digest: vec![5; 32],
            expected_event_id: "55".repeat(32),
            created_at_unix_s: 1,
            kind: 1,
            tags: Vec::new(),
            content: "test".to_owned(),
        };
        let result = HostSigningResult {
            schema_version: 1,
            outcome: HostSigningOutcome::Failed,
            operation_id: request.operation_id.clone(),
            signer_request_id: request.signer_request_id.clone(),
            public_key: request.public_key.clone(),
            purpose: request.purpose,
            signature_hex: Some("66".repeat(64)),
            completed_at_unix_ms: 2,
        };
        assert_eq!(
            validate_result_binding(&request, &result)
                .expect_err("failure cannot carry signature")
                .kind(),
            Kind::SignerOutputInvalid
        );

        let valid = HostSigningResult {
            signature_hex: None,
            ..result.clone()
        };
        assert!(validate_result_binding(&request, &valid).is_ok());
        let invalid_results = [
            HostSigningResult {
                schema_version: 2,
                ..valid.clone()
            },
            HostSigningResult {
                operation_id: "different".to_owned(),
                ..valid.clone()
            },
            HostSigningResult {
                signer_request_id: "different".to_owned(),
                ..valid.clone()
            },
            HostSigningResult {
                public_key: "different".to_owned(),
                ..valid.clone()
            },
            HostSigningResult {
                purpose: HostSigningPurpose::BlossomUpload,
                ..valid.clone()
            },
            HostSigningResult {
                completed_at_unix_ms: 0,
                ..valid.clone()
            },
            HostSigningResult {
                outcome: HostSigningOutcome::Signed,
                signature_hex: None,
                ..valid
            },
        ];
        for invalid in invalid_results {
            assert_eq!(
                validate_result_binding(&request, &invalid)
                    .expect_err("binding mismatch")
                    .kind(),
                Kind::SignerOutputInvalid
            );
        }
    }

    #[tokio::test]
    async fn host_status_maps_every_availability_and_rejects_schema_drift() {
        for (ffi, expected) in [
            (SignerAvailabilityRecord::Ready, SignerAvailability::Ready),
            (SignerAvailabilityRecord::Busy, SignerAvailability::Busy),
            (
                SignerAvailabilityRecord::Locked,
                SignerAvailability::AwaitingAuthentication,
            ),
            (
                SignerAvailabilityRecord::Unavailable,
                SignerAvailability::Unavailable,
            ),
        ] {
            let adapter = HostSignerAdapter::new(Box::new(StatusHost {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                availability: ffi,
            }));
            assert_eq!(
                Signer::status(&adapter)
                    .await
                    .expect("mapped host status")
                    .availability(),
                expected
            );
        }

        let adapter = HostSignerAdapter::new(Box::new(StatusHost {
            schema_version: MOBILE_FFI_SCHEMA_VERSION + 1,
            availability: SignerAvailabilityRecord::Ready,
        }));
        assert_eq!(
            Signer::status(&adapter)
                .await
                .expect_err("schema drift")
                .kind(),
            Kind::SignerOutputInvalid
        );
    }
}
