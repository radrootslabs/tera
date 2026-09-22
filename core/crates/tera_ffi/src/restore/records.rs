use tera_core::runtime::restore::{
    RestoreObservation, RestorePhase, RestoreStatus, RestoreTargetReview,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRestoreObservation {
    Observed,
    NotObserved,
    Incomplete,
}
impl From<RestoreObservation> for FfiRestoreObservation {
    fn from(value: RestoreObservation) -> Self {
        match value {
            RestoreObservation::Observed => Self::Observed,
            RestoreObservation::NotObserved => Self::NotObserved,
            RestoreObservation::Incomplete => Self::Incomplete,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRestorePhase {
    Held,
    Reviewed,
    Resumed,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRestoreTarget {
    pub draft_id: String,
    pub event_id: String,
    pub target_fingerprint: String,
    pub observation: Option<FfiRestoreObservation>,
}

impl From<RestoreTargetReview> for FfiRestoreTarget {
    fn from(value: RestoreTargetReview) -> Self {
        Self {
            draft_id: hex::encode(value.draft_id),
            event_id: hex::encode(value.event_id),
            target_fingerprint: value.target_fingerprint,
            observation: Some(value.observation.into()),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRestoreStatus {
    pub attempt_id: String,
    pub phase: FfiRestorePhase,
    pub targets: Vec<FfiRestoreTarget>,
}

impl From<RestoreStatus> for FfiRestoreStatus {
    fn from(value: RestoreStatus) -> Self {
        Self {
            attempt_id: hex::encode(value.attempt_id),
            phase: match value.phase {
                RestorePhase::Held => FfiRestorePhase::Held,
                RestorePhase::Reviewed => FfiRestorePhase::Reviewed,
                RestorePhase::Resumed => FfiRestorePhase::Resumed,
            },
            targets: value
                .targets
                .into_iter()
                .map(|target| FfiRestoreTarget {
                    draft_id: hex::encode(target.draft_id),
                    event_id: hex::encode(target.event_id),
                    target_fingerprint: target.target_fingerprint,
                    observation: target.observation.map(Into::into),
                })
                .collect(),
        }
    }
}
