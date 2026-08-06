pub mod app_info;
pub mod builder;
pub mod info;
#[cfg(feature = "mobile-social")]
pub mod key_management;
#[cfg(feature = "mobile-social")]
pub mod nostr;
pub mod product_surface;
pub mod sdk;

use chrono::Utc;
use radroots_sdk::{Client, ClientBuilder};
use std::sync::{
    RwLock,
    atomic::{AtomicBool, Ordering},
};

use self::{
    app_info::AppInfoPlatform,
    info::{RuntimeInfo, gather_runtime_info},
};
use crate::RadrootsAppError;

#[derive(uniffi::Object)]
pub struct RadrootsRuntime {
    pub(crate) client: Client,
    #[cfg(feature = "mobile-social")]
    pub(crate) signing_slot: radroots_sdk::signing::Slot,
    #[cfg(feature = "mobile-social")]
    pub(crate) nostr_slot: radroots_sdk::transport::NostrSlot,
    #[cfg(feature = "mobile-social")]
    pub(crate) identity_label: RwLock<Option<String>>,
    pub(crate) started_unix_ms: i64,
    pub(crate) shutting_down: AtomicBool,
    pub(crate) platform_app: RwLock<Option<AppInfoPlatform>>,
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl RadrootsRuntime {
    #[cfg_attr(not(coverage_nightly), uniffi::constructor)]
    pub fn new() -> Result<Self, RadrootsAppError> {
        #[cfg(feature = "mobile-social")]
        let signing_slot = radroots_sdk::signing::Slot::new();
        #[cfg(feature = "mobile-social")]
        let nostr_slot = radroots_sdk::transport::NostrSlot::new(
            radroots_sdk::transport::RelayUrlPolicy::Public,
        );
        let builder = ClientBuilder::memory_default();
        #[cfg(feature = "mobile-social")]
        let builder = builder
            .signing(radroots_sdk::signing::Provider::slot(signing_slot.clone()))
            .nostr(nostr_slot.clone())
            .host_sync(radroots_sdk::sync::HostPolicy::standard());
        let client = builder.build().map_err(RadrootsAppError::from_sdk)?;

        Ok(Self {
            client,
            #[cfg(feature = "mobile-social")]
            signing_slot,
            #[cfg(feature = "mobile-social")]
            nostr_slot,
            #[cfg(feature = "mobile-social")]
            identity_label: RwLock::new(None),
            started_unix_ms: Utc::now().timestamp_millis(),
            shutting_down: AtomicBool::new(false),
            platform_app: RwLock::new(None),
        })
    }

    /// Closes SDK resources asynchronously across every runtime reference.
    ///
    /// Dropping the returned future before its first poll has no effect. If a
    /// host cancels after close begins, it must call `shutdown` again; the SDK
    /// remains unavailable and resumes the explicit close attempt. Completed
    /// calls are idempotent and no blocking destructor is installed.
    pub async fn shutdown(&self) -> Result<sdk::SdkShutdownRecord, RadrootsAppError> {
        let already_closed = self.client.is_closed();
        self.shutting_down.store(true, Ordering::Release);
        self.client
            .close()
            .await
            .map_err(RadrootsAppError::from_sdk)?;
        Ok(sdk::SdkShutdownRecord {
            state: "closed".to_owned(),
            already_closed,
        })
    }

    pub fn uptime_millis(&self) -> i64 {
        Utc::now().timestamp_millis() - self.started_unix_ms
    }

    pub fn info(&self) -> RuntimeInfo {
        gather_runtime_info(self)
    }

    pub fn info_json(&self) -> String {
        serde_json::to_string_pretty(&self.info())
            .unwrap_or_else(|error| format!(r#"{{"error":"serialize RuntimeInfo: {error}"}}"#))
    }

    pub fn set_app_info_platform(
        &self,
        platform: Option<String>,
        bundle_id: Option<String>,
        version: Option<String>,
        build_number: Option<String>,
        build_sha: Option<String>,
    ) {
        let platform_info =
            AppInfoPlatform::new(platform, bundle_id, version, build_number, build_sha);
        if let Ok(mut guard) = self.platform_app.write() {
            *guard = Some(platform_info);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::RadrootsRuntime;
    use radroots_sdk::capability::{Availability, CapabilityId};
    use std::panic::{AssertUnwindSafe, catch_unwind};

    fn poison_platform_lock(runtime: &RadrootsRuntime) {
        let _ = catch_unwind(AssertUnwindSafe(|| {
            let _guard = runtime.platform_app.write().expect("lock platform");
            panic!("poison platform lock");
        }));
    }

    #[test]
    fn runtime_owns_one_sdk_client() {
        let runtime = RadrootsRuntime::new().expect("runtime");
        let storage = runtime
            .client
            .capabilities()
            .get(CapabilityId::CANONICAL_STORAGE)
            .expect("storage capability");
        assert_eq!(storage.availability(), Availability::Available);
        assert!(!runtime.client.is_closed());
    }

    #[test]
    fn set_platform_info_handles_poisoned_lock() {
        let runtime = RadrootsRuntime::new().expect("runtime");
        runtime.set_app_info_platform(
            Some("ios".to_owned()),
            Some("org.radroots.app".to_owned()),
            Some("1.0.0".to_owned()),
            Some("100".to_owned()),
            Some("abc123".to_owned()),
        );
        assert_eq!(
            runtime
                .info()
                .app
                .platform
                .as_ref()
                .and_then(|value| value.platform.clone()),
            Some("ios".to_owned())
        );
        poison_platform_lock(&runtime);
        runtime.set_app_info_platform(None, None, None, None, None);
    }

    #[test]
    fn runtime_metadata_helpers_are_host_safe() {
        let runtime = RadrootsRuntime::new().expect("runtime");
        assert!(runtime.uptime_millis() >= 0);
        let json = runtime.info_json();
        assert!(json.contains("sdk"));
    }
}
