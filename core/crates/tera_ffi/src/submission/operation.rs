//! Scoped operation records never masquerade as legacy draft mutation inputs.

use crate::{
    FfiComposerDraftRecord, FfiDraftMediaRecord, FfiOperationSettlementRecord, FfiOutboxState,
    FfiPreparedMediaInput, FfiSubmissionReservationRequest, MOBILE_FFI_SCHEMA_VERSION,
};
use tera_core::runtime::product_surface::{PublicationDeliveryState, SubmissionOperationStatus};

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiPublicationDeliveryState {
    NotIssued,
    Unknown,
    PartiallyAccepted,
    Accepted,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiPublicationDeliveryEvidence {
    pub state: FfiPublicationDeliveryState,
    pub stop_requested_at_unix_ms: Option<u64>,
    pub scheduling_revision: u64,
    pub retained_facts: u32,
    pub recorded_attempts: u32,
    pub unresolved_claims: bool,
}

impl From<tera_core::runtime::product_surface::PublicationDeliveryEvidence>
    for FfiPublicationDeliveryEvidence
{
    fn from(value: tera_core::runtime::product_surface::PublicationDeliveryEvidence) -> Self {
        Self {
            state: match value.state {
                PublicationDeliveryState::NotIssued => FfiPublicationDeliveryState::NotIssued,
                PublicationDeliveryState::Unknown => FfiPublicationDeliveryState::Unknown,
                PublicationDeliveryState::PartiallyAccepted => {
                    FfiPublicationDeliveryState::PartiallyAccepted
                }
                PublicationDeliveryState::Accepted => FfiPublicationDeliveryState::Accepted,
            },
            stop_requested_at_unix_ms: value.stop_requested_at_unix_ms,
            scheduling_revision: value.scheduling_revision,
            retained_facts: value.retained_facts,
            recorded_attempts: value.recorded_attempts,
            unresolved_claims: value.unresolved_claims,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiSubmissionMediaRecord {
    pub opaque_reference: String,
    pub progress: FfiDraftMediaRecord,
    pub authorizations: Vec<FfiUploadAttemptIdentity>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiUploadAttemptIdentity {
    pub operation_id: String,
    pub revision: Option<u64>,
    pub expiration_unix_s: u64,
}

#[derive(Clone, Eq, PartialEq, uniffi::Record)]
pub struct FfiSubmissionOperationRecord {
    pub schema_version: u16,
    pub request: FfiSubmissionReservationRequest,
    pub intent_id: String,
    pub operation_id: String,
    pub revision: u64,
    pub captured: FfiComposerDraftRecord,
    pub state: FfiOutboxState,
    pub committed_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub media: Vec<FfiSubmissionMediaRecord>,
    pub settlement: FfiOperationSettlementRecord,
    pub delivery: FfiPublicationDeliveryEvidence,
}

impl std::fmt::Debug for FfiSubmissionOperationRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FfiSubmissionOperationRecord")
            .field("state", &self.state)
            .finish_non_exhaustive()
    }
}

impl From<&SubmissionOperationStatus> for FfiSubmissionOperationRecord {
    fn from(value: &SubmissionOperationStatus) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            request: value.receipt().request().into(),
            intent_id: hex::encode(value.receipt().intent_id().as_bytes()),
            operation_id: hex::encode(value.receipt().operation_id().as_bytes()),
            revision: value.intent().revision().get(),
            captured: value.captured().into(),
            state: value.state().into(),
            committed_at_unix_ms: value.receipt().committed_at_unix_ms(),
            updated_at_unix_ms: value.intent().updated_at_unix_ms(),
            media: value
                .media()
                .iter()
                .map(|media| FfiSubmissionMediaRecord {
                    opaque_reference: media.local_reference().to_owned(),
                    progress: media.into(),
                    authorizations: media
                        .upload_authorizations()
                        .into_iter()
                        .map(|value| FfiUploadAttemptIdentity {
                            operation_id: hex::encode(value.operation_id),
                            revision: value.revision,
                            expiration_unix_s: value.expiration_unix_s,
                        })
                        .collect(),
                })
                .collect(),
            settlement: value.push().settlement().into(),
            delivery: value.delivery_evidence().into(),
        }
    }
}

#[derive(Clone, uniffi::Record)]
pub struct FfiSubmissionMediaInput {
    pub schema_version: u16,
    pub request: FfiSubmissionReservationRequest,
    pub expected_revision: u64,
    pub media: FfiPreparedMediaInput,
}

#[derive(Clone, uniffi::Record)]
pub struct FfiSubmissionUploadJobRecord {
    pub schema_version: u16,
    pub submission: FfiSubmissionOperationRecord,
    pub operation_id: String,
    pub remote_url: String,
    pub upload_url: String,
    pub authorization_header: String,
    pub expected_sha256: String,
    pub media_type: String,
    pub byte_size: u64,
}

#[derive(Clone, uniffi::Record)]
pub struct FfiSubmissionUploadResponse {
    pub schema_version: u16,
    pub status_code: u16,
    pub media_type: Option<String>,
    pub content_encoding: Option<String>,
    pub body: Vec<u8>,
}
