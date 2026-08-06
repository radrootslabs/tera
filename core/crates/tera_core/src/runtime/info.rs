use super::RadrootsRuntime;
use chrono::Utc;
use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default, uniffi::Record)]
pub struct RuntimeBuildInfo {
    pub crate_name: String,
    pub crate_version: String,
    pub rustc: Option<String>,
    pub profile: Option<String>,
    pub git_sha: Option<String>,
    pub build_time_unix: Option<u64>,
}

#[derive(Debug, Clone, Serialize, uniffi::Record)]
pub struct AppInfo {
    pub build: RuntimeBuildInfo,
    pub started_unix_ms: i64,
    pub uptime_millis: i64,
    pub shutting_down: bool,
    pub platform: Option<super::app_info::AppInfoPlatform>,
}

#[derive(Debug, Clone, Serialize, uniffi::Record)]
pub struct RuntimeInfo {
    pub app: AppInfo,
    pub sdk: RuntimeBuildInfo,
    pub sdk_closed: bool,
}

pub fn gather_runtime_info(runtime: &RadrootsRuntime) -> RuntimeInfo {
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

pub fn app_build_info() -> RuntimeBuildInfo {
    RuntimeBuildInfo {
        crate_name: env!("CARGO_PKG_NAME").to_owned(),
        crate_version: env!("CARGO_PKG_VERSION").to_owned(),
        rustc: option_env!("RUSTC_VERSION").map(str::to_owned),
        profile: option_env!("PROFILE").map(str::to_owned),
        git_sha: option_env!("GIT_HASH").map(str::to_owned),
        build_time_unix: option_env!("BUILD_TIME_UNIX").and_then(|value| value.parse().ok()),
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn build_info_uses_sdk_identity_without_lower_runtime_metadata() {
        let runtime = super::RadrootsRuntime::new().expect("runtime");
        let info = runtime.info();
        assert_eq!(info.sdk.crate_name, "radroots_sdk");
        assert_eq!(info.sdk.crate_version, "0.1.0-alpha");
        assert!(!info.sdk_closed);
    }
}
