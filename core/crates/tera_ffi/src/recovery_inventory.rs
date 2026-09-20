use crate::{
    FfiSubmissionRepairReason, FfiSubmissionReservationRequest, MOBILE_FFI_SCHEMA_VERSION,
};
use tera_core::runtime::product_surface::{
    Phase1DraftRepairReason,
    recovery_inventory::{RecoveryEntry, RecoveryOwner, RecoveryPage},
};

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRecoveryOwner {
    Legacy,
    Submission {
        request: FfiSubmissionReservationRequest,
    },
    Repair {
        reason: FfiSubmissionRepairReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRecoveryEntry {
    pub schema_version: u16,
    pub key: String,
    pub revision: u64,
    pub owner: FfiRecoveryOwner,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRecoveryPage {
    pub schema_version: u16,
    pub author: String,
    pub entries: Vec<FfiRecoveryEntry>,
    pub scanned: u16,
    pub next_cursor: Option<String>,
}

impl From<&RecoveryEntry> for FfiRecoveryEntry {
    fn from(value: &RecoveryEntry) -> Self {
        let owner = match &value.owner {
            RecoveryOwner::Legacy => FfiRecoveryOwner::Legacy,
            RecoveryOwner::Submission(request) => FfiRecoveryOwner::Submission {
                request: request.into(),
            },
            RecoveryOwner::Repair(reason) => FfiRecoveryOwner::Repair {
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
        };
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            key: hex::encode(value.key),
            revision: value.revision,
            owner,
        }
    }
}

impl From<&RecoveryPage> for FfiRecoveryPage {
    fn from(value: &RecoveryPage) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            author: hex::encode(value.author),
            entries: value.entries.iter().map(Into::into).collect(),
            scanned: value.scanned,
            next_cursor: value.next_cursor.clone(),
        }
    }
}
