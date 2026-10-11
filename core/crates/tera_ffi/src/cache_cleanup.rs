use crate::{FfiLocalNetworkRecord, FfiRuntimeChangeKind, TeraAppError, TeraRuntime};

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiMediaCacheCleanup {
    pub invalidated_entries: u32,
    pub retained_candidates: u32,
    pub remaining_entries: u32,
    pub recovered_orphan_files: u32,
    pub scan_pending: bool,
}

#[uniffi::export(async_runtime = "tokio")]
impl TeraRuntime {
    pub async fn phase1_cleanup_media_cache(
        &self,
        context: FfiLocalNetworkRecord,
    ) -> Result<FfiMediaCacheCleanup, TeraAppError> {
        let context = self.local_network(context)?;
        let result = self.inner.phase1_cleanup_media_cache(&context).await?;
        self.subscriptions
            .notify_context(FfiRuntimeChangeKind::Media, Some(&context), None);
        Ok(FfiMediaCacheCleanup {
            invalidated_entries: result.invalidated_entries,
            retained_candidates: result.retained_candidates,
            remaining_entries: result.remaining_entries,
            recovered_orphan_files: result.recovered_orphan_files,
            scan_pending: result.scan_pending,
        })
    }
}
