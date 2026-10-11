//! Bounded application staging over the canonical owner. Caller holds Today mutex.
use super::*;
use radroots_storage::{
    event::VisibilityIndexBounds,
    projection::{
        InvalidationReason, ProjectionHealth, ProjectionInvalidation,
        batch::ProjectionDocumentBatch,
        cache::{ProjectionCacheLimits, ProjectionCacheScope},
    },
};
use std::time::{Duration, Instant};
#[path = "today_build_progress.rs"]
pub(super) mod progress;
pub(super) use progress::BuildProgress;

const SOURCE_ROWS: u16 = 63;
const WORK_BYTES: u64 = 16 * 1024 * 1024;

pub(super) enum BuildOutcome {
    Complete(Box<BuildProgress>),
    Pending,
}

pub(super) async fn run(
    runtime: &TeraRuntime,
    context: &LocalNetwork,
    query_scope: [u8; 32],
    now_unix_ms: u64,
) -> Result<BuildOutcome, TodayError> {
    let deadline = Instant::now() + Duration::from_secs(5);
    let storage = runtime
        .client
        .storage()
        .map_err(|_| TodayError::RuntimeUnavailable)?;
    let policy = runtime.load_author_visibility().await?;
    let policy_digest = policy.cache_digest()?;
    let source = loop {
        if Instant::now() >= deadline {
            return Ok(BuildOutcome::Pending);
        }
        let status =
            EventStore::maintain_visibility(storage, VisibilityIndexBounds::new(64, WORK_BYTES)?)
                .await?;
        if status.is_complete() {
            break status.token();
        }
    };
    let id = ProjectionId::parse(progress::ID)?;
    let mut active_status = ProjectionStore::status(storage, id.clone()).await?;
    if let Some(status) = active_status.as_ref()
        && status.health() == ProjectionHealth::Invalidated
        && status.active_rebuild().is_none()
    {
        retire_active(runtime, storage, source, status.generation()).await?;
        active_status = None;
    }
    if active_status
        .as_ref()
        .is_some_and(|value| value.health() != ProjectionHealth::Ready)
    {
        return Err(TodayError::CorruptProjection);
    }
    let mut active = active_status.as_ref().map(|value| value.generation());
    let status = EventStore::status(storage).await?;
    if EventStore::source_token(storage).await? != source {
        return Err(radroots_storage::Error::SourceEpochChanged.into());
    }
    if let Some(generation) = active {
        let header = progress::load(storage, id.clone(), generation, progress::HEADER)
            .await?
            .ok_or(TodayError::CorruptProjection)?;
        if !header.complete || header.generation != generation {
            return Err(TodayError::CorruptProjection);
        }
        if header.source == source
            && header.query_scope == query_scope
            && header.policy == policy_digest
        {
            if EventStore::source_token(storage).await? != source {
                return Err(radroots_storage::Error::SourceEpochChanged.into());
            }
            return Ok(BuildOutcome::Complete(Box::new(header)));
        }
        // An interrupted legacy upgrade may retain two protected identities.
        // Its incompatible application cache can release metadata protection;
        // all old payloads remain, and its Invalidated status resumes on reopen.
        if let Some(legacy) = ProjectionStore::status(storage, projection_id()?).await?
            && !coordination_retirement::retired(storage, &legacy).await?
        {
            let replacement = BuildProgress::new(
                source,
                query_scope,
                policy_digest,
                None,
                status.raw_events(),
            )?;
            let write = runtime.today_projection_lock.begin_write();
            ProjectionStore::invalidate(
                storage,
                ProjectionInvalidation::new(
                    id.clone(),
                    generation,
                    replacement.generation,
                    InvalidationReason::ProjectionGenerationChanged,
                    now_unix_ms.max(
                        active_status
                            .as_ref()
                            .and_then(|value| value.checkpoint())
                            .map_or(0, ProjectionCheckpoint::updated_at_unix_ms),
                    ),
                )?,
            )
            .await?;
            write.complete();
            retire_active(runtime, storage, source, generation).await?;
            active = None;
        }
    }
    if Instant::now() >= deadline {
        return Ok(BuildOutcome::Pending);
    }
    ProjectionStore::configure_projection_cache(
        storage,
        id.clone(),
        ProjectionCacheLimits::new(64 * 1024 * 1024, 2, 32, 1_800_000)?,
    )
    .await?;
    let prior = progress::pointer(storage).await?;
    let requested = match prior {
        Some(value)
            if value.source == source
                && value.query_scope == query_scope
                && value.policy == policy_digest
                && value.expected_active == active
                && Some(value.generation) != active =>
        {
            value
        }
        prior => {
            if let Some(prior) = prior
                && Some(prior.generation) != active
            {
                let write = runtime.today_projection_lock.begin_write();
                ProjectionStore::retire_projection_generation(
                    storage,
                    ProjectionCacheScope::new(id.clone(), prior.generation, source, active)?,
                )
                .await?;
                write.complete();
            }
            BuildProgress::new(
                source,
                query_scope,
                policy_digest,
                active,
                status.raw_events(),
            )?
        }
    };
    // Acknowledge the identity before any staged row can escape. Per-batch
    // cursor/counters are written atomically with their deterministic row values.
    progress::save_pointer(runtime, &requested).await?;
    let mut build = progress::load(
        storage,
        id.clone(),
        requested.generation,
        progress::PROGRESS,
    )
    .await?
    .unwrap_or(requested);
    if build.source != source
        || build.query_scope != query_scope
        || build.policy != policy_digest
        || build.expected_active != active
    {
        return Err(TodayError::CorruptProjection);
    }
    while !build.complete && Instant::now() < deadline {
        if runtime.load_author_visibility().await?.cache_digest()? != policy_digest {
            return Err(CursorError::Stale.into());
        }
        let bounds = build
            .after
            .map_or(EventQueryBounds::first(SOURCE_ROWS)?, |cursor| {
                EventQueryBounds::first(SOURCE_ROWS)
                    .expect("positive fixed limit")
                    .after(cursor)
            });
        let page = EventStore::query_visible_at(storage, EventQuery::all(bounds), source).await?;
        let mut charge = 4096u64;
        for stored in page.items() {
            charge = charge
                .checked_add(VisibilityIndexBounds::charge(
                    stored.event().raw_json().len() as u64,
                )?)
                .ok_or(radroots_storage::Error::SpaceInsufficient)?;
        }
        if charge > WORK_BYTES {
            return Err(radroots_storage::Error::SpaceInsufficient.into());
        }
        let mut documents = Vec::new();
        for stored in page.items() {
            if !policy.allows(&stored.event().envelope().author().to_hex()) {
                continue;
            }
            if let Some(row) = rows::reduce(context, stored)? {
                build.count(&row)?;
                documents.push(row.document()?);
            }
        }
        build.after = page.next_cursor();
        build.complete = build.after.is_none();
        documents.push(ProjectionDocument::new(
            progress::PROGRESS.into(),
            encode(&build)?,
        )?);
        let batch = ProjectionDocumentBatch::new(build.scope()?, documents)?;
        let write = runtime.today_projection_lock.begin_write();
        ProjectionStore::put_projection_batch(storage, batch).await?;
        write.complete();
    }
    if !build.complete || Instant::now() >= deadline {
        return Ok(BuildOutcome::Pending);
    }
    if runtime.load_author_visibility().await?.cache_digest()? != policy_digest {
        return Err(CursorError::Stale.into());
    }
    let source_position = (build.source_events != 0)
        .then(|| {
            EventSequence::new(build.source_events)
                .map(|sequence| EventPosition::new(source.generation(), sequence))
        })
        .transpose()?;
    let checkpoint = ProjectionCheckpoint::new(
        id.clone(),
        build.generation,
        source_position,
        build.document_rows()?,
        now_unix_ms,
    )?;
    let final_batch = ProjectionDocumentBatch::new(
        build.scope()?,
        vec![ProjectionDocument::new(
            progress::HEADER.into(),
            encode(&build)?,
        )?],
    )?;
    let write = runtime.today_projection_lock.begin_write();
    ProjectionStore::promote_projection_batch(storage, final_batch, checkpoint).await?;
    write.complete();
    if EventStore::source_token(storage).await? != source {
        return Err(radroots_storage::Error::SourceEpochChanged.into());
    }
    Ok(BuildOutcome::Complete(Box::new(build)))
}

async fn retire_active(
    runtime: &TeraRuntime,
    storage: &dyn radroots_storage::Storage,
    source: radroots_storage::event::EventSourceToken,
    generation: ProjectionGeneration,
) -> Result<(), TodayError> {
    let write = runtime.today_projection_lock.begin_write();
    ProjectionStore::retire_projection_generation(
        storage,
        ProjectionCacheScope::new(
            ProjectionId::parse(progress::ID)?,
            generation,
            source,
            Some(generation),
        )?,
    )
    .await?;
    write.complete();
    Ok(())
}

#[cfg(test)]
#[path = "today_build_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "today_build_fault_tests.rs"]
mod fault_tests;

#[cfg(test)]
#[path = "today_build_reference_tests.rs"]
mod reference_tests;
