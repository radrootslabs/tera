//! Current draft facts and Rust-owned coordinate admission projection.

use crate::{
    FfiAddCommandType, FfiDraftFormRecord, FfiDraftKind, FfiDraftMediaRecord,
    FfiOperationSettlementRecord, FfiOutboxState, MOBILE_FFI_SCHEMA_VERSION,
};
use tera_core::runtime::product_surface::Phase1DraftStatus;

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiDraftStatusRecord {
    pub coordinate_writable: bool,
    pub coordinate_captured: bool,
    pub schema_version: u16,
    pub draft_id: String,
    pub revision: u64,
    pub author_public_key: String,
    pub kind: FfiDraftKind,
    pub command_type: FfiAddCommandType,
    pub form: Option<FfiDraftFormRecord>,
    pub state: FfiOutboxState,
    pub card_id: String,
    pub operation_id: Option<String>,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub media: Vec<FfiDraftMediaRecord>,
    pub settlement: Option<FfiOperationSettlementRecord>,
    pub is_revision: bool,
    pub revision_parent_draft_id: Option<String>,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<Phase1DraftStatus> for FfiDraftStatusRecord {
    fn from(value: Phase1DraftStatus) -> Self {
        let draft = value.draft();
        let settlement = value.push().map(|push| push.settlement());
        Self {
            coordinate_writable: value.coordinate_writable(),
            coordinate_captured: value.coordinate_captured(),
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            draft_id: hex::encode(draft.draft_id().as_bytes()),
            revision: draft.revision().get(),
            author_public_key: hex::encode(draft.author()),
            kind: value.kind().into(),
            command_type: value.command_type().into(),
            form: value.form().map(Into::into),
            state: value.state().into(),
            card_id: value.card_id().to_hex(),
            operation_id: draft.operation_id().map(|id| hex::encode(id.as_bytes())),
            created_at_unix_ms: draft.created_at_unix_ms(),
            updated_at_unix_ms: draft.updated_at_unix_ms(),
            media: value.media().iter().map(Into::into).collect(),
            settlement: settlement.map(Into::into),
            is_revision: value.revision_policy().is_some(),
            revision_parent_draft_id: value.revision_parent_draft_id().map(hex::encode),
        }
    }
}
