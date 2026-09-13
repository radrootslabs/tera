use crate::{
    FfiComposerScopeRecord, FfiOutboxState, FfiSubmissionReservationRequest,
    MOBILE_FFI_SCHEMA_VERSION,
};
use tera_core::runtime::product_surface::{
    Phase1DraftRepairReason, SubmissionListEntry, SubmissionPage, SubmissionSummaryState,
};

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiSubmissionSummaryState {
    Reserved,
    Operation {
        intent_id: String,
        operation_id: String,
        revision: u64,
        state: FfiOutboxState,
    },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiSubmissionRepairReason {
    UnsupportedSchema,
    CorruptRecord,
    NeedsAttention,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiSubmissionListEntry {
    Submission {
        request: FfiSubmissionReservationRequest,
        reservation_id: String,
        reserved_at_unix_ms: u64,
        state: FfiSubmissionSummaryState,
    },
    Repair {
        reservation_key: String,
        revision: u64,
        reason: FfiSubmissionRepairReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiSubmissionPageRecord {
    pub schema_version: u16,
    pub scope: FfiComposerScopeRecord,
    pub entries: Vec<FfiSubmissionListEntry>,
    pub next_cursor: Option<String>,
}

impl From<&SubmissionPage> for FfiSubmissionPageRecord {
    fn from(value: &SubmissionPage) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            scope: value.scope().into(),
            entries: value
                .entries()
                .iter()
                .map(|entry| match entry {
                    SubmissionListEntry::Submission(summary) => {
                        FfiSubmissionListEntry::Submission {
                            request: summary.request().into(),
                            reservation_id: hex::encode(summary.reservation_id().as_bytes()),
                            reserved_at_unix_ms: summary.reserved_at_unix_ms(),
                            state: match summary.state() {
                                SubmissionSummaryState::Reserved => {
                                    FfiSubmissionSummaryState::Reserved
                                }
                                SubmissionSummaryState::Operation {
                                    intent_id,
                                    operation_id,
                                    revision,
                                    state,
                                } => FfiSubmissionSummaryState::Operation {
                                    intent_id: hex::encode(intent_id.as_bytes()),
                                    operation_id: hex::encode(operation_id.as_bytes()),
                                    revision,
                                    state: state.into(),
                                },
                            },
                        }
                    }
                    SubmissionListEntry::Repair {
                        reservation_key,
                        revision,
                        reason,
                    } => FfiSubmissionListEntry::Repair {
                        reservation_key: hex::encode(reservation_key),
                        revision: *revision,
                        reason: match reason {
                            Phase1DraftRepairReason::UnsupportedSchema => {
                                FfiSubmissionRepairReason::UnsupportedSchema
                            }
                            Phase1DraftRepairReason::CorruptRecord => {
                                FfiSubmissionRepairReason::CorruptRecord
                            }
                            Phase1DraftRepairReason::NeedsAttention => {
                                FfiSubmissionRepairReason::NeedsAttention
                            }
                        },
                    },
                })
                .collect(),
            next_cursor: value.next_cursor().map(str::to_owned),
        }
    }
}
