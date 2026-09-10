use super::*;

fn date(value: &str) -> CalendarDate {
    CalendarDate::parse(value).unwrap()
}

#[test]
fn civil_dates_keep_gregorian_invariants_and_exclusive_ends() {
    for value in ["0001-01-01", "2000-02-29", "2028-02-29", "9999-12-31"] {
        let timing = CalendarTiming::DateBased(DateBasedTiming::new(date(value), None).unwrap());
        let json = serde_json::to_string(&timing).unwrap();
        assert_eq!(
            json,
            format!(r#"{{"kind":"DateBased","start":"{value}","endExclusive":null}}"#)
        );
        assert_eq!(
            serde_json::from_str::<CalendarTiming>(&json).unwrap(),
            timing
        );
    }
    for invalid in [
        "0000-01-01",
        "1900-02-29",
        "2026-02-29",
        "2026-04-31",
        "2026-13-01",
        "2026-9-05",
        "2026-09-05Z",
    ] {
        assert!(
            serde_json::from_value::<CalendarTiming>(serde_json::json!({
                "kind": "DateBased", "start": invalid, "endExclusive": null,
            }))
            .is_err()
        );
    }
    for end in ["2026-09-05", "2026-09-04"] {
        assert!(DateBasedTiming::new(date("2026-09-05"), Some(date(end))).is_err());
        assert!(
            serde_json::from_value::<CalendarTiming>(serde_json::json!({
                "kind": "DateBased", "start": "2026-09-05", "endExclusive": end,
            }))
            .is_err()
        );
    }
    let single = DateBasedTiming::new(date("2026-09-05"), Some(date("2026-09-06"))).unwrap();
    assert_eq!(single.start().as_str(), "2026-09-05");
    assert_eq!(single.end_exclusive().unwrap().as_str(), "2026-09-06");
    let multiple = DateBasedTiming::new(date("2028-02-28"), Some(date("2028-03-01"))).unwrap();
    assert_eq!(multiple.end_exclusive().unwrap().as_str(), "2028-03-01");
}

#[test]
fn timing_serialization_cannot_mix_civil_dates_instants_or_zones() {
    for value in [
        serde_json::json!({"kind":"DateBased", "start":1800000000}),
        serde_json::json!({"kind":"DateBased", "start":"2026-09-05", "startTzid":"America/Vancouver"}),
        serde_json::json!({"kind":"TimeBased", "start":"2026-09-05"}),
        serde_json::json!({"kind":"TimeBased", "start":-1}),
        serde_json::json!({"kind":"TimeBased", "start":1.5}),
        serde_json::json!({"kind":"Unsupported", "start":1}),
        serde_json::json!({"kind":"TimeBased", "start":1, "endExclusive":1}),
        serde_json::json!({"kind":"TimeBased", "start":1, "startTzid":"not/a-zone"}),
    ] {
        assert!(serde_json::from_value::<CalendarTiming>(value).is_err());
    }
}

#[test]
fn timed_values_preserve_exact_wire_integer_domain_and_source_zones() {
    for start in [0, 1, i64::MAX as u64, u64::MAX] {
        let timing =
            CalendarTiming::TimeBased(TimeBasedTiming::new(start, None, None, None).unwrap());
        let encoded = serde_json::to_string(&timing).unwrap();
        assert_eq!(
            serde_json::from_str::<CalendarTiming>(&encoded).unwrap(),
            timing
        );
    }
    let start_zone = IanaTimeZoneId::parse("America/Vancouver").unwrap();
    let end_zone = IanaTimeZoneId::parse("Europe/Paris").unwrap();
    let value = TimeBasedTiming::new(
        1_800_000_000,
        Some(1_800_003_600),
        Some(start_zone),
        Some(end_zone),
    )
    .unwrap();
    assert_eq!(value.start(), 1_800_000_000);
    assert_eq!(value.end_exclusive(), Some(1_800_003_600));
    assert_eq!(value.start_tzid().unwrap().as_str(), "America/Vancouver");
    assert_eq!(value.end_tzid().unwrap().as_str(), "Europe/Paris");
    let timing = CalendarTiming::TimeBased(value);
    let encoded = serde_json::to_string(&timing).unwrap();
    assert_eq!(
        serde_json::from_str::<CalendarTiming>(&encoded).unwrap(),
        timing
    );
    assert!(TimeBasedTiming::new(u64::MAX, Some(u64::MAX), None, None).is_err());
    assert!(
        serde_json::from_str::<CalendarTiming>(
            r#"{"kind":"TimeBased","start":18446744073709551616}"#
        )
        .is_err()
    );
}

#[test]
fn date_relevance_uses_explicit_civil_context_and_exclusive_end() {
    use crate::runtime::product_surface::{
        CardId, ContextRank, TimeRelevance, TodayCardType, TodayRank, TodayRankInput,
    };
    for (start, end, as_of_date, expected) in [
        ("2026-09-05", None, "2026-09-05", 4),
        ("2026-09-05", None, "2026-09-06", 0),
        ("2026-09-05", Some("2026-09-07"), "2026-09-06", 4),
        ("2026-09-05", Some("2026-09-07"), "2026-09-07", 0),
        ("2026-09-12", None, "2026-09-05", 3),
        ("2026-09-13", None, "2026-09-05", 2),
        ("2028-02-28", Some("2028-03-01"), "2028-02-29", 4),
        ("2028-02-28", Some("2028-03-01"), "2028-03-01", 0),
    ] {
        let rank = TodayRank::derive(TodayRankInput {
            card_type: TodayCardType::Event,
            context_rank: ContextRank::LocalityMatch,
            as_of: 1,
            effective_at: 1,
            time: TimeRelevance::DateBased {
                event: DateBasedTiming::new(date(start), end.map(date)).unwrap(),
                as_of_date: date(as_of_date),
            },
            card_id: CardId::parse(&"a".repeat(64)).unwrap(),
        })
        .unwrap();
        assert_eq!(
            rank.time_relevance_rank, expected,
            "{start}/{end:?}/{as_of_date}"
        );
    }
}
