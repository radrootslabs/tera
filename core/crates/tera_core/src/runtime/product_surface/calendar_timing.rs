//! Calendar projection values. Signed wire parsing remains owned by Lib.
use radroots_event::calendar::{CalendarDate, IanaTimeZoneId};
use radroots_event_codec::{
    admission::RadrootsAdmittedEvent,
    decode::calendar::{
        admit_radroots_calendar_date_event, admit_radroots_calendar_time_event,
        parse_nip52_calendar_date_event, parse_nip52_calendar_time_event,
    },
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Clone, Copy, Debug, Eq, Error, PartialEq)]
pub enum CalendarTimingError {
    #[error("calendar end must be after its start")]
    InvalidRange,
    #[error("calendar source does not match its admitted contract")]
    InvalidSource,
}

/// A civil interval. No timezone or fabricated instant is part of this value.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DateBasedTiming {
    start: CalendarDate,
    end_exclusive: Option<CalendarDate>,
}

impl DateBasedTiming {
    pub fn new(
        start: CalendarDate,
        end_exclusive: Option<CalendarDate>,
    ) -> Result<Self, CalendarTimingError> {
        if end_exclusive.as_ref().is_some_and(|end| end <= &start) {
            return Err(CalendarTimingError::InvalidRange);
        }
        Ok(Self {
            start,
            end_exclusive,
        })
    }
    pub fn start(&self) -> &CalendarDate {
        &self.start
    }
    pub fn end_exclusive(&self) -> Option<&CalendarDate> {
        self.end_exclusive.as_ref()
    }
}

impl<'de> Deserialize<'de> for DateBasedTiming {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        struct Wire {
            start: CalendarDate,
            end_exclusive: Option<CalendarDate>,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(wire.start, wire.end_exclusive).map_err(serde::de::Error::custom)
    }
}

/// Exact signed instants and their optional source zones. The full u64 wire
/// domain is retained; presentation support is a separate fallible boundary.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimeBasedTiming {
    start: u64,
    end_exclusive: Option<u64>,
    start_tzid: Option<IanaTimeZoneId>,
    end_tzid: Option<IanaTimeZoneId>,
}

impl TimeBasedTiming {
    pub fn new(
        start: u64,
        end_exclusive: Option<u64>,
        start_tzid: Option<IanaTimeZoneId>,
        end_tzid: Option<IanaTimeZoneId>,
    ) -> Result<Self, CalendarTimingError> {
        if end_exclusive.is_some_and(|end| end <= start) {
            return Err(CalendarTimingError::InvalidRange);
        }
        Ok(Self {
            start,
            end_exclusive,
            start_tzid,
            end_tzid,
        })
    }
    pub const fn start(&self) -> u64 {
        self.start
    }
    pub const fn end_exclusive(&self) -> Option<u64> {
        self.end_exclusive
    }
    pub fn start_tzid(&self) -> Option<&IanaTimeZoneId> {
        self.start_tzid.as_ref()
    }
    pub fn end_tzid(&self) -> Option<&IanaTimeZoneId> {
        self.end_tzid.as_ref()
    }
}

impl<'de> Deserialize<'de> for TimeBasedTiming {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields, rename_all = "camelCase")]
        struct Wire {
            start: u64,
            end_exclusive: Option<u64>,
            start_tzid: Option<IanaTimeZoneId>,
            end_tzid: Option<IanaTimeZoneId>,
        }
        let wire = Wire::deserialize(deserializer)?;
        Self::new(
            wire.start,
            wire.end_exclusive,
            wire.start_tzid,
            wire.end_tzid,
        )
        .map_err(serde::de::Error::custom)
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind")]
pub enum CalendarTiming {
    DateBased(DateBasedTiming),
    TimeBased(TimeBasedTiming),
}

impl CalendarTiming {
    pub(crate) fn from_admitted(
        admitted: &RadrootsAdmittedEvent,
    ) -> Result<Option<Self>, CalendarTimingError> {
        if !matches!(
            admitted.contract_id(),
            "radroots.calendar.date_event.v1" | "radroots.calendar.time_event.v1"
        ) {
            return Ok(None);
        }
        let event = admitted.event();
        let tags = event.tags_as_vec();
        match admitted.contract_id() {
            "radroots.calendar.date_event.v1" => {
                let parsed =
                    parse_nip52_calendar_date_event(event.kind_u32(), &tags, event.content())
                        .map_err(|_| CalendarTimingError::InvalidSource)?;
                let value = admit_radroots_calendar_date_event(parsed)
                    .map_err(|_| CalendarTimingError::InvalidSource)?;
                Ok(Some(Self::DateBased(DateBasedTiming::new(
                    value.parsed().start().clone(),
                    value.parsed().end().cloned(),
                )?)))
            }
            "radroots.calendar.time_event.v1" => {
                let parsed =
                    parse_nip52_calendar_time_event(event.kind_u32(), &tags, event.content())
                        .map_err(|_| CalendarTimingError::InvalidSource)?;
                let value = admit_radroots_calendar_time_event(parsed)
                    .map_err(|_| CalendarTimingError::InvalidSource)?;
                let parsed = value.parsed();
                Ok(Some(Self::TimeBased(TimeBasedTiming::new(
                    parsed.start(),
                    parsed.end(),
                    parsed.start_tzid().cloned(),
                    parsed.end_tzid().cloned(),
                )?)))
            }
            _ => Ok(None),
        }
    }
}

#[cfg(test)]
#[path = "calendar_timing_tests.rs"]
mod tests;
