//! Consumer conformance against the exact shared owner selected by Cargo.lock.
use radroots_event::calendar::{
    AuthoredCalendarDateEvent, AuthoredCalendarTimeEvent, CalendarDate,
};
use radroots_event_codec::{
    admission::admit_verified_event,
    decode::calendar::{
        admit_radroots_calendar_date_event, admit_radroots_calendar_time_event,
        parse_nip52_calendar_date_event, parse_nip52_calendar_time_event,
    },
    encode::calendar::time_to_wire_parts,
    verify::verify_nip01_event,
};
use serde_json::Value;

use super::tests::{keys, signed_owned};
use crate::runtime::product_surface::{CreateEvent, Phase1AddCommand};

fn fixtures() -> Value {
    serde_json::from_str(include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/calendar_wire.v1.json"
    )))
    .expect("standalone calendar fixtures")
}

fn tags(fixture: &Value) -> Vec<Vec<String>> {
    serde_json::from_value(fixture["tags"].clone()).unwrap()
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

#[test]
fn authored_calendar_plans_match_current_wire_and_signature_admission() {
    let suite = fixtures();
    assert_eq!(suite["schema_version"], 1);
    assert_eq!(
        suite["provenance"]["registry_version"],
        radroots_event::contract::RegistryVersion::CURRENT.get()
    );
    assert_eq!(suite["events"].as_array().unwrap().len(), 3);
    for fixture in suite["events"].as_array().unwrap() {
        let content = fixture["content"].as_str().unwrap();
        let event = if fixture["kind"] == 31_922 {
            CreateEvent::date(
                AuthoredCalendarDateEvent::new(
                    field(fixture, "d"),
                    field(fixture, "title"),
                    CalendarDate::parse(field(fixture, "start")).unwrap(),
                )
                .unwrap()
                .with_end(CalendarDate::parse(field(fixture, "end")).unwrap())
                .unwrap()
                .with_description(content)
                .unwrap()
                .with_locations(vec![field(fixture, "location").to_owned()])
                .unwrap(),
            )
        } else {
            let mut event = AuthoredCalendarTimeEvent::new(
                field(fixture, "d"),
                field(fixture, "title"),
                field(fixture, "start").parse().unwrap(),
            )
            .unwrap()
            .with_end(field(fixture, "end").parse().unwrap())
            .unwrap()
            .with_start_tzid(field(fixture, "start_tzid"))
            .unwrap()
            .with_description(content)
            .unwrap()
            .with_locations(vec![field(fixture, "location").to_owned()])
            .unwrap();
            if let Some(zone) = fixture["timing"]["time"]["end_tzid"].as_str() {
                event = event.with_end_tzid(zone).unwrap();
            }
            CreateEvent::time(event)
        };
        let plan = Phase1AddCommand::CreateEvent(event)
            .authored_plan(1_786_000_000, keys().public_key().to_string())
            .unwrap();
        assert_eq!(
            u64::from(plan.body().kind()),
            fixture["kind"].as_u64().unwrap()
        );
        assert_eq!(plan.body().tags(), tags(fixture));
        assert_eq!(plan.body().content(), content);
        let signed = signed_owned(
            plan.body().kind(),
            plan.body().tags().to_vec(),
            content,
            1_786_000_000,
        );
        let admitted =
            admit_verified_event(verify_nip01_event(signed.envelope().clone()).unwrap()).unwrap();
        assert_eq!(
            admitted.contract_id(),
            fixture["contract_id"].as_str().unwrap()
        );
        assert_eq!(admitted.event().tags_as_vec(), tags(fixture));
        assert_eq!(
            admitted.event(),
            signed.envelope(),
            "admission preserves signed source"
        );
    }
}

fn reject(kind: u32, tags: Vec<Vec<String>>) {
    let signed = signed_owned(kind, tags, "calendar conformance", 1_786_000_000);
    let verified =
        verify_nip01_event(signed.envelope().clone()).expect("valid signed malformed profile");
    assert!(
        admit_verified_event(verified).is_err(),
        "malformed calendar profile admitted"
    );
}

fn replace(tags: &mut [Vec<String>], name: &str, value: &str) {
    tags.iter_mut().find(|tag| tag[0] == name).unwrap()[1] = value.to_owned();
}

#[test]
fn signed_calendar_admission_rejects_noncanonical_cardinality_and_fields() {
    for fixture in fixtures()["events"].as_array().unwrap() {
        let kind = fixture["kind"].as_u64().unwrap() as u32;
        for name in ["d", "title", "start"] {
            let mut missing = tags(fixture);
            missing.retain(|tag| tag[0] != name);
            reject(kind, missing);
        }
        // Each scalar in these fixtures is singular, exactly two elements.
        for scalar in tags(fixture).iter().filter(|tag| tag[0] != "D") {
            let mut duplicate = tags(fixture);
            duplicate.push(scalar.clone());
            // location is intentionally repeatable in the shared contract.
            if scalar[0] != "location" {
                reject(kind, duplicate);
            }
            let mut arity = tags(fixture);
            arity
                .iter_mut()
                .find(|tag| tag[0] == scalar[0])
                .unwrap()
                .push("extra".into());
            reject(kind, arity);
        }
        let mut equal_end = tags(fixture);
        replace(&mut equal_end, "end", field(fixture, "start"));
        reject(kind, equal_end);
        if kind == 31_922 {
            let mut forbidden = tags(fixture);
            forbidden.push(vec!["D".into(), "20833".into()]);
            reject(kind, forbidden);
            let mut invalid = tags(fixture);
            replace(&mut invalid, "start", "2026-02-29");
            reject(kind, invalid);
        } else {
            for value in ["01800000000", "-1", "18446744073709551616"] {
                let mut invalid = tags(fixture);
                replace(&mut invalid, "start", value);
                reject(kind, invalid);
            }
            for value in ["america/vancouver", "Mars/Olympus", "UTC+03"] {
                let mut invalid = tags(fixture);
                replace(&mut invalid, "start_tzid", value);
                reject(kind, invalid);
            }
            let mut missing = tags(fixture);
            missing.retain(|tag| tag[0] != "D");
            reject(kind, missing);
            let mut duplicate = tags(fixture);
            duplicate.push(vec!["D".into(), "20833".into()]);
            reject(kind, duplicate);
        }
    }
}

#[test]
fn baseline_observation_does_not_weaken_strict_calendar_day_coverage() {
    let suite = fixtures();
    let fixture = &suite["events"][1];
    for values in [
        vec!["20833"],
        vec!["20834", "20833"],
        vec!["20833", "20833", "20834"],
        vec!["020833", "20834"],
    ] {
        let mut partial = tags(fixture);
        partial.retain(|tag| tag[0] != "D");
        partial.extend(values.into_iter().map(|day| vec!["D".into(), day.into()]));
        let parsed = parse_nip52_calendar_time_event(31_923, &partial, "")
            .expect("bounded in-range observed days");
        assert!(admit_radroots_calendar_time_event(parsed).is_err());
        reject(31_923, partial);
    }
    let mut absent = tags(fixture);
    absent.retain(|tag| tag[0] != "D");
    assert!(parse_nip52_calendar_time_event(31_923, &absent, "").is_err());
    let mut date = tags(&suite["events"][0]);
    date.push(vec!["D".into(), "20833".into()]);
    let parsed = parse_nip52_calendar_date_event(31_922, &date, "").unwrap();
    assert!(admit_radroots_calendar_date_event(parsed).is_err());
}

#[test]
fn timed_encoder_keeps_exclusive_midnight_and_full_width_bounded_days() {
    for (start, end, expected) in [
        (1_800_000_000, None, vec!["20833".to_owned()]),
        (1_800_000_000, Some(1_800_057_600), vec!["20833".to_owned()]),
        (
            1_800_000_000,
            Some(1_800_057_601),
            vec!["20833".to_owned(), "20834".to_owned()],
        ),
        (u64::MAX, None, vec![(u64::MAX / 86_400).to_string()]),
        (
            0,
            Some(366 * 86_400),
            (0..366).map(|day| day.to_string()).collect(),
        ),
    ] {
        let mut event = AuthoredCalendarTimeEvent::new("boundary", "Boundary", start).unwrap();
        if let Some(end) = end {
            event = event.with_end(end).unwrap();
        }
        let wire = time_to_wire_parts(&event).unwrap();
        assert_eq!(wire.kind, 31_923);
        assert_eq!(
            wire.tags
                .iter()
                .filter(|tag| tag[0] == "D")
                .map(|tag| tag[1].clone())
                .collect::<Vec<_>>(),
            expected
        );
        let signed = signed_owned(wire.kind, wire.tags, &wire.content, 1_786_000_000);
        admit_verified_event(verify_nip01_event(signed.envelope().clone()).unwrap()).unwrap();
    }
    assert!(
        AuthoredCalendarTimeEvent::new("boundary", "Boundary", 0)
            .unwrap()
            .with_end(367 * 86_400)
            .is_err()
    );
}
