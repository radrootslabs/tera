use super::tests::{context, ingest, keys, signed_owned};
use super::*;
use radroots_event::calendar::{
    AuthoredCalendarDateEvent, AuthoredCalendarTimeEvent, CalendarDate,
};
use radroots_event_codec::encode::calendar::{
    calendar_date_event_build_tags, calendar_time_event_build_tags,
};

const AUTHORED: u64 = 1_800_000_000;
const NOW: u64 = 2_000_000_000;

fn date_event() -> radroots_event::SignedEvent {
    let event = AuthoredCalendarDateEvent::new(
        "harvest-day",
        "Harvest day",
        CalendarDate::parse("2026-09-05").unwrap(),
    )
    .unwrap()
    .with_end(CalendarDate::parse("2026-09-07").unwrap())
    .unwrap();
    signed_owned(
        31_922,
        calendar_date_event_build_tags(&event).unwrap(),
        "",
        AUTHORED,
    )
}

#[tokio::test]
async fn actual_admitted_calendar_projection_keeps_dates_instants_and_zones_distinct() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let context = context(None, 1);
    let civil = date_event();
    ingest(&runtime, &context, civil.clone(), NOW).await;
    let timed = AuthoredCalendarTimeEvent::new("shift", "Shift", AUTHORED + 100)
        .unwrap()
        .with_end(AUTHORED + 200)
        .unwrap()
        .with_start_tzid("America/Vancouver")
        .unwrap()
        .with_end_tzid("Europe/Paris")
        .unwrap();
    let timed_event = signed_owned(
        31_923,
        calendar_time_event_build_tags(&timed).unwrap(),
        "",
        AUTHORED + 1,
    );
    ingest(&runtime, &context, timed_event.clone(), NOW).await;
    let page = runtime
        .phase1_today_page(&context, TodayPageRequest::first(20, NOW))
        .await
        .unwrap();
    assert_eq!(page.items.len(), 2);
    let civil_card = &page
        .items
        .iter()
        .find(|item| item.card.source_event_id == civil.id().to_hex())
        .unwrap()
        .card;
    let Some(CalendarTiming::DateBased(timing)) = &civil_card.calendar_timing else {
        panic!("civil timing")
    };
    assert_eq!(timing.start().as_str(), "2026-09-05");
    assert_eq!(timing.end_exclusive().unwrap().as_str(), "2026-09-07");
    assert_eq!(civil_card.effective_at, AUTHORED);
    assert!(civil_card.legacy_event_start.is_none() && civil_card.legacy_event_end.is_none());
    let timed_card = &page
        .items
        .iter()
        .find(|item| item.card.source_event_id == timed_event.id().to_hex())
        .unwrap()
        .card;
    let Some(CalendarTiming::TimeBased(timing)) = &timed_card.calendar_timing else {
        panic!("timed timing")
    };
    assert_eq!(timing.start(), AUTHORED + 100);
    assert_eq!(timing.end_exclusive(), Some(AUTHORED + 200));
    assert_eq!(timing.start_tzid().unwrap().as_str(), "America/Vancouver");
    assert_eq!(timing.end_tzid().unwrap().as_str(), "Europe/Paris");
    assert_eq!(timed_card.effective_at, AUTHORED + 100);
    let raw = EventStore::query_raw(
        runtime.client.storage().unwrap(),
        EventQuery::all(EventQueryBounds::first(20).unwrap()),
    )
    .await
    .unwrap();
    assert_eq!(raw.items()[0].event(), &civil);
    assert_eq!(raw.items()[1].event(), &timed_event);
}

#[tokio::test]
async fn every_local_presentation_read_reconstructs_untyped_v1_calendar_without_rewriting_source() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let context = context(None, 1);
    let event = date_event();
    ingest(&runtime, &context, event.clone(), NOW).await;
    let storage = runtime.client.storage().unwrap();
    let generation = projection_generation().unwrap();
    let mut legacy = load_state(storage, &context, generation)
        .await
        .unwrap()
        .unwrap();
    let card_id = legacy.cards[0].card.card_id;
    // Reproduce the old v1 value/hash using only a synthetic historical fixture.
    let old_midnight = chrono::NaiveDate::from_ymd_opt(2026, 9, 5)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp() as u64;
    legacy.cards[0].card.calendar_timing = None;
    legacy.cards[0].card.legacy_event_start = Some(old_midnight);
    legacy.cards[0].card.legacy_event_end = Some(old_midnight + 2 * 86_400);
    legacy.cards[0].card.effective_at = old_midnight;
    legacy.content_generation = content_generation(&legacy).unwrap();
    assert!(
        !String::from_utf8(encode(&legacy).unwrap())
            .unwrap()
            .contains("calendarTiming")
    );
    let before = EventStore::query_raw(
        storage,
        EventQuery::all(EventQueryBounds::first(20).unwrap()),
    )
    .await
    .unwrap();
    for reader in ["page", "search", "me", "reconcile"] {
        store_state(storage, &context, generation, &legacy)
            .await
            .unwrap();
        let selected = match reader {
            "page" => {
                runtime
                    .phase1_today_page(&context, TodayPageRequest::first(20, NOW))
                    .await
                    .unwrap()
                    .items
            }
            "search" => runtime
                .phase1_search(&context, "Harvest", 20, NOW)
                .await
                .unwrap()
                .into_iter()
                .filter_map(|result| result.card)
                .collect(),
            "me" => {
                runtime
                    .phase1_me(&context, &keys().public_key().to_string(), NOW)
                    .await
                    .unwrap()
                    .cards
            }
            "reconcile" => {
                runtime
                    .phase1_today_reconcile(&context, NOW, &[card_id.to_hex()], None)
                    .await
                    .unwrap()
                    .items
            }
            _ => unreachable!(),
        };
        assert_eq!(selected.len(), 1, "{reader}");
        assert_eq!(selected[0].card.card_id, card_id);
        assert_eq!(selected[0].card.source_event_id, event.id().to_hex());
        assert!(matches!(
            selected[0].card.calendar_timing,
            Some(CalendarTiming::DateBased(_))
        ));
        assert_eq!(selected[0].card.effective_at, AUTHORED);
        assert!(selected[0].card.legacy_event_start.is_none());
        let after = EventStore::query_raw(
            storage,
            EventQuery::all(EventQueryBounds::first(20).unwrap()),
        )
        .await
        .unwrap();
        assert_eq!(before, after, "{reader} changed signed history");
    }
}
