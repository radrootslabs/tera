//! Versioned legacy selection metadata. Full authored content loads by ID.

use crate::{
    FfiAddCommandType, FfiDraftKind, FfiOperationSettlementRecord, FfiOutboxState,
    MOBILE_FFI_SCHEMA_VERSION,
};
use tera_core::runtime::product_surface::{
    Phase1DraftListEntry, Phase1DraftPage, Phase1DraftRepairReason, Phase1DraftSummary,
};

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiLegacyDraftSummaryRecord {
    pub schema_version: u16,
    pub draft_id: String,
    pub revision: u64,
    pub kind: FfiDraftKind,
    pub command_type: FfiAddCommandType,
    pub state: FfiOutboxState,
    pub has_form: bool,
    pub is_revision: bool,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub media_count: u64,
    pub verified_media_count: u64,
    pub possible_orphan_count: u64,
    pub settlement: Option<FfiOperationSettlementRecord>,
}

impl From<&Phase1DraftSummary> for FfiLegacyDraftSummaryRecord {
    fn from(value: &Phase1DraftSummary) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            draft_id: hex::encode(value.draft_id().as_bytes()),
            revision: value.revision().get(),
            kind: value.kind().into(),
            command_type: value.command_type().into(),
            state: value.state().into(),
            has_form: value.has_form(),
            is_revision: value.is_revision(),
            created_at_unix_ms: value.created_at_unix_ms(),
            updated_at_unix_ms: value.updated_at_unix_ms(),
            media_count: value.media_count(),
            verified_media_count: value.verified_media_count(),
            possible_orphan_count: value.possible_orphan_count(),
            settlement: value.settlement().map(Into::into),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiLegacyDraftRepairReason {
    UnsupportedSchema,
    CorruptRecord,
    NeedsAttention,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiLegacyDraftListEntry {
    Draft {
        summary: FfiLegacyDraftSummaryRecord,
    },
    Repair {
        draft_key: String,
        revision: u64,
        reason: FfiLegacyDraftRepairReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiLegacyDraftPageRecord {
    pub schema_version: u16,
    pub author_public_key: String,
    pub entries: Vec<FfiLegacyDraftListEntry>,
    pub next_cursor: Option<String>,
}

impl From<&Phase1DraftPage> for FfiLegacyDraftPageRecord {
    fn from(value: &Phase1DraftPage) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            author_public_key: hex::encode(value.author()),
            entries: value.entries().iter().map(Into::into).collect(),
            next_cursor: value.next_cursor().map(str::to_owned),
        }
    }
}

impl From<&Phase1DraftListEntry> for FfiLegacyDraftListEntry {
    fn from(value: &Phase1DraftListEntry) -> Self {
        match value {
            Phase1DraftListEntry::Draft(summary) => Self::Draft {
                summary: summary.into(),
            },
            Phase1DraftListEntry::Repair {
                draft_key,
                revision,
                reason,
            } => Self::Repair {
                draft_key: hex::encode(draft_key),
                revision: *revision,
                reason: match reason {
                    Phase1DraftRepairReason::UnsupportedSchema => {
                        FfiLegacyDraftRepairReason::UnsupportedSchema
                    }
                    Phase1DraftRepairReason::CorruptRecord => {
                        FfiLegacyDraftRepairReason::CorruptRecord
                    }
                    Phase1DraftRepairReason::NeedsAttention => {
                        FfiLegacyDraftRepairReason::NeedsAttention
                    }
                },
            },
        }
    }
}
