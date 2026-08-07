use radroots_mobile_core::runtime::{
    info::RuntimeInfo,
    product_surface::{AddCommandType, CardAddParity, LocalNetwork, TodayCardType},
    sdk::{
        SdkCapabilityRecord, SdkRelayStatusReportRecord, SdkShutdownRecord, SdkStorageStatusRecord,
    },
};

use crate::RadrootsAppError;

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum ProtectedDataAvailability {
    Available,
    Unavailable,
}

impl From<ProtectedDataAvailability>
    for radroots_mobile_core::runtime::store::ProtectedDataAvailability
{
    fn from(value: ProtectedDataAvailability) -> Self {
        match value {
            ProtectedDataAvailability::Available => Self::Available,
            ProtectedDataAvailability::Unavailable => Self::Unavailable,
        }
    }
}

/// Native boundary object delegating all behavior to the ordinary Rust core.
#[derive(uniffi::Object)]
pub struct RadrootsRuntime {
    inner: radroots_mobile_core::RadrootsRuntime,
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl RadrootsRuntime {
    #[cfg_attr(not(coverage_nightly), uniffi::constructor)]
    pub async fn new(
        application_support_directory: String,
        public_key_hex: String,
        source_generation_hex: String,
        source_generation_created_at_unix_ms: u64,
        protected_data: ProtectedDataAvailability,
    ) -> Result<Self, RadrootsAppError> {
        let store = radroots_mobile_core::runtime::store::MobileUserStoreConfig::from_encoded(
            application_support_directory,
            public_key_hex.as_str(),
            source_generation_hex.as_str(),
            source_generation_created_at_unix_ms,
            protected_data.into(),
        )?;
        radroots_mobile_core::runtime::builder::RuntimeBuilder::new(store)
            .build()
            .await
            .map(|inner| Self { inner })
            .map_err(Into::into)
    }

    pub async fn shutdown(&self) -> Result<SdkShutdownRecord, RadrootsAppError> {
        self.inner.shutdown().await.map_err(Into::into)
    }

    pub fn uptime_millis(&self) -> i64 {
        self.inner.uptime_millis()
    }

    pub fn info(&self) -> RuntimeInfo {
        self.inner.info()
    }

    pub fn info_json(&self) -> String {
        self.inner.info_json()
    }

    pub fn set_app_info_platform(
        &self,
        platform: Option<String>,
        bundle_id: Option<String>,
        version: Option<String>,
        build_number: Option<String>,
        build_sha: Option<String>,
    ) {
        self.inner
            .set_app_info_platform(platform, bundle_id, version, build_number, build_sha);
    }

    pub fn sdk_capabilities(&self) -> Vec<SdkCapabilityRecord> {
        self.inner.sdk_capabilities()
    }

    pub async fn sdk_storage_status(&self) -> Result<SdkStorageStatusRecord, RadrootsAppError> {
        self.inner.sdk_storage_status().await.map_err(Into::into)
    }

    pub fn sdk_relay_status(&self) -> Result<Option<SdkRelayStatusReportRecord>, RadrootsAppError> {
        self.inner.sdk_relay_status().map_err(Into::into)
    }

    pub fn configure_public_relays(
        &self,
        writable_relays: Vec<String>,
    ) -> Result<(), RadrootsAppError> {
        self.inner
            .configure_public_relays(writable_relays)
            .map_err(Into::into)
    }

    pub fn configure_simulator_relays(
        &self,
        loopback_relays: Vec<String>,
    ) -> Result<(), RadrootsAppError> {
        self.inner
            .configure_simulator_relays(loopback_relays)
            .map_err(Into::into)
    }

    pub fn configure_device_relays(
        &self,
        writable_relays: Vec<String>,
    ) -> Result<(), RadrootsAppError> {
        self.inner
            .configure_device_relays(writable_relays)
            .map_err(Into::into)
    }

    pub fn phase1_card_types(&self) -> Vec<TodayCardType> {
        self.inner.phase1_card_types()
    }

    pub fn phase1_add_command_types(&self) -> Vec<AddCommandType> {
        self.inner.phase1_add_command_types()
    }

    pub fn phase1_card_add_parity(&self) -> Vec<CardAddParity> {
        self.inner.phase1_card_add_parity()
    }

    pub fn phase1_local_network(
        &self,
        id: String,
        label: String,
        relay_urls: Vec<String>,
        locality: Option<String>,
        followed_authors: Vec<String>,
        generation: u64,
    ) -> Result<LocalNetwork, RadrootsAppError> {
        self.inner
            .phase1_local_network(
                id,
                label,
                relay_urls,
                locality,
                followed_authors,
                generation,
            )
            .map_err(Into::into)
    }
}
