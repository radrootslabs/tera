use super::sync_tests::runtime;
use super::tests::{context, signed};
use super::*;
use radroots_transport::{
    Error, EventSource, FetchPage, FetchRequest, SourceStatus,
    outcome::{FetchTargetOutcome, FetchTargetState},
    source::{EventProvenance, NextPage, ObservedEvent},
};
use std::sync::Arc;

struct BatchSource {
    count: usize,
}

impl EventSource for BatchSource {
    fn status(&self) -> radroots_transport::BoxFuture<'_, Result<SourceStatus, Error>> {
        Box::pin(async { Err(Error::UnsupportedOperation) })
    }

    fn fetch(
        &self,
        request: FetchRequest,
    ) -> radroots_transport::BoxFuture<'_, Result<FetchPage, Error>> {
        Box::pin(async move {
            let target = request.target_set().targets()[0].fingerprint().clone();
            let event = signed(1, vec![], "bounded observation", 2_000_000_000);
            let provenance = EventProvenance::new(
                radroots_transport::TransportId::NOSTR,
                target.clone(),
                2_000_000_100_000,
            )
            .unwrap();
            let events = vec![ObservedEvent::new(event, provenance); self.count];
            FetchPage::for_request(
                &request,
                events,
                vec![FetchTargetOutcome::new(target, FetchTargetState::Complete)],
                NextPage::Complete,
            )
        })
    }
}

#[tokio::test]
async fn maximum_returned_observations_are_bounded_and_deduplicated_in_local_content() {
    let runtime = runtime(Arc::new(BatchSource {
        count: usize::from(TODAY_SYNC_PAGE_LIMIT),
    }));
    let receipt = runtime
        .phase1_sync_today(
            &context(None, 1),
            2_000_000_200,
            TodayProjectionUpdate::Incremental,
        )
        .await
        .unwrap();
    assert_eq!(receipt.events_observed, u64::from(TODAY_SYNC_PAGE_LIMIT));
    assert_eq!(receipt.relay_state, TodayRelaySyncState::Complete);
    assert_eq!(receipt.projection.visible_cards, 1);
    assert_eq!(receipt.projection.source_events, 1);
}

#[tokio::test]
async fn an_oversized_source_page_never_becomes_success_or_admitted_history() {
    let runtime = runtime(Arc::new(BatchSource {
        count: usize::from(TODAY_SYNC_PAGE_LIMIT) + 1,
    }));
    let receipt = runtime
        .phase1_sync_today(
            &context(None, 1),
            2_000_000_200,
            TodayProjectionUpdate::Incremental,
        )
        .await
        .unwrap();
    assert_eq!(receipt.relay_state, TodayRelaySyncState::Offline);
    assert_eq!(receipt.termination, TodaySyncTermination::SourceFailed);
    assert_eq!(receipt.pages_fetched, 0);
    assert_eq!(receipt.events_observed, 0);
    assert_eq!(receipt.projection.source_events, 0);
    assert_eq!(receipt.targets[0].final_state, None);
    assert_eq!(
        receipt.targets[0].summary.as_ref().unwrap().pages_observed,
        0
    );
}
