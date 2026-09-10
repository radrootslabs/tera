use super::*;

fn instant(value: &str) -> u64 {
    chrono::DateTime::parse_from_rfc3339(value)
        .unwrap()
        .timestamp()
        .try_into()
        .unwrap()
}

#[test]
fn explicit_zone_derives_local_midnight_and_date_line_context() {
    let as_of = instant("2026-09-05T00:30:00Z");
    for (zone, expected) in [
        ("America/Vancouver", "2026-09-04"),
        ("UTC", "2026-09-05"),
        ("Pacific/Kiritimati", "2026-09-05"),
    ] {
        let context = ViewerCalendarContext::new(as_of, zone).unwrap();
        assert_eq!(context.time_zone(), zone);
        assert_eq!(context.civil_date().as_str(), expected);
        assert_eq!(context.as_of(), as_of);
    }
    for (value, expected) in [
        ("2024-03-10T09:59:59Z", "2024-03-10"),
        ("2024-03-10T10:00:00Z", "2024-03-10"),
        ("2024-11-03T08:30:17Z", "2024-11-03"),
        ("2024-11-03T09:30:17Z", "2024-11-03"),
        ("2024-03-01T07:59:59Z", "2024-02-29"),
        ("2024-03-01T08:00:00Z", "2024-03-01"),
    ] {
        assert_eq!(
            ViewerCalendarContext::new(instant(value), "America/Vancouver")
                .unwrap()
                .civil_date()
                .as_str(),
            expected
        );
    }
    assert_eq!(
        ViewerCalendarContext::new(instant("2011-12-30T10:00:00Z"), "Pacific/Apia")
            .unwrap()
            .civil_date()
            .as_str(),
        "2011-12-31"
    );
}

#[test]
fn unsupported_numbers_zones_and_serialized_contexts_fail_closed() {
    for as_of in [0, u64::MAX, i64::MAX as u64, 253_402_300_800] {
        assert!(ViewerCalendarContext::new(as_of, "UTC").is_err());
    }
    for zone in [
        "",
        "utc",
        "Invalid/Zone",
        " UTC",
        "America/Vancouver\0",
        &"x".repeat(256),
    ] {
        assert!(ViewerCalendarContext::new(1, zone).is_err());
    }
    let original =
        ViewerCalendarContext::new(instant("2026-09-05T00:30:00Z"), "America/Vancouver").unwrap();
    let value = serde_json::to_value(&original).unwrap();
    assert_eq!(
        serde_json::from_value::<ViewerCalendarContext>(value.clone()).unwrap(),
        original
    );
    for (key, replacement) in [
        ("version", serde_json::json!(2)),
        ("asOf", serde_json::json!(0)),
        ("civilDate", serde_json::json!("2026-09-05")),
        ("timeZone", serde_json::json!("UTC")),
        ("extra", serde_json::json!(true)),
    ] {
        let mut invalid = value.clone();
        invalid[key] = replacement;
        assert!(serde_json::from_value::<ViewerCalendarContext>(invalid).is_err());
    }
}
