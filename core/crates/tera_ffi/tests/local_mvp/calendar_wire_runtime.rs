//! Real FFI authoring and fresh SQLite admission of the pinned calendar wire.
use super::*;
use nostr::Filter;
use serde_json::Value;
use tera_ffi::{FfiCalendarTiming, FfiCivilDate};

fn fixtures() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/calendar_wire.v1.json"
    )))
    .unwrap()
}

fn field<'a>(fixture: &'a Value, name: &str) -> &'a str {
    fixture["tags"]
        .as_array()
        .unwrap()
        .iter()
        .find(|tag| tag[0] == name)
        .unwrap()[1]
        .as_str()
        .unwrap()
}

fn input(fixture: &Value) -> FfiAddDraftInput {
    let mut input = add_input(
        FfiAddCommandType::CreateEvent,
        fixture["content"].as_str().unwrap(),
        None,
    );
    input.identifier = Some(field(fixture, "d").into());
    input.title = Some(field(fixture, "title").into());
    input.location = Some(field(fixture, "location").into());
    if fixture["kind"] == 31_922 {
        input.event_timing = Some(FfiEventTimingKind::AllDay);
        input.event_start_date = Some(field(fixture, "start").into());
        input.event_end_date = Some(field(fixture, "end").into());
        // An inactive editor zone must never become a civil wire timestamp/tag.
        input.event_timezone = Some("Pacific/Auckland".into());
    } else {
        input.event_timing = Some(FfiEventTimingKind::Timed);
        input.event_start_unix_s = Some(field(fixture, "start").parse().unwrap());
        input.event_end_unix_s = Some(field(fixture, "end").parse().unwrap());
        input.event_timezone = Some(field(fixture, "start_tzid").into());
    }
    input
}

fn expected_timing(fixture: &Value) -> FfiCalendarTiming {
    if fixture["kind"] == 31_922 {
        FfiCalendarTiming::DateBased {
            start: FfiCivilDate {
                year: 2026,
                month: 9,
                day: 5,
            },
            end_exclusive: Some(FfiCivilDate {
                year: 2026,
                month: 9,
                day: 7,
            }),
        }
    } else {
        let timing = &fixture["timing"]["time"];
        FfiCalendarTiming::TimeBased {
            start_unix_s: timing["start"].as_u64().unwrap(),
            end_exclusive_unix_s: timing["end_exclusive"].as_u64(),
            start_tzid: timing["start_tzid"].as_str().map(str::to_owned),
            end_tzid: timing["end_tzid"].as_str().map(str::to_owned),
        }
    }
}

#[tokio::test]
async fn calendar_wire_survives_ffi_publication_fresh_admission_and_reopen() {
    let suite = fixtures();
    let fixtures = suite["events"].as_array().unwrap();
    assert_eq!(fixtures.len(), 3);
    let relay = MockRelay::run().await.unwrap();
    let relay_url = relay.url().await.to_string();
    let context = local_network(&relay_url);
    let publisher_root = tempfile::tempdir().unwrap();
    support::prepare(publisher_root.path());
    let publisher = runtime_with_signer(publisher_root.path()).await;
    configure_simulator(
        &publisher,
        &relay_url,
        &format!("http://127.0.0.1:{}", unused_loopback_port().await),
    );
    for (index, fixture) in fixtures[..2].iter().enumerate() {
        let id = draft_id(81 + index as u8);
        let saved = publisher
            .phase1_save_draft(
                id.clone(),
                input(fixture),
                AUTHORED_AT,
                None,
                1_800_000_000_000 + index as u64,
            )
            .await
            .unwrap();
        let queued = queue(
            &publisher,
            &id,
            saved.revision,
            &relay_url,
            false,
            1_800_000_000_100 + index as u64,
        )
        .await;
        advance_complete(&publisher, &id, queued.revision).await;
    }
    // The creator has one optional start zone. The independent receive path
    // must also preserve the shared contract's separate optional end zone.
    let client = Client::new(Keys::parse(FIXTURE_SECRET).unwrap());
    client.add_relay(&relay_url).await.unwrap();
    client.connect().await;
    client.wait_for_connection(Duration::from_secs(2)).await;
    let external = &fixtures[2];
    let wire_tags: Vec<Vec<String>> = serde_json::from_value(external["tags"].clone()).unwrap();
    client
        .send_event_builder(
            EventBuilder::new(Kind::from(31_923), external["content"].as_str().unwrap())
                .tags(wire_tags.into_iter().map(|tag| Tag::parse(tag).unwrap()))
                .custom_created_at(Timestamp::from_secs(AUTHORED_AT)),
        )
        .await
        .unwrap();
    let events = client
        .fetch_events(
            Filter::new().kinds([Kind::from(31_922), Kind::from(31_923)]),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(events.len(), 3);
    for fixture in fixtures {
        let event = events
            .iter()
            .find(|event| event.content == fixture["content"].as_str().unwrap())
            .unwrap();
        event.verify().expect("published wire signature and id");
        assert_eq!(
            u64::from(event.kind.as_u16()),
            fixture["kind"].as_u64().unwrap()
        );
        assert_eq!(serde_json::to_value(&event.tags).unwrap(), fixture["tags"]);
    }
    let reader_root = tempfile::tempdir().unwrap();
    support::prepare(reader_root.path());
    let reader = runtime_with_signer(reader_root.path()).await;
    reader
        .configure_simulator_relays(vec![relay_url.clone()])
        .unwrap();
    let sync = reader
        .phase1_sync_today(context.clone(), AS_OF, FfiTodayProjectionUpdate::Rebuild)
        .await
        .unwrap();
    assert_eq!(sync.relay_state, FfiTodayRelaySyncState::Complete);
    assert_eq!(sync.events_admitted, 3);
    let cards = collect_pages(&reader, &context, 1, AS_OF).await;
    assert_eq!(cards.len(), 3);
    for fixture in fixtures {
        let event = events
            .iter()
            .find(|event| event.content == fixture["content"].as_str().unwrap())
            .unwrap();
        let card = cards
            .iter()
            .find(|card| card.source_event_id == event.id.to_hex())
            .unwrap();
        assert_eq!(card.card_type, FfiTodayCardType::Event);
        assert_eq!(card.calendar_timing, Some(expected_timing(fixture)));
    }
    reader.shutdown().await.unwrap();
    client.shutdown().await;
    publisher.shutdown().await.unwrap();
    relay.shutdown();
    let reopened = runtime_with_signer(reader_root.path()).await;
    reopened
        .configure_simulator_relays(vec![relay_url])
        .unwrap();
    let cached = collect_pages(&reopened, &context, 1, AS_OF).await;
    assert_eq!(
        cached, cards,
        "offline reopen preserves exact source and typed timing"
    );
    reopened.shutdown().await.unwrap();
}
