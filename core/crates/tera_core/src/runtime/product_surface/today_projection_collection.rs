//! One foreground pass; remaining provider rows/indexes are its durable resume state.
use super::*;
use radroots_storage::{
    event::EventSourceToken,
    projection::cache::{ProjectionCacheCollection, ProjectionCacheLimits, ProjectionCacheScope},
};
use std::time::Instant;

pub(super) fn limits() -> Result<ProjectionCacheLimits, radroots_storage::Error> {
    ProjectionCacheLimits::new(64 * 1024 * 1024, 2, 32, 1_800_000)
}

fn eligible(runtime: &TeraRuntime, deadline: Instant) -> bool {
    Instant::now() < deadline && runtime.today_projection_lock.can_collect()
}

fn acknowledge(
    write: crate::runtime::today_projection::WritePermit<'_>,
    result: Result<ProjectionCacheCollection, radroots_storage::Error>,
) -> Result<ProjectionCacheCollection, TodayError> {
    // Both owned providers return this source fence refusal before any effects.
    // It remains a caller-visible refusal, but is a settled non-write. A dropped
    // future or any uncertain backend/capacity failure still keeps the fence.
    if result.is_ok() || matches!(&result, Err(radroots_storage::Error::SourceEpochChanged)) {
        write.complete();
    }
    result.map_err(Into::into)
}

/// Caller holds Today mutex and has resumed any pending retirement intent.
/// The deadline is cooperative: dispatched storage work must settle honestly.
pub(super) async fn run(
    runtime: &TeraRuntime,
    storage: &dyn radroots_storage::Storage,
    source: EventSourceToken,
    active: Option<ProjectionGeneration>,
    deadline: Instant,
) -> Result<Option<ProjectionCacheCollection>, TodayError> {
    if !eligible(runtime, deadline) {
        return Ok(None);
    }
    let legacy = projection_id()?;
    if let Some(status) = ProjectionStore::status(storage, legacy.clone()).await?
        && coordination_retirement::retired(storage, &status).await?
        && eligible(runtime, deadline)
    {
        // Legacy documents still hold custody for contexts not yet converted.
        // Configure only after acknowledged retirement, never during its intent.
        let write = runtime.today_projection_lock.begin_write();
        ProjectionStore::configure_projection_cache(storage, legacy.clone(), limits()?).await?;
        write.complete();
        for generation in [
            calendar_migration::legacy_generation()?,
            projection_generation()?,
        ] {
            if !eligible(runtime, deadline) {
                return Ok(None);
            }
            let write = runtime.today_projection_lock.begin_write();
            let result = ProjectionStore::collect_obsolete_projection_snapshots(
                storage,
                ProjectionCacheScope::new(
                    legacy.clone(),
                    generation,
                    source,
                    Some(status.generation()),
                )?,
            )
            .await;
            let result = acknowledge(write, result)?;
            if result.removed_rows != 0 {
                return Ok(Some(result));
            }
        }
    }
    if !eligible(runtime, deadline) {
        return Ok(None);
    }
    let write = runtime.today_projection_lock.begin_write();
    let result = ProjectionStore::collect_retired_projection_cache(
        storage,
        ProjectionId::parse(build::progress::ID)?,
        source,
        active,
    )
    .await;
    let result = acknowledge(write, result)?;
    Ok(Some(result))
}

#[cfg(test)]
#[path = "today_projection_collection_tests.rs"]
mod tests;
