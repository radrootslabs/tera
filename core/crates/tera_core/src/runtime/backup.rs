//! Local account backup through the one canonical database and native file
//! owners. No SQL, live database copy, exporter, background task or secret port.

use radroots_storage::EventStore;
use radroots_transport::BoxFuture;

use crate::TeraRuntime;

mod error;
mod model;
pub use error::BackupError;
pub use model::{
    APPLICATION_BACKUP_VERSION, ApplicationBackupManifest, BACKUP_MANIFEST_MAX_BYTES,
    BACKUP_MEDIA_MAX_BYTES, BackupMediaLease, BackupMediaRequirement, BackupRequest,
};

/// The native host owns file admission, immutable verified leases and durable
/// create-only publication. All paths are derived by that owner from the exact
/// account/request binding. Errors must not contain filesystem or user data.
pub trait BackupHost: Send + Sync {
    /// Only definitive absence returns None; malformed or protected data fails.
    fn load_candidate(
        &self,
        request: BackupRequest,
    ) -> BoxFuture<'_, Result<Option<Vec<u8>>, BackupError>>;
    fn retain_media(
        &self,
        request: BackupRequest,
        media: Vec<BackupMediaRequirement>,
    ) -> BoxFuture<'_, Result<Vec<BackupMediaLease>, BackupError>>;
    fn persist_candidate(
        &self,
        manifest: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), BackupError>>;
    /// Reverify the exact immutable leases and durably publish this manifest.
    fn publish_complete(
        &self,
        manifest: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), BackupError>>;
}

impl TeraRuntime {
    /// Captures an idle runtime or reconciles this exact prior candidate. Busy
    /// admission never cancels existing work. Cancellation retains partial data
    /// and releases the fence; no complete receipt is manufactured.
    pub async fn capture_application_backup(
        &self,
        request: BackupRequest,
        host: &dyn BackupHost,
    ) -> Result<ApplicationBackupManifest, BackupError> {
        request.validate()?;
        if self.store_public_key.map(|v| v.into_bytes()) != Some(request.author()) {
            return Err(BackupError::IdentityMismatch);
        }
        let _maintenance = self.lifecycle.maintenance()?;
        let operations = self
            .client
            .storage_operations()
            .map_err(|_| BackupError::Unavailable)?;
        // A canceled command can leave work on the owner's executor after its
        // application guard drops. Drain that work while excluding new writes.
        operations.settle_backup_writes().await?;
        let store = self
            .client
            .storage()
            .map_err(|_| BackupError::Unavailable)?;
        let generation = EventStore::status(store)
            .await
            .map_err(|_| BackupError::Unavailable)?
            .generation();
        if generation.as_bytes() != &request.generation() {
            return Err(BackupError::GenerationMismatch);
        }
        let plan = request.plan()?;
        let manifest = if let Some(bytes) = host.load_candidate(request.clone()).await? {
            let manifest = ApplicationBackupManifest::decode(&bytes, &request)?;
            let requirements = manifest
                .media()
                .iter()
                .map(|v| BackupMediaRequirement {
                    sha256: v.sha256.clone(),
                    byte_length: v.byte_length,
                })
                .collect();
            let leases = host.retain_media(request.clone(), requirements).await?;
            if leases != manifest.media() {
                return Err(BackupError::MediaUnavailable);
            }
            manifest
        } else {
            let references =
                super::product_surface::media_gc::backup_references(store, request.author())
                    .await
                    .map_err(|_| BackupError::MediaUnavailable)?;
            if references
                .iter()
                .any(|reference| reference.byte_length > BACKUP_MEDIA_MAX_BYTES)
            {
                return Err(BackupError::CapacityExceeded);
            }
            let requirements = references
                .iter()
                .map(|reference| BackupMediaRequirement {
                    sha256: reference.sha256.clone(),
                    byte_length: reference.byte_length,
                })
                .collect();
            let leases = host.retain_media(request.clone(), requirements).await?;
            model::validate_leases(&request, &leases)?;
            if !leases
                .iter()
                .map(|v| (&v.sha256, v.byte_length))
                .eq(references.iter().map(|v| (&v.sha256, v.byte_length)))
            {
                return Err(BackupError::MediaUnavailable);
            }
            let owner = operations.capture_backup(plan.clone()).await?;
            let manifest = ApplicationBackupManifest::new(request.clone(), owner, leases)?;
            operations
                .verify_backup(plan.clone(), manifest.owner().clone())
                .await?;
            // Bound canonical encoding before crossing the native port.
            manifest.encode()?;
            host.persist_candidate(manifest.clone()).await?;
            manifest
        };
        // The owner re-verifies either staged or already finalized members.
        // Thus an interrupted prior finalization does not force a new snapshot.
        operations
            .finalize_backup(plan, manifest.owner().clone())
            .await?;
        host.publish_complete(manifest.clone()).await?;
        Ok(manifest)
    }
}

#[cfg(test)]
mod fault_tests;
#[cfg(test)]
mod tests;
