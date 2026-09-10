use super::tests::{context, ingest, keys, signed, signed_owned};
use super::*;
use crate::runtime::product_surface::ViewerCalendarContext;
use radroots_event::calendar::{AuthoredCalendarDateEvent, CalendarDate};
use radroots_event_codec::encode::calendar::calendar_date_event_build_tags;

fn instant(value: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp()
        .try_into()
        .unwrap()
}

async fn populate(runtime: &TeraRuntime, context: &LocalNetwork, now: u64) {
    for (index, date) in ["2026-09-04", "2026-09-05", "2026-09-06"]
        .into_iter()
        .enumerate()
    {
        let event = AuthoredCalendarDateEvent::new(
            format!("harvest-{index}"),
            "Harvest",
            CalendarDate::parse(date).unwrap(),
        )
        .unwrap();
        ingest(
            runtime,
            context,
            signed_owned(
                31_922,
                calendar_date_event_build_tags(&event).unwrap(),
                "",
                now - 100 + index as u64,
            ),
            now,
        )
        .await;
    }
}

#[tokio::test]
async fn sqlite_pages_freeze_zone_date_identity_and_order_across_other_queries_and_reopen() {
    let root = tempfile::tempdir().unwrap();
    let runtime = super::performance_tests::reopen_counted_sqlite(&root).await;
    let selected = context(None, 1);
    let now = instant("2026-09-05T00:30:00Z");
    populate(&runtime, &selected, now).await;
    let first = runtime
        .phase1_today_page(
            &selected,
            TodayPageRequest::first(1, now, "America/Vancouver"),
        )
        .await
        .unwrap();
    assert_eq!(first.calendar.civil_date().as_str(), "2026-09-04");
    let cursor = first.next_cursor.clone().unwrap();
    let original_scope = TodayCursor::scope(&cursor).unwrap();
    let east = runtime
        .phase1_today_page(
            &selected,
            TodayPageRequest::first(1, now, "Pacific/Kiritimati"),
        )
        .await
        .unwrap();
    let eastern_scope = TodayCursor::scope(east.next_cursor.as_deref().unwrap()).unwrap();
    assert_eq!(first.projection_generation, east.projection_generation);
    assert_ne!(snapshot_id(&original_scope), snapshot_id(&eastern_scope));
    assert_ne!(first.items[0].card.card_id, east.items[0].card.card_id);
    let expected = runtime
        .phase1_today_page(
            &selected,
            TodayPageRequest::first(100, now, "America/Vancouver"),
        )
        .await
        .unwrap();
    let next = runtime
        .phase1_today_page(&selected, TodayPageRequest::after(100, cursor.clone()))
        .await
        .unwrap();
    assert_eq!(next.calendar, first.calendar);
    assert_eq!([first.items.clone(), next.items].concat(), expected.items);
    let ids = expected
        .items
        .iter()
        .map(|card| card.card.card_id.to_hex())
        .collect::<Vec<_>>();
    let reconciled = runtime
        .phase1_today_reconcile(
            &selected,
            now,
            &ids,
            Some(first.projection_generation),
            first.calendar.time_zone(),
        )
        .await
        .unwrap();
    assert_eq!(reconciled.calendar, first.calendar);
    assert_eq!(reconciled.items, expected.items);
    let search = runtime
        .phase1_search(&selected, "Harvest", 100, now, first.calendar.time_zone())
        .await
        .unwrap();
    assert_eq!(
        search
            .into_iter()
            .map(|value| value.card.unwrap())
            .collect::<Vec<_>>(),
        expected.items
    );
    assert_eq!(
        runtime
            .phase1_me(
                &selected,
                &keys().public_key().to_string(),
                now,
                first.calendar.time_zone()
            )
            .await
            .unwrap()
            .cards,
        expected.items
    );
    runtime.shutdown().await.unwrap();
    // Reopen the same actual SQLite database. No clock or device zone is needed
    // to continue the opaque cursor, and no transaction spans this think time.
    let runtime = super::performance_tests::reopen_counted_sqlite(&root).await;
    let reopened = runtime
        .phase1_today_page(&selected, TodayPageRequest::after(100, cursor.clone()))
        .await
        .unwrap();
    assert_eq!(reopened.calendar, first.calendar);
    assert_eq!(reopened.items, expected.items[1..]);
    ingest(
        &runtime,
        &selected,
        signed(1, vec![], "new content", now + 1),
        now + 1,
    )
    .await;
    assert!(matches!(
        runtime
            .phase1_today_page(&selected, TodayPageRequest::after(100, cursor))
            .await,
        Err(TodayError::Cursor(CursorError::Stale))
    ));
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn local_midnight_changes_fresh_relevance_but_keeps_loaded_snapshot_frozen() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let selected = context(None, 1);
    let before = instant("2026-09-05T06:59:59Z");
    populate(&runtime, &selected, before).await;
    let first = runtime
        .phase1_today_page(
            &selected,
            TodayPageRequest::first(1, before, "America/Vancouver"),
        )
        .await
        .unwrap();
    let after = runtime
        .phase1_today_page(
            &selected,
            TodayPageRequest::first(100, before + 1, "America/Vancouver"),
        )
        .await
        .unwrap();
    assert_eq!(after.calendar.civil_date().as_str(), "2026-09-05");
    assert_ne!(after.items[0].card.card_id, first.items[0].card.card_id);
    let next = runtime
        .phase1_today_page(
            &selected,
            TodayPageRequest::after(100, first.next_cursor.unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(next.calendar, first.calendar);
    assert_eq!(next.calendar.civil_date().as_str(), "2026-09-04");
    let mut scope = CursorScope::new(
        selected.id.as_str().to_owned(),
        selected.generation,
        before,
        [0; 32],
        1,
        [0; 32],
        first.calendar.clone(),
    )
    .unwrap();
    let position = TodayCursorPosition {
        rank: first.items[0].card.rank.unwrap(),
    };
    scope.as_of += 1;
    assert_eq!(
        TodayCursor::encode(&scope, position),
        Err(CursorError::SnapshotMismatch)
    );
    scope.as_of = before;
    scope.calendar = ViewerCalendarContext::new(before, "UTC").unwrap();
    let changed = TodayCursor::encode(&scope, position).unwrap();
    scope.calendar = first.calendar;
    assert_eq!(
        TodayCursor::decode(changed.as_str(), &scope),
        Err(CursorError::SnapshotMismatch)
    );
}
