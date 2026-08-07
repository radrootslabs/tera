use crate::runtime::store::{MobileUserStoreConfig, ProtectedDataAvailability};
use crate::{RadrootsAppError, RadrootsRuntime};

/// Host-owned construction boundary for the shared SDK-backed runtime.
pub struct RuntimeBuilder {
    store: MobileUserStoreConfig,
    #[cfg(feature = "mobile-social")]
    signer: Option<std::sync::Arc<dyn radroots_signing::Signer>>,
    #[cfg(feature = "mobile-social")]
    relay_profile: radroots_sdk::transport::RelayProfile,
    #[cfg(feature = "mobile-social")]
    blossom_config: Option<radroots_sdk::transport::BlossomConfig>,
}

impl RuntimeBuilder {
    #[must_use]
    pub fn new(store: MobileUserStoreConfig) -> Self {
        Self {
            store,
            #[cfg(feature = "mobile-social")]
            signer: None,
            #[cfg(feature = "mobile-social")]
            relay_profile: radroots_sdk::transport::RelayProfile::public(Vec::<String>::new())
                .expect("bundled public relay profile is valid"),
            #[cfg(feature = "mobile-social")]
            blossom_config: None,
        }
    }

    /// Installs one opaque host signer without transferring secret material.
    #[cfg(feature = "mobile-social")]
    #[must_use]
    pub fn signer(mut self, signer: std::sync::Arc<dyn radroots_signing::Signer>) -> Self {
        self.signer = Some(signer);
        self
    }

    /// Replaces the bundled read-only public profile with one validated host
    /// environment profile. Construction remains inert.
    #[cfg(feature = "mobile-social")]
    #[must_use]
    pub fn relay_profile(mut self, relay_profile: radroots_sdk::transport::RelayProfile) -> Self {
        self.relay_profile = relay_profile;
        self
    }

    /// Installs one validated inert Blossom environment profile.
    #[cfg(feature = "mobile-social")]
    #[must_use]
    pub fn blossom_config(
        mut self,
        blossom_config: radroots_sdk::transport::BlossomConfig,
    ) -> Self {
        self.blossom_config = Some(blossom_config);
        self
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
        RadrootsRuntime::from_client_builder(
            builder,
            Some(self.store.public_key()),
            #[cfg(feature = "mobile-social")]
            self.signer,
            #[cfg(feature = "mobile-social")]
            Some(self.relay_profile),
            #[cfg(feature = "mobile-social")]
            self.blossom_config,
        )
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
        #[cfg(feature = "mobile-social")]
        {
            let report = runtime
                .sdk_relay_status()
                .expect("relay status")
                .expect("configured profile");
            assert_eq!(report.profile, "public");
            assert_eq!(report.state, "configured");
            assert_eq!(report.relays.len(), 1);
            assert_eq!(report.relays[0].relay_url, "wss://radroots.org");
            assert_eq!(report.relays[0].access, "read_only");
            assert_eq!(report.relays[0].read_state, "unobserved");
            assert_eq!(report.relays[0].write_state, "unsupported");
        }
        runtime.shutdown().await.expect("shutdown");
    }

    #[cfg(feature = "mobile-social")]
    #[tokio::test]
    async fn runtime_reconfiguration_preserves_profile_network_boundaries() {
        let root = tempfile::tempdir().expect("tempdir");
        let runtime = RuntimeBuilder::new(store(root.path(), ProtectedDataAvailability::Available))
            .build()
            .await
            .expect("runtime");
        assert!(
            runtime
                .configure_simulator_relays(vec!["ws://127.0.0.1:8080".to_owned()])
                .is_ok()
        );
        let report = runtime
            .sdk_relay_status()
            .expect("simulator status")
            .expect("configured profile");
        assert_eq!(report.profile, "simulator_local");
        assert_eq!(report.relays.len(), 1);
        assert_eq!(report.relays[0].access, "read_write");
        assert!(
            runtime
                .configure_public_relays(vec!["ws://127.0.0.1:8080".to_owned()])
                .is_err()
        );
        assert_eq!(
            runtime
                .sdk_relay_status()
                .expect("unchanged status")
                .expect("configured profile"),
            report
        );
        assert!(
            runtime
                .configure_simulator_blossom(vec!["http://127.0.0.1:3000".to_owned()])
                .is_ok()
        );
        assert_eq!(
            runtime.sdk_blossom_profile().expect("Blossom profile"),
            Some("simulator_local".to_owned())
        );
        assert!(
            runtime
                .configure_public_blossom(vec!["http://127.0.0.1:3000".to_owned()])
                .is_err()
        );
        assert_eq!(
            runtime.sdk_blossom_profile().expect("unchanged profile"),
            Some("simulator_local".to_owned())
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
