//! Explicit bounded eviction of reconstructable Today cache only.
use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MediaCacheCleanup {
    pub invalidated_entries: u32,
    pub retained_candidates: u32,
    pub remaining_entries: u32,
}

impl TeraRuntime {
    /// Requires durable invalidation before physical collection. A full store
    /// can refuse this action; that never grants permission to delete authored
    /// staging, pending receipts, or files whose ownership is not proven.
    pub async fn phase1_cleanup_media_cache(
        &self,
        context: &LocalNetwork,
    ) -> Result<MediaCacheCleanup, TodayError> {
        let _command = self.lifecycle.enter()?;
        let files = self.inbound_media_lock.lock().await;
        let projection = self.today_projection_lock.lock().await;
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let generation = projection_generation()?;
        let mut state = load_state(storage, context, generation)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        let candidates = state
            .media_cache
            .cleanup_candidates(media_collection::MAX_CANDIDATES)?;
        for candidate in &candidates {
            state.media_cache.invalidate_artifact(*candidate);
            invalidate_artifact_references(&mut state, *candidate);
        }
        if !candidates.is_empty() {
            persist_media_state(self, storage, context, generation, &mut state).await?;
        }
        let collected = match self.inbound_media_directory.as_deref() {
            Some(directory) => {
                media_collection::collect_report(self, directory, &candidates, &files, &projection)
                    .await?
            }
            None => 0,
        };
        let invalidated_entries = candidates.len() as u32;
        Ok(MediaCacheCleanup {
            invalidated_entries,
            retained_candidates: invalidated_entries.saturating_sub(collected),
            remaining_entries: state.media_cache.artifact_count(),
        })
    }
}
