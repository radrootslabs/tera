use crate::runtime::store::{MobileUserStoreConfig, ProtectedDataAvailability};
use crate::{RadrootsAppError, RadrootsRuntime};

/// Host-owned construction boundary for the shared SDK-backed runtime.
pub struct RuntimeBuilder {
    store: MobileUserStoreConfig,
}

impl RuntimeBuilder {
    #[must_use]
    pub const fn new(store: MobileUserStoreConfig) -> Self {
        Self { store }
    }

    /// Opens the exact authenticated user's durable SQLite store.
    pub async fn build(self) -> Result<RadrootsRuntime, RadrootsAppError> {
        if self.store.protected_data() == ProtectedDataAvailability::Unavailable {
            return Err(RadrootsAppError::protected_data_unavailable());
        }
        self.store.validate_host_filesystem()?;
        let options = self.store.sqlite_options()?;
        let builder = radroots_sdk::ClientBuilder::sqlite(options)
            .await
            .map_err(RadrootsAppError::from_sdk)?;
        RadrootsRuntime::from_client_builder(builder, Some(self.store.public_key()))
    }
}

#[cfg(test)]
mod tests {
    use super::RuntimeBuilder;
    use crate::runtime::store::{MobileUserStoreConfig, ProtectedDataAvailability};

    const PUBLIC_KEY: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
    const GENERATION: &str = "0202020202020202020202020202020202020202020202020202020202020202";

    fn store(
        root: &std::path::Path,
        protected_data: ProtectedDataAvailability,
    ) -> MobileUserStoreConfig {
        let store = MobileUserStoreConfig::from_encoded(
            root,
            PUBLIC_KEY,
            GENERATION,
            1_800_000_000_000,
            protected_data,
        )
        .expect("store config");
        std::fs::create_dir_all(store.owner_directory()).expect("owner directory");
        store
    }

    #[tokio::test]
    async fn builder_constructs_a_durable_sdk_backed_runtime() {
        let root = tempfile::tempdir().expect("tempdir");
        let runtime = RuntimeBuilder::new(store(root.path(), ProtectedDataAvailability::Available))
            .build()
            .await
            .expect("runtime");
        assert!(!runtime.info().sdk_closed);
        assert_eq!(
            runtime.sdk_storage_status().await.expect("status").backend,
            "sqlite"
        );
        runtime.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn protected_data_unavailability_is_retryable_and_reopen_recovers() {
        let root = tempfile::tempdir().expect("tempdir");
        let unavailable =
            RuntimeBuilder::new(store(root.path(), ProtectedDataAvailability::Unavailable))
                .build()
                .await;
        let Err(unavailable) = unavailable else {
            panic!("protected data unavailability must fail");
        };
        let report = unavailable.store_report().expect("store report");
        assert_eq!(report.code, "protected_data_unavailable");
        assert!(report.retryable);

        let runtime = RuntimeBuilder::new(store(root.path(), ProtectedDataAvailability::Available))
            .build()
            .await
            .expect("recovered runtime");
        runtime.shutdown().await.expect("shutdown");
    }
}
