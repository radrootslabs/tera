//! Application materialization coordinated by the existing storage rebuild owner.
//! Generation documents remain separate; only owner promotion exposes new state.
use super::*;
use radroots_storage::{
    event::{AdmissionStage, SourceGeneration},
    projection::{
        InvalidationReason, ProjectionHealth, ProjectionInvalidation, RawSourceDigest,
        RebuildFailure, RebuildStage, RebuildTicket, RebuildTicketId, RebuildTransition,
    },
};

const LEGACY_GENERATION_DOMAIN: &[u8] = b"radroots.today-projection.v1\0";
const LEGACY_CONTENT_DOMAIN: &[u8] = b"radroots.today-content-generation.v1\0";
const SOURCE_DIGEST_DOMAIN: &[u8] = b"tera.today.calendar-rebuild-source.v1\0";

pub(super) fn legacy_generation() -> Result<ProjectionGeneration, TodayError> {
    Ok(ProjectionGeneration::new(
        Sha256::digest(LEGACY_GENERATION_DOMAIN).into(),
    )?)
}

pub(super) async fn ready(storage: &dyn radroots_storage::Storage) -> Result<bool, TodayError> {
    let Some(status) = ProjectionStore::status(storage, projection_id()?).await? else {
        return Ok(true);
    };
    if status.generation() == projection_generation()? {
        return Ok(status.health() == ProjectionHealth::Ready);
    }
    if status.generation() == legacy_generation()? {
        return Ok(false);
    }
    Err(TodayError::UnsupportedProjectionVersion)
}

/// Historical cards supply only validated local media and overlays. Their
/// timing and payload are reconstructed from canonical events, never promoted.
pub(super) async fn legacy_state(
    storage: &dyn radroots_storage::Storage,
    context: &LocalNetwork,
) -> Result<Option<TodayProjectionState>, TodayError> {
    let document = ProjectionStore::projection_document(
        storage,
        projection_id()?,
        legacy_generation()?,
        projection_document_key(context),
    )
    .await?;
    let Some(document) = document else {
        return Ok(None);
    };
    verify_legacy_content_generation(document.value(), 1, LEGACY_CONTENT_DOMAIN)?;
    let mut value: serde_json::Value = decode(document.value())?;
    migrate_legacy_media_values(&mut value)?;
    let object = value.as_object_mut().ok_or(TodayError::CorruptProjection)?;
    object.entry("mediaCache").or_insert(
        serde_json::to_value(Phase1MediaCacheIndex::default())
            .map_err(|_| TodayError::Serialization)?,
    );
    if let Some(cards) = object
        .get_mut("cards")
        .and_then(serde_json::Value::as_array_mut)
    {
        for entry in cards {
            let card = entry
                .get_mut("card")
                .and_then(serde_json::Value::as_object_mut)
                .ok_or(TodayError::CorruptProjection)?;
            card.remove("eventStart");
            card.remove("eventEnd");
            card.remove("calendarTiming");
        }
    }
    let state: TodayProjectionState =
        serde_json::from_value(value).map_err(|_| TodayError::CorruptProjection)?;
    if state.context_id != context.id.as_str() || state.context_generation != context.generation {
        return Err(TodayError::CorruptProjection);
    }
    validate_media_state(&state)?;
    Ok(Some(state))
}

#[derive(Eq, PartialEq)]
pub(super) struct SourceSnapshot {
    pub(super) generation: SourceGeneration,
    pub(super) high_water: Option<EventPosition>,
    pub(super) digest: RawSourceDigest,
}

impl SourceSnapshot {
    fn matches(&self, ticket: &RebuildTicket) -> bool {
        self.generation == ticket.source_generation()
            && self.high_water == ticket.source_high_water()
            && self.digest == ticket.source_digest()
    }
}

/// A bounded streaming digest detects both appended and same-count admission
/// changes. Storage continues to own all signature, replacement and deletion data.
pub(super) async fn source_snapshot(
    storage: &dyn radroots_storage::Storage,
) -> Result<SourceSnapshot, TodayError> {
    let mut hash = Sha256::new();
    hash.update(SOURCE_DIGEST_DOMAIN);
    let mut generation = None;
    let mut high_water = None;
    let mut after = None;
    let mut count = 0_u64;
    loop {
        let mut bounds = EventQueryBounds::first(radroots_storage::event::EVENT_QUERY_LIMIT_MAX)?;
        if let Some(cursor) = after {
            bounds = bounds.after(cursor);
        }
        let page = EventStore::query_raw(storage, EventQuery::all(bounds)).await?;
        if generation
            .replace(page.generation())
            .is_some_and(|prior| prior != page.generation())
        {
            return Err(radroots_storage::Error::SourceGenerationChanged.into());
        }
        hash.update(page.generation().as_bytes());
        for event in page.items() {
            count = count.checked_add(1).ok_or(TodayError::InvalidRequest)?;
            if count > radroots_sync::projection::PROJECTION_RAW_SOURCE_MAX_EVENTS {
                return Err(TodayError::InvalidRequest);
            }
            hash.update(event.position().sequence().get().to_be_bytes());
            hash.update([match event.stage() {
                AdmissionStage::Raw => 0,
                AdmissionStage::Verified => 1,
                AdmissionStage::Visible => 2,
            }]);
            let raw = event.event().raw_json().as_bytes();
            hash.update(
                u64::try_from(raw.len())
                    .map_err(|_| TodayError::InvalidRequest)?
                    .to_be_bytes(),
            );
            hash.update(raw);
            high_water = Some(event.position());
        }
        after = page.next_cursor();
        if after.is_none() {
            break;
        }
    }
    Ok(SourceSnapshot {
        generation: generation.ok_or(TodayError::CorruptProjection)?,
        high_water,
        digest: RawSourceDigest::new(hash.finalize().into()),
    })
}

pub(super) async fn begin(
    storage: &dyn radroots_storage::Storage,
    now: u64,
    expected: &radroots_storage::status::EventStoreStatus,
) -> Result<Option<RebuildTicket>, TodayError> {
    let Some(status) = ProjectionStore::status(storage, projection_id()?).await? else {
        return Ok(None);
    };
    let replacement = projection_generation()?;
    if status.generation() == replacement && status.health() == ProjectionHealth::Ready {
        return Ok(None);
    }
    if status.generation() != legacy_generation()? {
        return Err(TodayError::UnsupportedProjectionVersion);
    }
    let source = source_snapshot(storage).await?;
    if source.generation != expected.generation()
        || source
            .high_water
            .map_or(0, |position| position.sequence().get())
            != expected.raw_events()
    {
        return Err(radroots_storage::Error::SourceGenerationChanged.into());
    }
    let now = now.max(
        status
            .checkpoint()
            .map_or(0, ProjectionCheckpoint::updated_at_unix_ms),
    );
    let mut ticket = if let Some(id) = status.active_rebuild() {
        let ticket = ProjectionStore::rebuild(storage, id)
            .await?
            .ok_or(TodayError::CorruptProjection)?;
        if ticket.invalidation().replacement_generation() != replacement
            || ticket.invalidation().invalid_generation() != status.generation()
            || ticket.invalidation().projection_id() != status.projection_id()
        {
            return Err(TodayError::UnsupportedProjectionVersion);
        }
        if !source.matches(&ticket) {
            fail_changed_source(storage, &ticket, now).await?;
            return Err(radroots_storage::Error::SourceGenerationChanged.into());
        }
        ticket
    } else {
        let invalidation =
            match ProjectionStore::invalidation(storage, projection_id()?, replacement).await? {
                Some(value) if value.invalid_generation() == status.generation() => value,
                Some(_) => return Err(TodayError::UnsupportedProjectionVersion),
                None => ProjectionInvalidation::new(
                    projection_id()?,
                    status.generation(),
                    replacement,
                    InvalidationReason::ProjectionGenerationChanged,
                    now,
                )?,
            };
        if status.health() == ProjectionHealth::Ready {
            ProjectionStore::invalidate(storage, invalidation.clone()).await?;
        } else if status.health() != ProjectionHealth::Invalidated {
            return Err(TodayError::CorruptProjection);
        }
        ProjectionStore::request_rebuild(
            storage,
            RebuildTicket::requested(
                RebuildTicketId::new(*uuid::Uuid::new_v4().as_bytes())?,
                invalidation,
                source.generation,
                source.high_water,
                source.digest,
            )?,
        )
        .await?
    };
    if ticket.stage() == RebuildStage::Requested {
        ticket = ProjectionStore::transition_rebuild(
            storage,
            RebuildTransition::start(
                ticket.ticket_id(),
                ticket.revision(),
                now.max(ticket.updated_at_unix_ms()),
            ),
        )
        .await?;
    }
    if ticket.stage() != RebuildStage::Running {
        return Err(TodayError::CorruptProjection);
    }
    Ok(Some(ticket))
}

async fn fail_changed_source(
    storage: &dyn radroots_storage::Storage,
    ticket: &RebuildTicket,
    now: u64,
) -> Result<(), TodayError> {
    ProjectionStore::transition_rebuild(
        storage,
        RebuildTransition::fail(
            ticket.ticket_id(),
            ticket.revision(),
            now.max(ticket.updated_at_unix_ms()),
            RebuildFailure::SourceChanged,
        ),
    )
    .await?;
    Ok(())
}

pub(super) async fn complete(
    storage: &dyn radroots_storage::Storage,
    ticket: &RebuildTicket,
    checkpoint: ProjectionCheckpoint,
) -> Result<(), TodayError> {
    let now = checkpoint
        .updated_at_unix_ms()
        .max(ticket.updated_at_unix_ms());
    if !source_snapshot(storage).await?.matches(ticket) {
        fail_changed_source(storage, ticket, now).await?;
        return Err(radroots_storage::Error::SourceGenerationChanged.into());
    }
    match ProjectionStore::transition_rebuild(
        storage,
        RebuildTransition::complete(ticket.ticket_id(), ticket.revision(), now, checkpoint),
    )
    .await
    {
        Ok(_) => Ok(()),
        Err(radroots_storage::Error::SourceGenerationChanged) => {
            fail_changed_source(storage, ticket, now).await?;
            Err(radroots_storage::Error::SourceGenerationChanged.into())
        }
        Err(error) => Err(error.into()),
    }
}
