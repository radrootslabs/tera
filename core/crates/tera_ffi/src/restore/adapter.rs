use super::*;
use radroots_storage::outbox::BoxFuture;
use tera_core::runtime::{backup::ApplicationBackupManifest, restore::RestoreHost};

pub(crate) struct RestoreHostAdapter(pub(crate) Box<dyn TeraRestoreHost>);

impl RestoreHost for RestoreHostAdapter {
    fn require_quiescent(&self) -> BoxFuture<'_, Result<(), RestoreError>> {
        Box::pin(async { self.0.require_quiescent().await.map_err(host_error) })
    }
    fn load_completed(
        &self,
        request: RestoreRequest,
    ) -> BoxFuture<'_, Result<Vec<u8>, RestoreError>> {
        Box::pin(async move {
            self.0
                .load_completed((&request).into())
                .await
                .map_err(host_error)
        })
    }
    fn restore_media(
        &self,
        manifest: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), RestoreError>> {
        Box::pin(async move {
            self.0
                .restore_media(manifest.try_into()?)
                .await
                .map_err(host_error)
        })
    }
    fn arm_guard(&self, guard: ApplicationRestoreGuard) -> BoxFuture<'_, Result<(), RestoreError>> {
        Box::pin(async move {
            self.0
                .arm_guard(guard.try_into()?)
                .await
                .map_err(host_error)
        })
    }
}
