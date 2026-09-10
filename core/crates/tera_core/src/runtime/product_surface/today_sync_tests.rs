use super::tests::{context, ingest, signed};
use super::*;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, RwLock, atomic::AtomicBool};

use radroots_transport::{
    Error as TransportError, EventSource, FetchPage, FetchRequest, SourceStatus,
    outcome::{FetchTargetOutcome, FetchTargetState},
    source::{FetchCursor, NextPage},
};

struct ScriptedSource {
    pages: Mutex<VecDeque<Vec<Option<FetchTargetState>>>>,
}

impl EventSource for ScriptedSource {
    fn status(&self) -> radroots_transport::BoxFuture<'_, Result<SourceStatus, TransportError>> {
        Box::pin(async { unreachable!("explicit refresh only") })
    }

    fn fetch(
        &self,
        request: FetchRequest,
    ) -> radroots_transport::BoxFuture<'_, Result<FetchPage, TransportError>> {
        Box::pin(async move {
            assert_eq!(request.bounds().limit(), TODAY_SYNC_PAGE_LIMIT);
            assert_eq!(request.selector().kinds(), TODAY_SYNC_KINDS);
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis() as u64;
            assert!(request.bounds().deadline_unix_ms() > now);
            assert!(request.bounds().deadline_unix_ms() <= now + 30_000);
            let mut pages = self.pages.lock().expect("pages");
            let states = pages.pop_front().expect("bounded page count");
            assert_eq!(states.len(), request.target_set().len());
            let outcomes = request
                .target_set()
                .targets()
                .iter()
                .zip(states)
                .filter_map(|(target, state)| {
                    state.map(|state| FetchTargetOutcome::new(target.fingerprint().clone(), state))
                })
                .collect();
            let next = if pages.is_empty() {
                NextPage::Complete
            } else {
                NextPage::Cursor(
                    FetchCursor::parse(format!("remaining-{}", pages.len())).expect("cursor"),
                )
            };
            FetchPage::for_request(&request, vec![], outcomes, next)
        })
    }
}

pub(super) fn runtime(source: Arc<dyn EventSource>) -> TeraRuntime {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .source(source)
        .host_sync(radroots_sdk::sync::HostPolicy::standard())
        .build()
        .expect("client");
    TeraRuntime {
        client,
        started_unix_ms: 1,
        shutting_down: AtomicBool::new(false),
        lifecycle: Default::default(),
        platform_app: RwLock::new(None),
        store_public_key: None,
        mutations: Default::default(),
        settings_lock: tokio::sync::Mutex::new(()),
        identity_session: tokio::sync::RwLock::new(None),
        inbound_media_directory: None,
        inbound_media_lock: tokio::sync::Mutex::new(()),
    }
}

#[tokio::test]
async fn later_complete_page_does_not_erase_partial_refresh_or_cached_content() {
    let source = Arc::new(ScriptedSource {
        pages: Mutex::new(VecDeque::from([
            vec![Some(FetchTargetState::Partial)],
            vec![Some(FetchTargetState::Complete)],
        ])),
    });
    let runtime = runtime(source);
    let selected = context(None, 1);
    ingest(
        &runtime,
        &selected,
        signed(1, vec![], "saved before refresh", 2_000_000_000),
        2_000_000_100,
    )
    .await;
    let receipt = runtime
        .phase1_sync_today(&selected, 2_000_000_200, TodayProjectionUpdate::Incremental)
        .await
        .expect("refresh");
    assert_eq!(receipt.pages_fetched, 2);
    assert_eq!(receipt.projection.visible_cards, 1);
    assert_eq!(receipt.relay_state, TodayRelaySyncState::Partial);
    assert_eq!(receipt.termination, TodaySyncTermination::Complete);
    assert_eq!(receipt.targets.len(), 1);
    assert_eq!(
        receipt.targets[0].final_state,
        Some(TodayTargetSyncState::Complete)
    );
    let summary = receipt.targets[0]
        .summary
        .as_ref()
        .expect("measured evidence");
    assert_eq!(summary.pages_observed, 2);
    assert_eq!(summary.incomplete_pages, 1);
    assert_eq!(summary.missing_outcome_pages, 0);
    assert_eq!(summary.last_incomplete, Some(TodayTargetSyncState::Partial));
}

#[tokio::test]
async fn requested_targets_preserve_missing_and_failed_outcomes_independently() {
    let source = Arc::new(ScriptedSource {
        pages: Mutex::new(VecDeque::from([vec![
            Some(FetchTargetState::Complete),
            None,
            Some(FetchTargetState::FailedRetryable),
        ]])),
    });
    let runtime = runtime(source);
    let mut selected = context(None, 1);
    selected.relay_urls = (0..3)
        .map(|index| format!("wss://relay-{index}.example"))
        .collect();
    let receipt = runtime
        .phase1_sync_today(&selected, 2_000_000_200, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    assert_eq!(receipt.relay_state, TodayRelaySyncState::Partial);
    assert_eq!(receipt.targets.len(), 3);
    assert_eq!(
        receipt.targets[0]
            .summary
            .as_ref()
            .unwrap()
            .incomplete_pages,
        0
    );
    assert_eq!(receipt.targets[1].final_state, None);
    assert_eq!(
        receipt.targets[1]
            .summary
            .as_ref()
            .unwrap()
            .missing_outcome_pages,
        1
    );
    assert_eq!(
        receipt.targets[2].final_state,
        Some(TodayTargetSyncState::FailedRetryable)
    );
    assert_eq!(
        receipt.targets[2]
            .summary
            .as_ref()
            .unwrap()
            .incomplete_pages,
        1
    );
    for (target, url) in receipt.targets.iter().zip(&selected.relay_urls) {
        assert_eq!(
            target.target_fingerprint,
            radroots_transport::Target::nostr_relay(url)
                .unwrap()
                .fingerprint()
                .as_str()
        );
    }
    assert_eq!(
        serde_json::from_slice::<TodaySyncReceipt>(&serde_json::to_vec(&receipt).unwrap()).unwrap(),
        receipt
    );
}

#[tokio::test]
async fn page_limit_returns_eight_pages_and_retains_the_remaining_backfill() {
    let source = Arc::new(ScriptedSource {
        pages: Mutex::new(VecDeque::from(vec![
            vec![Some(FetchTargetState::Complete)];
            usize::from(TODAY_SYNC_MAX_PAGES) + 1
        ])),
    });
    let runtime = runtime(source.clone());
    let receipt = runtime
        .phase1_sync_today(
            &context(None, 1),
            2_000_000_200,
            TodayProjectionUpdate::Incremental,
        )
        .await
        .unwrap();
    assert_eq!(receipt.pages_fetched, TODAY_SYNC_MAX_PAGES);
    assert_eq!(receipt.termination, TodaySyncTermination::PageLimit);
    assert_eq!(receipt.relay_state, TodayRelaySyncState::Partial);
    assert_eq!(source.pages.lock().unwrap().len(), 1);
    assert_eq!(
        receipt.targets[0].summary.as_ref().unwrap().pages_observed,
        TODAY_SYNC_MAX_PAGES
    );
}

#[tokio::test]
async fn relay_inventory_accepts_the_shared_maximum_and_rejects_extra_before_fetch() {
    use radroots_transport::target::TARGET_SET_MAX_ITEMS;
    let relays = (0..TARGET_SET_MAX_ITEMS)
        .map(|index| format!("wss://relay-{index}.example"))
        .collect::<Vec<_>>();
    let selected = LocalNetwork::new(
        "bounds".into(),
        "Bounds".into(),
        relays.clone(),
        None,
        vec![],
        1,
    )
    .unwrap();
    let source = Arc::new(ScriptedSource {
        pages: Mutex::new(VecDeque::from([vec![
            Some(FetchTargetState::Complete);
            TARGET_SET_MAX_ITEMS
        ]])),
    });
    let runtime = runtime(source.clone());
    let receipt = runtime
        .phase1_sync_today(&selected, 2_000_000_200, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    assert_eq!(receipt.relay_state, TodayRelaySyncState::Complete);
    assert_eq!(receipt.targets.len(), TARGET_SET_MAX_ITEMS);
    let mut oversized = selected;
    oversized.relay_urls.push("wss://extra.example".into());
    assert!(
        LocalNetwork::new(
            "bounds".into(),
            "Bounds".into(),
            oversized.relay_urls.clone(),
            None,
            vec![],
            1
        )
        .is_err()
    );
    let decoded: LocalNetwork =
        serde_json::from_slice(&serde_json::to_vec(&oversized).unwrap()).unwrap();
    assert!(matches!(
        runtime
            .phase1_sync_today(&decoded, 2_000_000_200, TodayProjectionUpdate::Incremental)
            .await,
        Err(TodayError::InvalidRequest)
    ));
    assert!(source.pages.lock().unwrap().is_empty());
}
