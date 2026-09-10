use jiff::{Timestamp, tz::TimeZone};
use radroots_event::calendar::{CalendarDate, IanaTimeZoneId};
use serde::{Deserialize, Serialize};

use super::today::TodayError;

pub const VIEWER_CALENDAR_VERSION: u16 = 1;
pub const VIEWER_TIME_ZONE_MAX_BYTES: usize = 255;

/// Frozen query context, derived only from an explicit zone and actual instant.
/// Event civil dates never pass through this instant conversion.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ViewerCalendarContext {
    version: u16,
    as_of: u64,
    time_zone: IanaTimeZoneId,
    civil_date: CalendarDate,
}

impl ViewerCalendarContext {
    pub fn new(as_of: u64, time_zone: &str) -> Result<Self, TodayError> {
        if as_of == 0 || time_zone.len() > VIEWER_TIME_ZONE_MAX_BYTES {
            return Err(TodayError::InvalidRequest);
        }
        let time_zone = IanaTimeZoneId::parse(time_zone).map_err(|_| TodayError::InvalidRequest)?;
        let (_, bytes) = jiff_tzdb::get(time_zone.as_str()).ok_or(TodayError::InvalidRequest)?;
        // Read the same exact pinned database that validates shared IANA IDs.
        // System timezone discovery and alternate database features are disabled.
        let zone =
            TimeZone::tzif(time_zone.as_str(), bytes).map_err(|_| TodayError::InvalidRequest)?;
        let timestamp = i64::try_from(as_of)
            .ok()
            .and_then(|value| Timestamp::from_second(value).ok())
            .ok_or(TodayError::InvalidRequest)?;
        let date = zone.to_datetime(timestamp).date();
        let civil_date =
            CalendarDate::parse(&date.to_string()).map_err(|_| TodayError::InvalidRequest)?;
        Ok(Self {
            version: VIEWER_CALENDAR_VERSION,
            as_of,
            time_zone,
            civil_date,
        })
    }

    pub const fn version(&self) -> u16 {
        self.version
    }
    pub const fn as_of(&self) -> u64 {
        self.as_of
    }
    pub fn time_zone(&self) -> &str {
        self.time_zone.as_str()
    }
    pub fn civil_date(&self) -> &CalendarDate {
        &self.civil_date
    }
}

impl<'de> Deserialize<'de> for ViewerCalendarContext {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase", deny_unknown_fields)]
        struct Wire {
            version: u16,
            as_of: u64,
            time_zone: IanaTimeZoneId,
            civil_date: CalendarDate,
        }
        let wire = Wire::deserialize(deserializer)?;
        let context =
            Self::new(wire.as_of, wire.time_zone.as_str()).map_err(serde::de::Error::custom)?;
        if wire.version != context.version || wire.civil_date != context.civil_date {
            return Err(serde::de::Error::custom("invalid viewer calendar context"));
        }
        Ok(context)
    }
}

#[cfg(test)]
#[path = "viewer_calendar_tests.rs"]
mod tests;
