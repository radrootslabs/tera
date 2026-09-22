use super::*;
use crate::restore::{self, *};
use tera_core::runtime::{
    restore::{ApplicationRestoreGuard, restore_application_backup},
    store::MobileUserStoreConfig,
};

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRestoreStore {
    pub application_support_directory: String,
    pub public_key: String,
    pub source_generation: String,
    pub source_generation_created_at_ms: u64,
    pub protected_data: ProtectedDataAvailability,
}

#[uniffi::export(async_runtime = "tokio")]
pub async fn restore_local_application_backup(
    store: FfiRestoreStore,
    request: FfiRestoreRequest,
    host: Box<dyn TeraRestoreHost>,
) -> Result<FfiRestoreGuard, TeraAppError> {
    let config = MobileUserStoreConfig::from_encoded(
        store.application_support_directory,
        &store.public_key,
        &store.source_generation,
        store.source_generation_created_at_ms,
        store.protected_data.into(),
    )?;
    restore_application_backup(config, request.decode()?, &RestoreHostAdapter(host))
        .await?
        .try_into()
        .map_err(Into::into)
}

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl TeraRuntime {
    #[cfg_attr(not(coverage_nightly), uniffi::constructor)]
    pub async fn with_host_signer_and_restore_guard(
        store: FfiRestoreStore,
        host_signer: Box<dyn TeraHostSigner>,
        guard: Vec<u8>,
        local_backups: bool,
    ) -> Result<Self, TeraAppError> {
        build_runtime(
            store.application_support_directory,
            store.public_key,
            store.source_generation,
            store.source_generation_created_at_ms,
            store.protected_data,
            Some(host_signer),
            RuntimeOpenOptions {
                local_backups,
                restore_guard: Some(ApplicationRestoreGuard::decode(&guard)?),
            },
        )
        .await
    }

    pub async fn application_restore_status(
        &self,
    ) -> Result<Option<FfiRestoreStatus>, TeraAppError> {
        self.inner
            .restore_status()
            .await
            .map(|value| value.map(Into::into))
            .map_err(Into::into)
    }

    pub async fn reconcile_restored_target(
        &self,
        draft_id: String,
        target_fingerprint: String,
    ) -> Result<FfiRestoreTarget, TeraAppError> {
        self.inner
            .reconcile_restored_target(restore::bytes(&draft_id)?, &target_fingerprint)
            .await
            .map(Into::into)
            .map_err(Into::into)
    }

    pub async fn review_restored_work(&self) -> Result<String, TeraAppError> {
        self.inner
            .review_restored_work()
            .await
            .map(hex::encode)
            .map_err(Into::into)
    }

    pub async fn resume_restored_work(
        &self,
        reviewed_inventory: String,
    ) -> Result<(), TeraAppError> {
        self.inner
            .resume_restored_work(restore::bytes(&reviewed_inventory)?)
            .await
            .map_err(Into::into)
    }
}
