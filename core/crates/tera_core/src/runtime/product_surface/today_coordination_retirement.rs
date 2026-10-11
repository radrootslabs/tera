//! One acknowledged retirement intent preserves legacy refusal and restart recovery.
use super::*;
use radroots_storage::{
    event::EventSourceToken,
    projection::{
        InvalidationReason, ProjectionHealth, ProjectionInvalidation, ProjectionStatus,
        cache::ProjectionCacheScope,
    },
};
const ID: &str = "tera.today.build.v1";
const KEY: &str = "calendar.coordination.retired";
const MAX_BYTES: usize = 4096;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct Retirement {
    schema_version: u16,
    source: EventSourceToken,
    rows: ProjectionGeneration,
    now_unix_ms: u64,
    complete: bool,
}
impl Retirement {
    fn validate(&self) -> Result<(), TodayError> {
        if self.schema_version != 1
            || self.now_unix_ms == 0
            || self.rows == projection_generation()?
        {
            return Err(TodayError::CorruptProjection);
        }
        radroots_storage::event::SourceGeneration::new(*self.source.generation().as_bytes())?;
        ProjectionGeneration::new(*self.rows.as_bytes())?;
        Ok(())
    }
    fn invalidation(&self) -> Result<ProjectionInvalidation, TodayError> {
        Ok(ProjectionInvalidation::new(
            projection_id()?,
            projection_generation()?,
            self.rows,
            InvalidationReason::ProjectionGenerationChanged,
            self.now_unix_ms,
        )?)
    }
}
async fn load(storage: &dyn radroots_storage::Storage) -> Result<Option<Retirement>, TodayError> {
    ProjectionStore::projection_document(
        storage,
        ProjectionId::parse(ID)?,
        projection_generation()?,
        KEY.into(),
    )
    .await?
    .map(|document| {
        if document.value().len() > MAX_BYTES {
            return Err(TodayError::CorruptProjection);
        }
        let value: Retirement = decode(document.value())?;
        value.validate()?;
        Ok(value)
    })
    .transpose()
}
async fn save(runtime: &TeraRuntime, value: &Retirement) -> Result<(), TodayError> {
    value.validate()?;
    let bytes = encode(value)?;
    if bytes.len() > MAX_BYTES {
        return Err(TodayError::CorruptProjection);
    }
    let storage = runtime
        .client
        .storage()
        .map_err(|_| TodayError::RuntimeUnavailable)?;
    let write = runtime.today_projection_lock.begin_write();
    ProjectionStore::put_projection_document(
        storage,
        ProjectionId::parse(ID)?,
        projection_generation()?,
        ProjectionDocument::new(KEY.into(), bytes)?,
    )
    .await?;
    write.complete();
    Ok(())
}

pub(super) async fn retired(
    storage: &dyn radroots_storage::Storage,
    status: &ProjectionStatus,
) -> Result<bool, TodayError> {
    let Some(value) = load(storage).await? else {
        return Ok(false);
    };
    retired_value(storage, status, value).await
}

pub(super) async fn retired_document(
    storage: &dyn radroots_storage::Storage,
    status: &ProjectionStatus,
    document: Option<&ProjectionDocument>,
) -> Result<bool, TodayError> {
    let Some(document) = document else {
        return Ok(false);
    };
    if document.value().len() > MAX_BYTES {
        return Err(TodayError::CorruptProjection);
    }
    let value: Retirement = decode(document.value())?;
    value.validate()?;
    retired_value(storage, status, value).await
}

pub(super) fn document_identity() -> Result<(ProjectionId, ProjectionGeneration, String), TodayError>
{
    Ok((
        ProjectionId::parse(ID)?,
        projection_generation()?,
        KEY.into(),
    ))
}

async fn retired_value(
    storage: &dyn radroots_storage::Storage,
    status: &ProjectionStatus,
    value: Retirement,
) -> Result<bool, TodayError> {
    if !value.complete
        || status.generation() != projection_generation()?
        || status.health() != ProjectionHealth::Invalidated
        || status.active_rebuild().is_some()
    {
        return Ok(false);
    }
    let source = EventStore::source_token(storage).await?;
    if source.generation() != value.source.generation() {
        return Err(TodayError::CorruptProjection);
    }
    Ok(
        ProjectionStore::invalidation(storage, projection_id()?, value.rows)
            .await?
            .as_ref()
            == Some(&value.invalidation()?),
    )
}

/// Caller holds the existing Today mutex. A pending marker is repaired before
/// allocating a successor stage; it never changes canonical or legacy payloads.
pub(super) async fn resume(runtime: &TeraRuntime) -> Result<(), TodayError> {
    let storage = runtime
        .client
        .storage()
        .map_err(|_| TodayError::RuntimeUnavailable)?;
    let Some(mut value) = load(storage).await? else {
        return Ok(());
    };
    let status = ProjectionStore::status(storage, projection_id()?)
        .await?
        .ok_or(TodayError::CorruptProjection)?;
    if value.complete {
        if !retired(storage, &status).await? {
            return Err(TodayError::CorruptProjection);
        }
        return Ok(());
    }
    if status.generation() != projection_generation()? || status.active_rebuild().is_some() {
        return Err(TodayError::UnsupportedProjectionVersion);
    }
    let active = ProjectionStore::status(storage, ProjectionId::parse(build::progress::ID)?)
        .await?
        .ok_or(TodayError::CorruptProjection)?;
    let header = build::progress::load(
        storage,
        ProjectionId::parse(build::progress::ID)?,
        value.rows,
        build::progress::HEADER,
    )
    .await?
    .ok_or(TodayError::CorruptProjection)?;
    let source = EventStore::source_token(storage).await?;
    if active.generation() != value.rows
        || active.health() != ProjectionHealth::Ready
        || !header.complete
        || header.generation != value.rows
        || header.source != value.source
        || source.generation() != value.source.generation()
    {
        return Err(TodayError::CorruptProjection);
    }
    if status.health() == ProjectionHealth::Ready {
        let write = runtime.today_projection_lock.begin_write();
        ProjectionStore::invalidate(storage, value.invalidation()?).await?;
        write.complete();
    } else if status.health() != ProjectionHealth::Invalidated {
        return Err(TodayError::CorruptProjection);
    }
    if ProjectionStore::invalidation(storage, projection_id()?, value.rows)
        .await?
        .as_ref()
        != Some(&value.invalidation()?)
    {
        return Err(TodayError::CorruptProjection);
    }
    let write = runtime.today_projection_lock.begin_write();
    ProjectionStore::retire_projection_generation(
        storage,
        ProjectionCacheScope::new(
            projection_id()?,
            projection_generation()?,
            source,
            Some(projection_generation()?),
        )?,
    )
    .await?;
    write.complete();
    value.complete = true;
    save(runtime, &value).await
}

pub(super) async fn finish(
    runtime: &TeraRuntime,
    progress: &build::BuildProgress,
    now_unix_ms: u64,
) -> Result<(), TodayError> {
    let storage = runtime
        .client
        .storage()
        .map_err(|_| TodayError::RuntimeUnavailable)?;
    if load(storage).await?.is_some() {
        return resume(runtime).await;
    }
    let Some(status) = ProjectionStore::status(storage, projection_id()?).await? else {
        return Ok(());
    };
    if status.generation() != projection_generation()?
        || status.health() != ProjectionHealth::Ready
        || status.active_rebuild().is_some()
    {
        return Err(TodayError::UnsupportedProjectionVersion);
    }
    if EventStore::source_token(storage).await? != progress.source {
        return Err(CursorError::Stale.into());
    }
    let value = Retirement {
        schema_version: 1,
        source: progress.source,
        rows: progress.generation,
        now_unix_ms: now_unix_ms.max(
            status
                .checkpoint()
                .map_or(0, ProjectionCheckpoint::updated_at_unix_ms),
        ),
        complete: false,
    };
    save(runtime, &value).await?;
    resume(runtime).await
}

#[cfg(test)]
#[path = "today_coordination_retirement_tests.rs"]
mod tests;
