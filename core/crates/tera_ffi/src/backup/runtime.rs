use super::{ProtectedDataAvailability, RuntimeOpenOptions, TeraRuntime, build_runtime};
use crate::backup::BackupHostAdapter;
use crate::{FfiBackupManifest, FfiBackupRequest, TeraAppError, TeraBackupHost, TeraHostSigner};

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl TeraRuntime {
    /// Requires the native file owner to prepare the exact account backup root.
    #[cfg_attr(not(coverage_nightly), uniffi::constructor)]
    pub async fn with_host_signer_and_local_backups(
        application_support_directory: String,
        public_key_hex: String,
        source_generation_hex: String,
        source_generation_created_at_unix_ms: u64,
        protected_data: ProtectedDataAvailability,
        host_signer: Box<dyn TeraHostSigner>,
    ) -> Result<Self, TeraAppError> {
        build_runtime(
            application_support_directory,
            public_key_hex,
            source_generation_hex,
            source_generation_created_at_unix_ms,
            protected_data,
            Some(host_signer),
            RuntimeOpenOptions {
                local_backups: true,
                ..Default::default()
            },
        )
        .await
    }

    pub async fn capture_application_backup(
        &self,
        request: FfiBackupRequest,
        host: Box<dyn TeraBackupHost>,
    ) -> Result<FfiBackupManifest, TeraAppError> {
        let request = request.decode()?;
        self.inner
            .capture_application_backup(request, &BackupHostAdapter(host))
            .await?
            .try_into()
            .map_err(Into::into)
    }
}
