//! Output values retain civil components separately from exact wire instants.
use radroots_event::calendar::CalendarDate;
use tera_core::runtime::product_surface::CalendarTiming;

pub const TODAY_CARD_FFI_SCHEMA_VERSION: u16 = 2;

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiCivilDate {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl From<&CalendarDate> for FfiCivilDate {
    fn from(value: &CalendarDate) -> Self {
        // The shared private representation guarantees ten canonical ASCII
        // bytes and a valid Gregorian date. This is component translation only.
        let bytes = value.as_str().as_bytes();
        let digit = |index: usize| u16::from(bytes[index] - b'0');
        Self {
            year: digit(0) * 1_000 + digit(1) * 100 + digit(2) * 10 + digit(3),
            month: (digit(5) * 10 + digit(6)) as u8,
            day: (digit(8) * 10 + digit(9)) as u8,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiCalendarTiming {
    DateBased {
        start: FfiCivilDate,
        end_exclusive: Option<FfiCivilDate>,
    },
    TimeBased {
        start_unix_s: u64,
        end_exclusive_unix_s: Option<u64>,
        start_tzid: Option<String>,
        end_tzid: Option<String>,
    },
}

impl From<CalendarTiming> for FfiCalendarTiming {
    fn from(value: CalendarTiming) -> Self {
        match value {
            CalendarTiming::DateBased(timing) => Self::DateBased {
                start: timing.start().into(),
                end_exclusive: timing.end_exclusive().map(Into::into),
            },
            CalendarTiming::TimeBased(timing) => Self::TimeBased {
                start_unix_s: timing.start(),
                end_exclusive_unix_s: timing.end_exclusive(),
                start_tzid: timing.start_tzid().map(|zone| zone.as_str().to_owned()),
                end_tzid: timing.end_tzid().map(|zone| zone.as_str().to_owned()),
            },
        }
    }
}

#[cfg(test)]
#[path = "calendar_tests.rs"]
mod tests;
