//! Cold recovery has no signer, transport or live application runtime.

use radroots_sdk::{Client, ClientBuilder};
use radroots_storage::{
    EventStore,
    backup::{BackupSecretPolicy, RestorePlan},
};
use radroots_transport::BoxFuture;

use super::{ApplicationRestoreGuard, RestoreError as E, RestoreRequest, barrier};
use crate::runtime::{
    backup::ApplicationBackupManifest,
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};

/// Implemented by the existing native file owner while holding exclusive
/// maintenance for the entire call. No live process users or OS transfer tasks
/// may be admitted until it returns. Unknown task state must fail closed.
pub trait RestoreHost: Send + Sync {
    fn require_quiescent(&self) -> BoxFuture<'_, Result<(), E>>;
    fn load_completed(&self, request: RestoreRequest) -> BoxFuture<'_, Result<Vec<u8>, E>>;
    /// Verify immutable backup leases and restore exact required bytes through
    /// the native file owner. Never replace a conflicting immutable media file.
    fn restore_media(&self, manifest: ApplicationBackupManifest) -> BoxFuture<'_, Result<(), E>>;
    /// Durable create-only write outside both databases. An ambiguous result
    /// remains recovery evidence and cannot be treated as guard absence.
    fn arm_guard(&self, guard: ApplicationRestoreGuard) -> BoxFuture<'_, Result<(), E>>;
}

/// Starts one explicit attempt against the account's configured owner backup
/// root. This API cannot accept an arbitrary database or bundle pathname.
/// A retained guard from an interrupted attempt requires explicit recovery;
/// never delete staging or silently start a new attempt over it.
pub async fn restore_application_backup(
    config: MobileUserStoreConfig,
    request: RestoreRequest,
    host: &dyn RestoreHost,
) -> Result<ApplicationRestoreGuard, E> {
    request.validate()?;
    if config.public_key().into_bytes() != request.backup().author() {
        return Err(E::IdentityMismatch);
    }
    if config.source_generation().as_bytes() != &request.backup().generation() {
        return Err(E::GenerationMismatch);
    }
    if config.protected_data() != ProtectedDataAvailability::Available {
        return Err(E::Unavailable);
    }
    let config = config.with_local_backups();
    config
        .validate_host_filesystem()
        .map_err(|_| E::Unavailable)?;
    if config
        .restore_guard_exists()
        .map_err(|_| E::RecoveryRequired)?
    {
        return Err(E::RecoveryRequired);
    }
    host.require_quiescent().await?;
    let manifest = ApplicationBackupManifest::decode(
        &host.load_completed(request.clone()).await?,
        request.backup(),
    )?;
    let guard = ApplicationRestoreGuard::new(request, &manifest)?;
    host.restore_media(manifest.clone()).await?;
    let client = open(&config).await?;
    let installed = install(&client, &guard, &manifest, host).await;
    // Explicit close is required even when stage/finalize fails. Cancellation
    // retains the durable guard and canonical owner's interruption evidence.
    let closed = client.close().await.map_err(|_| E::RecoveryRequired);
    closed?;
    installed?;
    let reopened = open(&config).await?;
    let setup = setup(&reopened, &guard, &manifest).await;
    let closed = reopened.close().await.map_err(|_| E::RecoveryRequired);
    closed?;
    setup?;
    Ok(guard)
}

async fn open(config: &MobileUserStoreConfig) -> Result<Client, E> {
    ClientBuilder::sqlite(config.sqlite_options().map_err(|_| E::RecoveryRequired)?)
        .await
        .map_err(|_| E::RecoveryRequired)?
        .build()
        .map_err(|_| E::Unavailable)
}

async fn install(
    client: &Client,
    guard: &ApplicationRestoreGuard,
    manifest: &ApplicationBackupManifest,
    host: &dyn RestoreHost,
) -> Result<(), E> {
    let store = client.storage().map_err(|_| E::Unavailable)?;
    let generation = EventStore::status(store)
        .await
        .map_err(|_| E::Unavailable)?
        .generation();
    if generation.as_bytes() != &guard.request().backup().generation() {
        return Err(E::GenerationMismatch);
    }
    let operations = client.storage_operations().map_err(|_| E::Unavailable)?;
    let plan = RestorePlan::new(
        manifest.owner().clone(),
        BackupSecretPolicy::IncludeProtectedStorage,
        guard.request().requested_at_ms(),
    )
    .map_err(|_| E::InvalidRequest)?;
    host.arm_guard(guard.clone()).await?;
    operations.stage_restore(plan.clone()).await?;
    host.require_quiescent().await?;
    operations.finalize_restore(plan).await?;
    Ok(())
}

async fn setup(
    client: &Client,
    guard: &ApplicationRestoreGuard,
    manifest: &ApplicationBackupManifest,
) -> Result<(), E> {
    let store = client.storage().map_err(|_| E::Unavailable)?;
    let generation = EventStore::status(store)
        .await
        .map_err(|_| E::Unavailable)?
        .generation();
    if generation.as_bytes() != &guard.request().backup().generation() {
        return Err(E::GenerationMismatch);
    }
    let references = crate::runtime::product_surface::media_gc::backup_references(
        store,
        guard.request().backup().author(),
    )
    .await
    .map_err(|_| E::VerificationFailed)?;
    if !references
        .iter()
        .map(|item| (&item.sha256, item.byte_length))
        .eq(manifest
            .media()
            .iter()
            .map(|item| (&item.sha256, item.byte_length)))
    {
        return Err(E::MediaUnavailable);
    }
    barrier::install(store, guard).await
}
