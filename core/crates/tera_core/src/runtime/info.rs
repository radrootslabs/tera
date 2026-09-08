use super::TeraRuntime;
use chrono::Utc;
use serde::Serialize;

pub use crate::build_info::{RuntimeBuildInfo, app_build_info};

#[derive(Debug, Clone, Serialize)]
pub struct AppInfo {
    pub build: RuntimeBuildInfo,
    pub started_unix_ms: i64,
    pub uptime_millis: i64,
    pub shutting_down: bool,
    pub platform: Option<super::app_info::AppInfoPlatform>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInfo {
    pub app: AppInfo,
    pub sdk: RuntimeBuildInfo,
    pub sdk_closed: bool,
}

pub fn gather_runtime_info(runtime: &TeraRuntime) -> RuntimeInfo {
    let now_ms = Utc::now().timestamp_millis();
    RuntimeInfo {
        app: AppInfo {
            build: app_build_info(),
            started_unix_ms: runtime.started_unix_ms,
            uptime_millis: now_ms - runtime.started_unix_ms,
            shutting_down: runtime
                .shutting_down
                .load(std::sync::atomic::Ordering::SeqCst),
            platform: runtime
                .platform_app
                .read()
                .ok()
                .and_then(|value| (*value).clone()),
        },
        sdk: RuntimeBuildInfo {
            crate_name: "radroots_sdk".to_owned(),
            crate_version: "0.1.0-alpha".to_owned(),
            ..RuntimeBuildInfo::default()
        },
        sdk_closed: runtime.client.is_closed(),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn build_info_uses_sdk_identity_without_lower_runtime_metadata() {
        let runtime = super::TeraRuntime::test_memory().expect("runtime");
        let info = runtime.info();
        assert_eq!(info.sdk.crate_name, "radroots_sdk");
        assert_eq!(info.sdk.crate_version, "0.1.0-alpha");
        assert!(!info.sdk_closed);
    }
}
