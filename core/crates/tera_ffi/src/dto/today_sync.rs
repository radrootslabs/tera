use super::{FfiTodayRefreshRecord, MOBILE_FFI_SCHEMA_VERSION};
use tera_core::runtime::product_surface::{
    TodayRelaySyncState, TodaySyncReceipt, TodaySyncTermination, TodayTargetPageSummary,
    TodayTargetSyncReceipt, TodayTargetSyncState,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiTodayRelaySyncState {
    Complete,
    Partial,
    Offline,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiTodaySyncTermination {
    Complete,
    PageLimit,
    Deadline,
    Cancelled,
    SourceFailed,
}

impl From<TodaySyncTermination> for FfiTodaySyncTermination {
    fn from(value: TodaySyncTermination) -> Self {
        match value {
            TodaySyncTermination::Complete => Self::Complete,
            TodaySyncTermination::PageLimit => Self::PageLimit,
            TodaySyncTermination::Deadline => Self::Deadline,
            TodaySyncTermination::Cancelled => Self::Cancelled,
            TodaySyncTermination::SourceFailed => Self::SourceFailed,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiTodayTargetSyncState {
    Complete,
    Partial,
    Unavailable,
    FailedRetryable,
    FailedTerminal,
    Cancelled,
}

impl From<TodayTargetSyncState> for FfiTodayTargetSyncState {
    fn from(value: TodayTargetSyncState) -> Self {
        match value {
            TodayTargetSyncState::Complete => Self::Complete,
            TodayTargetSyncState::Partial => Self::Partial,
            TodayTargetSyncState::Unavailable => Self::Unavailable,
            TodayTargetSyncState::FailedRetryable => Self::FailedRetryable,
            TodayTargetSyncState::FailedTerminal => Self::FailedTerminal,
            TodayTargetSyncState::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiTodayTargetPageSummary {
    pub pages_observed: u16,
    pub incomplete_pages: u16,
    pub missing_outcome_pages: u16,
    pub last_incomplete: Option<FfiTodayTargetSyncState>,
}

impl From<TodayTargetPageSummary> for FfiTodayTargetPageSummary {
    fn from(value: TodayTargetPageSummary) -> Self {
        Self {
            pages_observed: value.pages_observed,
            incomplete_pages: value.incomplete_pages,
            missing_outcome_pages: value.missing_outcome_pages,
            last_incomplete: value.last_incomplete.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiTodayTargetSyncRecord {
    pub target_fingerprint: String,
    pub final_state: Option<FfiTodayTargetSyncState>,
    pub summary: Option<FfiTodayTargetPageSummary>,
}

impl From<TodayTargetSyncReceipt> for FfiTodayTargetSyncRecord {
    fn from(value: TodayTargetSyncReceipt) -> Self {
        Self {
            target_fingerprint: value.target_fingerprint,
            final_state: value.final_state.map(Into::into),
            summary: value.summary.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiTodaySyncRecord {
    pub schema_version: u16,
    pub relay_state: FfiTodayRelaySyncState,
    pub termination: FfiTodaySyncTermination,
    pub targets: Vec<FfiTodayTargetSyncRecord>,
    pub pages_fetched: u16,
    pub events_observed: u64,
    pub events_admitted: u64,
    pub events_rejected: u64,
    pub projection: FfiTodayRefreshRecord,
}

impl From<TodaySyncReceipt> for FfiTodaySyncRecord {
    fn from(value: TodaySyncReceipt) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            relay_state: match value.relay_state {
                TodayRelaySyncState::Complete => FfiTodayRelaySyncState::Complete,
                TodayRelaySyncState::Partial => FfiTodayRelaySyncState::Partial,
                TodayRelaySyncState::Offline => FfiTodayRelaySyncState::Offline,
            },
            termination: value.termination.into(),
            targets: value.targets.into_iter().map(Into::into).collect(),
            pages_fetched: value.pages_fetched,
            events_observed: value.events_observed,
            events_admitted: value.events_admitted,
            events_rejected: value.events_rejected,
            projection: value.projection.into(),
        }
    }
}
