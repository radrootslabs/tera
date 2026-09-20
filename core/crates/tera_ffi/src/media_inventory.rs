use crate::TeraAppError;
use std::{path::Path, sync::Arc};
use tera_core::runtime::product_surface::media_gc;

#[derive(uniffi::Object)]
pub struct FfiMediaReferenceInventory {
    inventory: media_gc::MediaReferenceInventory,
}

/// Host contract: retain the exclusive root fence from before inventory until
/// after the final unlink. The object is ephemeral and is never persisted.
#[uniffi::export(async_runtime = "tokio")]
pub async fn inspect_media_references(
    application_support_directory: String,
    native_hashes: Vec<String>,
) -> Result<Arc<FfiMediaReferenceInventory>, TeraAppError> {
    let inventory = media_gc::inspect_media_references(
        Path::new(&application_support_directory),
        native_hashes,
    )
    .await
    .map_err(|_| TeraAppError::invalid_argument("media_inventory_incomplete"))?;
    Ok(Arc::new(FfiMediaReferenceInventory { inventory }))
}

#[uniffi::export]
impl FfiMediaReferenceInventory {
    pub fn permits_orphan(&self, name: String, modified_ms: u64, now_ms: u64) -> bool {
        self.inventory.permits_orphan(&name, modified_ms, now_ms)
    }
}

#[derive(uniffi::Record)]
pub struct FfiMediaCleanupLimits {
    pub directory_entries: u64,
    pub removals: u64,
    pub native_references: u64,
}

#[uniffi::export]
pub fn media_cleanup_limits() -> FfiMediaCleanupLimits {
    FfiMediaCleanupLimits {
        directory_entries: media_gc::MEDIA_DIRECTORY_ENTRY_BUDGET as u64,
        removals: media_gc::MEDIA_REMOVAL_BUDGET as u64,
        native_references: media_gc::MEDIA_REFERENCE_BUDGET as u64,
    }
}
