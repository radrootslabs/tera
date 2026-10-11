//! Physical cache deletion requires absence across every context, not merely
//! invalidation in the requesting context. Incomplete proof retains bytes.
use std::{collections::BTreeSet, path::Path};

use radroots_storage::projection::document_query::{
    ProjectionDocumentGenerations, ProjectionDocumentQuery,
};
use tokio::sync::MutexGuard;

use super::*;

pub(super) const MAX_CANDIDATES: usize = 64;
const PAGE_ROWS: u16 = 32;
const MAX_PAGES: usize = 64;
const MAX_DOCUMENT_BYTES: usize = 64 * 1024 * 1024;

/// Both guards must belong to this runtime, acquired file-first, and remain
/// held through unlink. A live inventory is only complete under that fence.
/// Excess candidates and incomplete/unknown ownership are retained for a later
/// reconciliation; a quota never authorizes deletion of ambiguous ownership.
pub(super) async fn collect(
    runtime: &TeraRuntime,
    directory: &Path,
    candidates: &[Phase1MediaArtifactId],
    _files: &MutexGuard<'_, ()>,
    _projection: &MutexGuard<'_, ()>,
) -> Result<(), TodayError> {
    collect_report(runtime, directory, candidates, _files, _projection)
        .await
        .map(|_| ())
}

pub(super) async fn collect_report(
    runtime: &TeraRuntime,
    directory: &Path,
    candidates: &[Phase1MediaArtifactId],
    _files: &MutexGuard<'_, ()>,
    _projection: &MutexGuard<'_, ()>,
) -> Result<u32, TodayError> {
    if !runtime.today_projection_lock.can_collect() {
        return Ok(0);
    }
    let storage = runtime
        .client
        .storage()
        .map_err(|_| TodayError::RuntimeUnavailable)?;
    let Some(unreferenced) = unreferenced(storage, candidates).await? else {
        return Ok(0);
    };
    if !runtime.today_projection_lock.can_collect() {
        return Ok(0);
    }
    let collected = unreferenced.len() as u32;
    for artifact in unreferenced {
        super::super::media::remove_artifact_files(directory, artifact)?;
    }
    Ok(collected)
}

async fn unreferenced(
    storage: &dyn radroots_storage::Storage,
    candidates: &[Phase1MediaArtifactId],
) -> Result<Option<BTreeSet<Phase1MediaArtifactId>>, TodayError> {
    let mut remaining: BTreeSet<_> = candidates.iter().take(MAX_CANDIDATES).copied().collect();
    if remaining.is_empty() {
        return Ok(Some(remaining));
    }
    let mut bytes = 0_usize;
    let mut pages = 0_usize;
    for (id, local) in [
        (projection_id()?, false),
        (ProjectionId::parse(sidecar::ID)?, true),
    ] {
        let mut query =
            ProjectionDocumentQuery::new(id, ProjectionDocumentGenerations::All, PAGE_ROWS)?;
        loop {
            pages += 1;
            if pages > MAX_PAGES {
                return Ok(None);
            }
            let page =
                match ProjectionStore::query_projection_documents(storage, query.clone()).await {
                    Ok(page) => page,
                    Err(_) => return Ok(None),
                };
            for record in page.records() {
                let Some(document) = record.document() else {
                    return Ok(None);
                };
                bytes = bytes.saturating_add(document.value().len());
                if bytes > MAX_DOCUMENT_BYTES || record.generation() != projection_generation()? {
                    return Ok(None);
                }
                let cache = if local {
                    let Ok(state) = serde_json::from_slice::<sidecar::LocalState>(document.value())
                    else {
                        return Ok(None);
                    };
                    if state.validate(record.key()).is_err()
                        || encode(&state)?.as_slice() != document.value()
                    {
                        return Ok(None);
                    }
                    state.cache
                } else {
                    let Some(state) = current_ownership(document.value(), record.key()) else {
                        return Ok(None);
                    };
                    state.media_cache
                };
                let mut cache = cache;
                // Hidden entries retain ownership independently of visible rows.
                remaining.retain(|artifact| !cache.invalidate_artifact(*artifact));
            }
            let Some(cursor) = page.next_cursor() else {
                break;
            };
            query = query.with_cursor(cursor)?;
        }
    }
    Ok(Some(remaining))
}

fn current_ownership(bytes: &[u8], key: &str) -> Option<TodayProjectionState> {
    // Do not migrate or discard unknown fields while proving absence. Exact
    // canonical encoding also rejects duplicate keys and unrecognized nested
    // fields, which a permissive application decoder could otherwise ignore.
    let state: TodayProjectionState = serde_json::from_slice(bytes).ok()?;
    if state.schema_version != TODAY_PROJECTION_DOCUMENT_SCHEMA_VERSION
        || state.content_generation == 0
        || content_generation(&state).ok()? != state.content_generation
        || validate_media_state(&state).is_err()
        || encode(&state).ok()?.as_slice() != bytes
    {
        return None;
    }
    let mut digest = Sha256::new();
    digest.update(PROJECTION_DOCUMENT_KEY_DOMAIN);
    digest.update(state.context_id.as_bytes());
    digest.update(state.context_generation.to_be_bytes());
    (key == format!("context.{}", hex::encode(digest.finalize()))).then_some(state)
}

#[cfg(test)]
#[path = "today_media_collection_tests.rs"]
mod tests;
