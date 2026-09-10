use tera_core::runtime::product_surface::{
    TodayDiscoveryReceipt, TodayProjectionUpdate, TodayRefreshReceipt, TodayRelaySyncState,
    TodaySyncReceipt, TodaySyncTermination, TodayTargetPageSummary, TodayTargetSyncReceipt,
    TodayTargetSyncState,
};
use tera_ffi::{
    FfiTodayProjectionUpdate, FfiTodayRelaySyncState, FfiTodaySyncRecord, FfiTodaySyncTermination,
    FfiTodayTargetSyncState, MOBILE_FFI_SCHEMA_VERSION,
};

#[test]
fn ffi_preserves_partial_history_even_when_the_final_target_state_is_complete() {
    let receipt = TodaySyncReceipt {
        relay_state: TodayRelaySyncState::Partial,
        termination: TodaySyncTermination::PageLimit,
        targets: vec![
            TodayTargetSyncReceipt {
                target_fingerprint: "opaque".into(),
                final_state: Some(TodayTargetSyncState::Complete),
                summary: Some(TodayTargetPageSummary {
                    pages_observed: 8,
                    incomplete_pages: 1,
                    missing_outcome_pages: 2,
                    last_incomplete: Some(TodayTargetSyncState::FailedRetryable),
                }),
            },
            TodayTargetSyncReceipt {
                target_fingerprint: "unknown".into(),
                final_state: None,
                summary: None,
            },
        ],
        pages_fetched: 8,
        events_observed: 500,
        events_admitted: 490,
        events_rejected: 10,
        discovery: TodayDiscoveryReceipt {
            continuation: Some("opaque-app-backfill".into()),
            had_incomplete_responses: true,
        },
        projection: TodayRefreshReceipt {
            update: TodayProjectionUpdate::Rebuild,
            source_events: 501,
            visible_cards: 400,
            profiles: 20,
            thread_entries: 70,
            content_generation: 7,
            changed: true,
        },
    };
    let record: FfiTodaySyncRecord = receipt.into();
    assert_eq!(record.schema_version, MOBILE_FFI_SCHEMA_VERSION);
    assert_eq!(
        record.discovery.continuation.as_deref(),
        Some("opaque-app-backfill")
    );
    assert!(record.discovery.had_incomplete_responses);
    assert_eq!(record.relay_state, FfiTodayRelaySyncState::Partial);
    assert_eq!(record.termination, FfiTodaySyncTermination::PageLimit);
    assert_eq!(record.targets.len(), 2);
    assert_eq!(record.targets[0].target_fingerprint, "opaque");
    assert_eq!(
        record.targets[0].final_state,
        Some(FfiTodayTargetSyncState::Complete)
    );
    let summary = record.targets[0].summary.as_ref().unwrap();
    assert_eq!(summary.pages_observed, 8);
    assert_eq!(summary.incomplete_pages, 1);
    assert_eq!(summary.missing_outcome_pages, 2);
    assert_eq!(
        summary.last_incomplete,
        Some(FfiTodayTargetSyncState::FailedRetryable)
    );
    assert_eq!(record.targets[1].final_state, None);
    assert_eq!(record.targets[1].summary, None);
    assert_eq!(
        (
            record.pages_fetched,
            record.events_observed,
            record.events_admitted,
            record.events_rejected
        ),
        (8, 500, 490, 10)
    );
    assert_eq!(record.projection.update, FfiTodayProjectionUpdate::Rebuild);
    assert_eq!(record.projection.source_events, 501);
    assert_eq!(record.projection.visible_cards, 400);
    assert_eq!(record.projection.profiles, 20);
    assert_eq!(record.projection.thread_entries, 70);
    assert_eq!(record.projection.content_generation, 7);
    assert!(record.projection.changed);
}

#[test]
fn ffi_preserves_every_typed_target_and_termination_state() {
    let states = [
        TodayTargetSyncState::Complete,
        TodayTargetSyncState::Partial,
        TodayTargetSyncState::Unavailable,
        TodayTargetSyncState::FailedRetryable,
        TodayTargetSyncState::FailedTerminal,
        TodayTargetSyncState::Cancelled,
    ];
    assert_eq!(
        states.map(FfiTodayTargetSyncState::from),
        [
            FfiTodayTargetSyncState::Complete,
            FfiTodayTargetSyncState::Partial,
            FfiTodayTargetSyncState::Unavailable,
            FfiTodayTargetSyncState::FailedRetryable,
            FfiTodayTargetSyncState::FailedTerminal,
            FfiTodayTargetSyncState::Cancelled
        ]
    );
    let ends = [
        TodaySyncTermination::Complete,
        TodaySyncTermination::PageLimit,
        TodaySyncTermination::Deadline,
        TodaySyncTermination::Cancelled,
        TodaySyncTermination::SourceFailed,
    ];
    assert_eq!(
        ends.map(FfiTodaySyncTermination::from),
        [
            FfiTodaySyncTermination::Complete,
            FfiTodaySyncTermination::PageLimit,
            FfiTodaySyncTermination::Deadline,
            FfiTodaySyncTermination::Cancelled,
            FfiTodaySyncTermination::SourceFailed
        ]
    );
}
