use super::super::sync_tests::runtime;
use super::super::tests::{context, signed};
use super::*;
use radroots_event::SignedEvent;
use radroots_transport::{
    Error, EventSource, FetchPage, FetchRequest, SourceStatus,
    outcome::FetchTargetOutcome,
    source::{EventProvenance, FetchCursor, NextPage, ObservedEvent},
};
use std::sync::{Arc, Mutex};

#[derive(Default)]
struct HistorySource {
    events: Mutex<Vec<SignedEvent>>,
    cursors: Mutex<Vec<Option<String>>>,
}

impl EventSource for HistorySource {
    fn status(&self) -> radroots_transport::BoxFuture<'_, Result<SourceStatus, Error>> {
        Box::pin(async { Err(Error::UnsupportedOperation) })
    }

    fn fetch(
        &self,
        request: FetchRequest,
    ) -> radroots_transport::BoxFuture<'_, Result<FetchPage, Error>> {
        Box::pin(async move {
            assert_eq!(request.bounds().limit(), TODAY_SYNC_PAGE_LIMIT);
            assert_eq!(request.selector().kinds(), TODAY_SYNC_KINDS);
            assert_eq!(
                request.selector().since_unix_seconds(),
                None,
                "no latest-time watermark"
            );
            assert_eq!(
                request.selector().until_unix_seconds(),
                None,
                "no invented age cutoff"
            );
            self.cursors
                .lock()
                .unwrap()
                .push(request.cursor().map(|cursor| cursor.as_str().to_owned()));
            let offset = request.cursor().map_or(0, |cursor| {
                cursor
                    .as_str()
                    .strip_prefix("offset:")
                    .unwrap()
                    .parse::<usize>()
                    .unwrap()
            });
            let history = self.events.lock().unwrap();
            let mut sorted = history.iter().collect::<Vec<_>>();
            sorted.sort_by(|left, right| {
                right
                    .created_at()
                    .cmp(&left.created_at())
                    .then_with(|| right.id_str().cmp(left.id_str()))
            });
            let target = request.target_set().targets()[0].fingerprint().clone();
            let events = sorted
                .iter()
                .skip(offset)
                .take(usize::from(TODAY_SYNC_PAGE_LIMIT))
                .map(|event| {
                    ObservedEvent::new(
                        (*event).clone(),
                        EventProvenance::new(
                            radroots_transport::TransportId::NOSTR,
                            target.clone(),
                            2_000_000_100_000,
                        )
                        .unwrap(),
                    )
                })
                .collect::<Vec<_>>();
            let next = if offset + events.len() < history.len() {
                NextPage::Cursor(
                    FetchCursor::parse(format!("offset:{}", offset + events.len())).unwrap(),
                )
            } else {
                NextPage::Complete
            };
            FetchPage::for_request(
                &request,
                events,
                vec![FetchTargetOutcome::new(target, FetchTargetState::Complete)],
                next,
            )
        })
    }
}

#[tokio::test]
async fn overlap_refresh_recovers_an_older_offline_arrival_and_deduplicates_replay() {
    let source = Arc::new(HistorySource::default());
    let newer = signed(1, vec![], "newer first", 2_000_000_010);
    source.events.lock().unwrap().push(newer.clone());
    let runtime = runtime(source.clone());
    let selected = context(None, 1);
    let first = runtime
        .phase1_sync_today(&selected, 2_000_000_100, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    assert_eq!(first.projection.visible_cards, 1);
    source.events.lock().unwrap().extend([
        signed(1, vec![], "arrived late while offline", 2_000_000_001),
        newer,
    ]);
    let second = runtime
        .phase1_sync_today(&selected, 2_000_000_101, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    assert_eq!(second.events_observed, 3);
    assert_eq!(second.projection.source_events, 2);
    assert_eq!(second.projection.visible_cards, 2);
    assert_eq!(*source.cursors.lock().unwrap(), vec![None, None]);
    assert!(second.discovery.continuation.is_none());
}

#[tokio::test]
async fn equal_time_peers_beyond_one_page_survive_shared_ingest_without_duplicate_cards() {
    let source = Arc::new(HistorySource::default());
    let mut events = (0..501)
        .map(|index| signed(1, vec![], &format!("same-time {index}"), 2_000_000_000))
        .collect::<Vec<_>>();
    events.extend_from_within(..2);
    *source.events.lock().unwrap() = events;
    let runtime = runtime(source.clone());
    let receipt = runtime
        .phase1_sync_today(
            &context(None, 1),
            2_000_000_100,
            TodayProjectionUpdate::Incremental,
        )
        .await
        .unwrap();
    assert_eq!(receipt.pages_fetched, 2);
    assert_eq!(receipt.events_observed, 503);
    assert_eq!(receipt.projection.source_events, 501);
    assert_eq!(receipt.projection.visible_cards, 501);
    assert_eq!(
        *source.cursors.lock().unwrap(),
        vec![None, Some("offset:500".into())]
    );
    assert!(!receipt.discovery.had_incomplete_responses);
}

#[tokio::test]
async fn mismatched_context_store_and_oversized_backfill_fail_before_source_access() {
    let source = Arc::new(HistorySource::default());
    let runtime = runtime(source.clone());
    let selected = context(None, 1);
    let scope =
        super::super::paging_scope::query_scope(&selected, runtime.store_public_key).unwrap();
    let generation = *radroots_storage::EventStore::status(runtime.client.storage().unwrap())
        .await
        .unwrap()
        .generation()
        .as_bytes();
    let shared = FetchCursor::parse("offset:500").unwrap();
    let cursor = BackfillCursor::encode(&shared, scope, generation, false);
    let mut changed_label = selected.clone();
    changed_label.label = "changed".into();
    let mut changed_relays = selected.clone();
    changed_relays.relay_urls = vec!["wss://other.example".into()];
    let mut changed_following = selected.clone();
    changed_following.followed_authors = vec!["a".repeat(64)];
    for context in [
        changed_label,
        changed_relays,
        changed_following,
        context(Some("local"), 1),
        context(None, 2),
    ] {
        assert!(matches!(
            runtime
                .phase1_backfill_today(&context, 2_000_000_100, &cursor)
                .await,
            Err(TodayError::Cursor(
                super::super::CursorError::ContextMismatch
            ))
        ));
    }
    let mut retired = generation;
    retired[0] ^= 1;
    let retired = BackfillCursor::encode(&shared, scope, retired, false);
    assert!(matches!(
        runtime
            .phase1_backfill_today(&selected, 2_000_000_100, &retired)
            .await,
        Err(TodayError::Cursor(super::super::CursorError::Stale))
    ));
    assert!(
        runtime
            .phase1_backfill_today(
                &selected,
                2_000_000_100,
                &"x".repeat(backfill_cursor::MAX_BYTES + 1)
            )
            .await
            .is_err()
    );
    assert!(source.cursors.lock().unwrap().is_empty());
}
