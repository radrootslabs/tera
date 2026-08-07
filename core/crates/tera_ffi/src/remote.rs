//! UniFFI converter ownership for ordinary Rust DTOs defined by mobile core.

use radroots_mobile_core::runtime::app_info::*;
use radroots_mobile_core::runtime::info::*;
use radroots_mobile_core::runtime::key_management::*;
use radroots_mobile_core::runtime::nostr::*;
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
pub struct SdkShutdownRecord {
    pub state: String,
    pub already_closed: bool,
}

#[uniffi::remote(Record)]
pub struct NostrIdentityRecord {
    pub id: String,
    pub public_key_hex: String,
    pub public_key_npub: String,
    pub label: Option<String>,
    pub is_selected: bool,
}

#[uniffi::remote(Record)]
pub struct NostrIdentitySnapshot {
    pub has_selected_signing_identity: bool,
    pub selected_identity_id: Option<String>,
    pub selected_npub: Option<String>,
    pub identities: Vec<NostrIdentityRecord>,
}

#[uniffi::remote(Record)]
pub struct NostrHostCustodyIdentity {
    pub id: String,
    pub public_key_hex: String,
    pub public_key_npub: String,
}

#[uniffi::remote(Enum)]
pub enum NostrLight {
    Red,
    Yellow,
    Green,
}

#[uniffi::remote(Record)]
pub struct NostrConnectionStatus {
    pub light: NostrLight,
    pub configured: bool,
    pub source_available: bool,
    pub sink_available: bool,
    pub last_error: Option<String>,
}

#[uniffi::remote(Record)]
pub struct NostrProfile {
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub nip05: Option<String>,
    pub about: Option<String>,
    pub website: Option<String>,
    pub picture: Option<String>,
    pub banner: Option<String>,
    pub lud06: Option<String>,
    pub lud16: Option<String>,
    pub bot: Option<String>,
}

#[uniffi::remote(Record)]
pub struct NostrProfileEventMetadata {
    pub id: String,
    pub author: String,
    pub published_at: u64,
    pub profile: NostrProfile,
}

#[uniffi::remote(Record)]
pub struct NostrPost {
    pub content: String,
}

#[uniffi::remote(Record)]
pub struct NostrPostEventMetadata {
    pub id: String,
    pub author: String,
    pub published_at: u64,
    pub post: NostrPost,
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
