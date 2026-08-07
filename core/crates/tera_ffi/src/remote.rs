//! UniFFI converter ownership for ordinary Rust DTOs defined by mobile core.

use radroots_mobile_core::runtime::app_info::*;
use radroots_mobile_core::runtime::info::*;
use radroots_mobile_core::runtime::product_surface::*;
use radroots_mobile_core::runtime::sdk::*;
use radroots_mobile_core::{SdkErrorRecord, StoreErrorRecord};

#[uniffi::remote(Record)]
pub struct SdkErrorRecord {
    pub schema_version: u16,
    pub code: String,
    pub class: String,
    pub retryable: bool,
    pub recovery_actions: Vec<String>,
    pub operation_id: Option<String>,
    pub capability_id: Option<String>,
    pub message: String,
}

#[uniffi::remote(Record)]
pub struct StoreErrorRecord {
    pub schema_version: u16,
    pub code: String,
    pub class: String,
    pub retryable: bool,
    pub recovery_actions: Vec<String>,
    pub message: String,
}

#[uniffi::remote(Record)]
pub struct AppInfoPlatform {
    pub platform: Option<String>,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub build_number: Option<String>,
    pub build_sha: Option<String>,
}

#[uniffi::remote(Record)]
pub struct RuntimeBuildInfo {
    pub crate_name: String,
    pub crate_version: String,
    pub rustc: Option<String>,
    pub profile: Option<String>,
    pub lib_revision: Option<String>,
    pub consumer_revision: Option<String>,
    pub build_time_unix: Option<u64>,
}

#[uniffi::remote(Record)]
pub struct AppInfo {
    pub build: RuntimeBuildInfo,
    pub started_unix_ms: i64,
    pub uptime_millis: i64,
    pub shutting_down: bool,
    pub platform: Option<AppInfoPlatform>,
}

#[uniffi::remote(Record)]
pub struct RuntimeInfo {
    pub app: AppInfo,
    pub sdk: RuntimeBuildInfo,
    pub sdk_closed: bool,
}

#[uniffi::remote(Record)]
pub struct SdkCapabilityRecord {
    pub id: String,
    pub compiled: bool,
    pub configured: bool,
    pub availability: String,
    pub maturity: String,
}

#[uniffi::remote(Record)]
pub struct SdkStorageStatusRecord {
    pub backend: String,
    pub open_mode: String,
    pub shutdown: String,
    pub integrity: String,
}

#[uniffi::remote(Record)]
pub struct SdkRelayStatusRecord {
    pub relay_url: String,
    pub access: String,
    pub read_state: String,
    pub write_state: String,
    pub read_last_attempt_unix_ms: Option<u64>,
    pub write_last_attempt_unix_ms: Option<u64>,
    pub read_next_attempt_unix_ms: Option<u64>,
    pub write_next_attempt_unix_ms: Option<u64>,
}

#[uniffi::remote(Record)]
pub struct SdkRelayStatusReportRecord {
    pub profile: String,
    pub state: String,
    pub read_availability: String,
    pub write_availability: String,
    pub relays: Vec<SdkRelayStatusRecord>,
}

#[uniffi::remote(Record)]
pub struct SdkShutdownRecord {
    pub state: String,
    pub already_closed: bool,
}

#[uniffi::remote(Enum)]
pub enum TodayCardType {
    Update,
    PhotoUpdate,
    Ask,
    Event,
    FoodAvailability,
}

#[uniffi::remote(Enum)]
pub enum AddCommandType {
    CreateUpdate,
    CreatePhotoUpdate,
    CreateAsk,
    CreateEvent,
    CreateFoodAvailability,
}

#[uniffi::remote(Record)]
pub struct CardAddParity {
    pub card_type: TodayCardType,
    pub add_command_type: AddCommandType,
}

#[uniffi::remote(Record)]
pub struct LocalNetwork {
    pub id: String,
    pub label: String,
    pub relay_urls: Vec<String>,
    pub locality: Option<String>,
    pub followed_authors: Vec<String>,
    pub generation: u64,
}
