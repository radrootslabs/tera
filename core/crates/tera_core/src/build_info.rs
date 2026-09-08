//! Build metadata available without native runtime or storage dependencies.

use serde::Serialize;

#[derive(Debug, Clone, Serialize, Default)]
pub struct RuntimeBuildInfo {
    pub crate_name: String,
    pub crate_version: String,
    pub rustc: Option<String>,
    pub profile: Option<String>,
    pub lib_revision: Option<String>,
    pub consumer_revision: Option<String>,
    pub build_time_unix: Option<u64>,
}

pub fn app_build_info() -> RuntimeBuildInfo {
    RuntimeBuildInfo {
        crate_name: env!("CARGO_PKG_NAME").to_owned(),
        crate_version: env!("CARGO_PKG_VERSION").to_owned(),
        rustc: option_env!("RUSTC_VERSION").map(str::to_owned),
        profile: option_env!("PROFILE").map(str::to_owned),
        lib_revision: option_env!("RADROOTS_LIB_REVISION").map(str::to_owned),
        consumer_revision: option_env!("TERA_CONSUMER_REVISION").map(str::to_owned),
        build_time_unix: option_env!("BUILD_TIME_UNIX").and_then(|value| value.parse().ok()),
    }
}
