use radroots_sync::{PullRequest, pull::PullTermination};
use radroots_transport::{
    Target,
    outcome::FetchTargetState,
    source::FetchSelector,
    target::{TARGET_SET_MAX_ITEMS, TargetSet},
};
use serde::{Deserialize, Serialize};

use super::{
    LocalNetwork, TODAY_SYNC_KINDS, TODAY_SYNC_MAX_PAGES, TODAY_SYNC_PAGE_LIMIT,
    TodayAdmissionPolicy, TodayError, TodayProjectionUpdate, TodayRefreshReceipt,
};
use crate::runtime::TeraRuntime;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum TodayRelaySyncState {
    Complete,
    Partial,
    Offline,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum TodaySyncTermination {
    Complete,
    PageLimit,
    Deadline,
    Cancelled,
    SourceFailed,
}

impl From<PullTermination> for TodaySyncTermination {
    fn from(value: PullTermination) -> Self {
        match value {
            PullTermination::Complete => Self::Complete,
            PullTermination::PageLimit => Self::PageLimit,
            PullTermination::Deadline => Self::Deadline,
            PullTermination::Cancelled => Self::Cancelled,
            PullTermination::SourceFailed => Self::SourceFailed,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum TodayTargetSyncState {
    Complete,
    Partial,
    Unavailable,
    FailedRetryable,
    FailedTerminal,
    Cancelled,
}

impl From<FetchTargetState> for TodayTargetSyncState {
    fn from(value: FetchTargetState) -> Self {
        match value {
            FetchTargetState::Complete => Self::Complete,
            FetchTargetState::Partial => Self::Partial,
            FetchTargetState::Unavailable => Self::Unavailable,
            FetchTargetState::FailedRetryable => Self::FailedRetryable,
            FetchTargetState::FailedTerminal => Self::FailedTerminal,
            FetchTargetState::Cancelled => Self::Cancelled,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayTargetPageSummary {
    pub pages_observed: u16,
    pub incomplete_pages: u16,
    pub missing_outcome_pages: u16,
    pub last_incomplete: Option<TodayTargetSyncState>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayTargetSyncReceipt {
    pub target_fingerprint: String,
    pub final_state: Option<TodayTargetSyncState>,
    /// None denotes unknown cumulative evidence, never positive completeness.
    pub summary: Option<TodayTargetPageSummary>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodaySyncReceipt {
    pub relay_state: TodayRelaySyncState,
    pub termination: TodaySyncTermination,
    /// Stable request order, bounded by the shared target-set limit.
    pub targets: Vec<TodayTargetSyncReceipt>,
    pub pages_fetched: u16,
    pub events_observed: u64,
    pub events_admitted: u64,
    pub events_rejected: u64,
    pub projection: TodayRefreshReceipt,
}

impl TeraRuntime {
    /// Pulls finite Today observations under the shared network deadline, then
    /// materializes the local projection and retains scoped relay evidence.
    pub async fn phase1_sync_today(
        &self,
        context: &LocalNetwork,
        now_unix_seconds: u64,
        update: TodayProjectionUpdate,
    ) -> Result<TodaySyncReceipt, TodayError> {
        let _command = self.lifecycle.enter()?;
        if now_unix_seconds == 0
            || context.relay_urls.is_empty()
            || context.relay_urls.len() > TARGET_SET_MAX_ITEMS
        {
            return Err(TodayError::InvalidRequest);
        }
        let targets = context
            .relay_urls
            .iter()
            .map(Target::nostr_relay)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| TodayError::InvalidRequest)?;
        let targets = TargetSet::new(targets).map_err(|_| TodayError::InvalidRequest)?;
        let selector = FetchSelector::all()
            .with_kinds(TODAY_SYNC_KINDS.to_vec())
            .map_err(|_| TodayError::InvalidRequest)?;
        let request =
            PullRequest::new(targets.clone(), TODAY_SYNC_PAGE_LIMIT, TODAY_SYNC_MAX_PAGES)
                .map_err(|_| TodayError::RuntimeUnavailable)?
                .with_selector(selector);
        let sync = self
            .client
            .sync()
            .map_err(|_| TodayError::RuntimeUnavailable)?
            .ok_or(TodayError::RuntimeUnavailable)?;
        let pull = sync
            .pull(request, &TodayAdmissionPolicy)
            .await
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let projection = self
            .phase1_refresh_today(context, now_unix_seconds, update)
            .await?;
        let events_admitted = pull
            .ingest_outcomes()
            .iter()
            .filter(|outcome| outcome.is_ok())
            .count() as u64;
        let events_observed =
            u64::try_from(pull.events_observed()).map_err(|_| TodayError::InvalidRequest)?;
        let events_rejected = events_observed.saturating_sub(events_admitted);
        let target_complete = pull.target_summaries().is_some_and(|summaries| {
            summaries.len() == targets.len()
                && targets.targets().iter().all(|target| {
                    summaries.iter().any(|summary| {
                        summary.target() == target.fingerprint()
                            && summary.pages_observed() == pull.pages_fetched()
                            && summary.all_pages_complete()
                    })
                })
        });
        let relay_state = match pull.termination() {
            PullTermination::Complete if target_complete => TodayRelaySyncState::Complete,
            PullTermination::SourceFailed if pull.pages_fetched() == 0 => {
                TodayRelaySyncState::Offline
            }
            _ => TodayRelaySyncState::Partial,
        };
        let targets = targets
            .targets()
            .iter()
            .map(|target| {
                let final_state = pull
                    .target_outcomes()
                    .iter()
                    .find(|outcome| outcome.target() == target.fingerprint())
                    .map(|outcome| outcome.state().into());
                let summary = pull
                    .target_summaries()
                    .and_then(|summaries| {
                        summaries
                            .iter()
                            .find(|summary| summary.target() == target.fingerprint())
                    })
                    .map(|summary| TodayTargetPageSummary {
                        pages_observed: summary.pages_observed(),
                        incomplete_pages: summary.incomplete_pages(),
                        missing_outcome_pages: summary.missing_outcome_pages(),
                        last_incomplete: summary.last_incomplete().map(Into::into),
                    });
                TodayTargetSyncReceipt {
                    target_fingerprint: target.fingerprint().as_str().to_owned(),
                    final_state,
                    summary,
                }
            })
            .collect();
        Ok(TodaySyncReceipt {
            relay_state,
            termination: pull.termination().into(),
            targets,
            pages_fetched: pull.pages_fetched(),
            events_observed,
            events_admitted,
            events_rejected,
            projection,
        })
    }
}
