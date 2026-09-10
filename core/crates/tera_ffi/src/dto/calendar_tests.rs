use super::*;
use radroots_event::calendar::IanaTimeZoneId;
use tera_core::runtime::product_surface::{DateBasedTiming, TimeBasedTiming};

#[test]
fn civil_components_keep_shared_leap_dates_and_exclusive_ends() {
    for (value, year, month, day) in [
        ("0001-01-01", 1, 1, 1),
        ("2024-02-29", 2024, 2, 29),
        ("2026-09-05", 2026, 9, 5),
        ("9999-12-31", 9999, 12, 31),
    ] {
        let start = CalendarDate::parse(value).unwrap();
        let timing = DateBasedTiming::new(start, None).unwrap();
        assert_eq!(
            FfiCalendarTiming::from(CalendarTiming::DateBased(timing)),
            FfiCalendarTiming::DateBased {
                start: FfiCivilDate { year, month, day },
                end_exclusive: None,
            }
        );
    }
    let timing = DateBasedTiming::new(
        CalendarDate::parse("2024-02-29").unwrap(),
        Some(CalendarDate::parse("2024-03-01").unwrap()),
    )
    .unwrap();
    let FfiCalendarTiming::DateBased { end_exclusive, .. } =
        CalendarTiming::DateBased(timing).into()
    else {
        panic!("civil variant must survive the native boundary")
    };
    assert_eq!(
        end_exclusive,
        Some(FfiCivilDate {
            year: 2024,
            month: 3,
            day: 1
        })
    );
}

#[test]
fn timed_values_keep_the_entire_unsigned_domain_and_both_source_zones() {
    for start in [0, (1_u64 << 53) + 1, u64::MAX - 1, u64::MAX] {
        let end = start.checked_add(1);
        for zones in [false, true] {
            let start_tzid = zones.then(|| IanaTimeZoneId::parse("America/Vancouver").unwrap());
            let end_tzid = zones.then(|| IanaTimeZoneId::parse("Europe/Paris").unwrap());
            let timing = TimeBasedTiming::new(start, end, start_tzid, end_tzid).unwrap();
            assert_eq!(
                FfiCalendarTiming::from(CalendarTiming::TimeBased(timing)),
                FfiCalendarTiming::TimeBased {
                    start_unix_s: start,
                    end_exclusive_unix_s: end,
                    start_tzid: zones.then(|| "America/Vancouver".to_owned()),
                    end_tzid: zones.then(|| "Europe/Paris".to_owned()),
                }
            );
        }
    }
}
