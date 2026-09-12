use super::{E, Prepared, required};
use crate::runtime::product_surface::{ComposerFormInput, CreateEvent, Phase1DraftEventTiming};
use radroots_event::calendar::{
    AuthoredCalendarDateEvent, AuthoredCalendarTimeEvent, CalendarDate,
};

pub(super) fn command(input: &ComposerFormInput, media: &[Prepared]) -> Result<CreateEvent, E> {
    if media.len() > 1 {
        return Err(E::InvalidInput("event_image_limit"));
    }
    let identifier = required(input.identifier.as_deref(), "event_identifier_required")?;
    let title = required(input.title.as_deref(), "event_title_required")?;
    match input
        .event_timing
        .ok_or(E::InvalidInput("event_timing_required"))?
    {
        Phase1DraftEventTiming::AllDay => {
            let start = CalendarDate::parse(required(
                input.event_start_date.as_deref(),
                "event_start_date_required",
            )?)
            .map_err(|_| E::InvalidInput("invalid_event_start_date"))?;
            let mut event = AuthoredCalendarDateEvent::new(identifier, title, start)
                .map_err(|_| E::InvalidInput("invalid_event"))?;
            if let Some(end) = input.event_end_date.as_deref() {
                event = event
                    .with_end(
                        CalendarDate::parse(end)
                            .map_err(|_| E::InvalidInput("invalid_event_end_date"))?,
                    )
                    .map_err(|_| E::InvalidInput("invalid_event_range"))?;
            }
            if !input.content.is_empty() {
                event = event
                    .with_description(input.content.clone())
                    .map_err(|_| E::InvalidInput("invalid_event_description"))?;
            }
            if let Some(location) = &input.location {
                event = event
                    .with_locations(vec![location.clone()])
                    .map_err(|_| E::InvalidInput("invalid_event_location"))?;
            }
            if let Some(image) = media.first() {
                event = event
                    .with_image(image.image()?)
                    .map_err(|_| E::InvalidInput("invalid_event_image"))?;
            }
            Ok(CreateEvent::date(event))
        }
        Phase1DraftEventTiming::Timed => {
            let start = input
                .event_start_unix_s
                .filter(|value| *value != 0)
                .ok_or(E::InvalidInput("event_start_required"))?;
            let mut event = AuthoredCalendarTimeEvent::new(identifier, title, start)
                .map_err(|_| E::InvalidInput("invalid_event"))?;
            if let Some(end) = input.event_end_unix_s {
                event = event
                    .with_end(end)
                    .map_err(|_| E::InvalidInput("invalid_event_range"))?;
            }
            if let Some(zone) = &input.event_timezone {
                event = event
                    .with_start_tzid(zone)
                    .map_err(|_| E::InvalidInput("invalid_event_timezone"))?;
            }
            if !input.content.is_empty() {
                event = event
                    .with_description(input.content.clone())
                    .map_err(|_| E::InvalidInput("invalid_event_description"))?;
            }
            if let Some(location) = &input.location {
                event = event
                    .with_locations(vec![location.clone()])
                    .map_err(|_| E::InvalidInput("invalid_event_location"))?;
            }
            if let Some(image) = media.first() {
                event = event
                    .with_image(image.image()?)
                    .map_err(|_| E::InvalidInput("invalid_event_image"))?;
            }
            Ok(CreateEvent::time(event))
        }
    }
}
