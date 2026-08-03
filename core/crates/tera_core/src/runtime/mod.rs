pub mod app_info;
pub mod builder;
pub mod info;
pub mod product_surface;
pub mod sdk;

use chrono::Utc;
use radroots_sdk::{Client, ClientBuilder};
use std::sync::{
    Mutex, RwLock,
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
    #[cfg(feature = "rt")]
    executor: Mutex<Option<tokio::runtime::Runtime>>,
    pub(crate) started_unix_ms: i64,
    pub(crate) shutting_down: AtomicBool,
    pub(crate) platform_app: RwLock<Option<AppInfoPlatform>>,
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
impl RadrootsRuntime {
    #[cfg_attr(not(coverage_nightly), uniffi::constructor)]
    pub fn new() -> Result<Self, RadrootsAppError> {
        let client = ClientBuilder::memory_default()
            .build()
            .map_err(RadrootsAppError::from_sdk)?;
        #[cfg(feature = "rt")]
        let executor = tokio::runtime::Builder::new_multi_thread()
            .thread_name("radroots-app-sdk")
            .enable_all()
            .build()
            .map_err(|error| RadrootsAppError::initialization(error.to_string()))?;

        Ok(Self {
            client,
            #[cfg(feature = "rt")]
            executor: Mutex::new(Some(executor)),
            started_unix_ms: Utc::now().timestamp_millis(),
            shutting_down: AtomicBool::new(false),
            platform_app: RwLock::new(None),
        })
    }

    pub fn stop(&self) {
        if self.shutting_down.swap(true, Ordering::SeqCst) {
            let _ = crate::logging::log_info(
                "Runtime stop already in progress or completed.".to_owned(),
            );
            return;
        }

        #[cfg(feature = "rt")]
        match self.executor.lock() {
            Ok(mut executor) => {
                if let Some(executor) = executor.take() {
                    if let Err(error) = executor.block_on(self.client.close()) {
                        let _ = crate::logging::log_error(format!(
                            "SDK runtime close failed safely: {error}"
                        ));
                    }
                }
            }
            Err(_) => {
                let _ = crate::logging::log_error(
                    "SDK executor lock was unavailable during shutdown.".to_owned(),
                );
            }
        }

        #[cfg(not(feature = "rt"))]
        {
            let _ = crate::logging::log_info(
                "Host must complete asynchronous SDK close for this runtime build.".to_owned(),
            );
        }
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
    fn runtime_owns_one_sdk_client_and_closes_idempotently() {
        let runtime = RadrootsRuntime::new().expect("runtime");
        let storage = runtime
            .client
            .capabilities()
            .get(CapabilityId::CANONICAL_STORAGE)
            .expect("storage capability");
        assert_eq!(storage.availability(), Availability::Available);
        assert!(!runtime.client.is_closed());

        runtime.stop();
        assert!(runtime.client.is_closed());
        runtime.stop();
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
        runtime.stop();
    }
}
