//! Current revision evidence and permitted local actions.
use crate::{FfiDraftStatusRecord, FfiPublicationTargetDetails, MOBILE_FFI_SCHEMA_VERSION};
use tera_core::runtime::product_surface::{
    Phase1RevisionBranchStatus, Phase1RevisionPhase, Phase1RevisionPolicy, Phase1RevisionStatus,
};

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRevisionTargetRecord {
    pub card_id: String,
    pub source_event_id: String,
    pub source_address: Option<String>,
    pub author_public_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRevisionBranchRecord {
    pub stopped: bool,
    pub can_resume: bool,
    pub can_cancel: bool,
    pub targets: Option<FfiPublicationTargetDetails>,
}

impl From<&Phase1RevisionBranchStatus> for FfiRevisionBranchRecord {
    fn from(value: &Phase1RevisionBranchStatus) -> Self {
        Self {
            stopped: value.stopped,
            can_resume: value.can_resume,
            can_cancel: value.can_cancel,
            targets: value.targets.as_ref().map(Into::into),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRevisionPolicy {
    ReplaceThenRetract,
    AddressableReplacement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRevisionPhase {
    ReplacementPending,
    ReplacementFailed,
    RetractionPending,
    Complete,
    PartialEffect,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRevisionStatusRecord {
    pub schema_version: u16,
    pub operation_id: String,
    pub replacement: FfiDraftStatusRecord,
    pub retraction: Option<FfiDraftStatusRecord>,
    pub policy: FfiRevisionPolicy,
    pub phase: FfiRevisionPhase,
    pub original: FfiRevisionTargetRecord,
    pub replacement_progress: FfiRevisionBranchRecord,
    pub retraction_progress: Option<FfiRevisionBranchRecord>,
    pub can_resume: bool,
    pub can_cancel: bool,
}

impl From<Phase1RevisionStatus> for FfiRevisionStatusRecord {
    fn from(value: Phase1RevisionStatus) -> Self {
        let operation_id = hex::encode(value.replacement().draft().draft_id().as_bytes());
        let retraction = value.retraction().cloned().map(Into::into);
        let replacement = value.replacement().clone().into();
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            operation_id,
            original: FfiRevisionTargetRecord {
                card_id: value.target().card_id().to_hex(),
                source_event_id: value.target().source_event_id().to_owned(),
                source_address: value.target().source_address().map(str::to_owned),
                author_public_key: hex::encode(value.replacement().draft().author()),
            },
            replacement_progress: value.replacement_progress().into(),
            retraction_progress: value.retraction_progress().map(Into::into),
            can_resume: value.can_resume(),
            can_cancel: value.can_cancel(),
            replacement,
            retraction,
            policy: match value.policy() {
                Phase1RevisionPolicy::ReplaceThenRetract => FfiRevisionPolicy::ReplaceThenRetract,
                Phase1RevisionPolicy::AddressableReplacement => {
                    FfiRevisionPolicy::AddressableReplacement
                }
            },
            phase: match value.phase() {
                Phase1RevisionPhase::ReplacementPending => FfiRevisionPhase::ReplacementPending,
                Phase1RevisionPhase::ReplacementFailed => FfiRevisionPhase::ReplacementFailed,
                Phase1RevisionPhase::RetractionPending => FfiRevisionPhase::RetractionPending,
                Phase1RevisionPhase::Complete => FfiRevisionPhase::Complete,
                Phase1RevisionPhase::PartialEffect => FfiRevisionPhase::PartialEffect,
                Phase1RevisionPhase::Cancelled => FfiRevisionPhase::Cancelled,
            },
        }
    }
}
