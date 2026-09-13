//! Scoped operation records never masquerade as legacy draft mutation inputs.

use crate::{
    FfiComposerDraftRecord, FfiDraftMediaRecord, FfiOperationSettlementRecord, FfiOutboxState,
    FfiPreparedMediaInput, FfiSubmissionReservationRequest, MOBILE_FFI_SCHEMA_VERSION,
};
use tera_core::runtime::product_surface::SubmissionOperationStatus;

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiSubmissionMediaRecord {
    pub opaque_reference: String,
    pub progress: FfiDraftMediaRecord,
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
                })
                .collect(),
            settlement: value.push().settlement().into(),
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
