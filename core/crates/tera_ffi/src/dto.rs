//! Focused, versioned value types owned by the native boundary.

#[cfg(unix)]
use std::os::fd::{FromRawFd, OwnedFd, RawFd};
#[cfg(unix)]
use std::os::unix::fs::FileExt;

use radroots_blossom::{BlobDescriptor, MediaType, Sha256};
use radroots_event::{
    calendar::{AuthoredCalendarDateEvent, AuthoredCalendarTimeEvent, CalendarDate},
    food::availability::{
        FoodAvailabilityDetails, FoodAvailabilityDetailsParts, FoodAvailabilityImage,
        FoodAvailabilityStatus, FoodContent, FoodCurrency, FoodIdentifier, FoodImageDimensions,
        FoodPrice, FoodPublishedAt, FoodQuantity, FoodText, FoodUnit,
    },
    media::AuthoredImage,
    post::{AuthoredPostImage, PostImageDimensions},
};
use radroots_mobile_core::runtime::{
    app_info::AppInfoPlatform,
    info::{AppInfo, RuntimeBuildInfo, RuntimeInfo},
    product_surface::{
        AddCommandType, CardLifecycleState, CreateAsk, CreateEvent, CreateFoodAvailability,
        CreatePhotoUpdate, CreateUpdate, LocalNetwork, LocalNetworkRelayPolicy, MeSnapshot,
        MediaReference, MediaVerificationState, Phase1AddCommand, Phase1CancellationPolicy,
        Phase1DraftEventTiming, Phase1DraftFormSnapshot, Phase1DraftKind, Phase1DraftMediaSnapshot,
        Phase1DraftStatus, Phase1MediaPrerequisite, Phase1MediaStage, Phase1OutboxState,
        Phase1QueuePolicy, Phase1RelaySatisfaction, ProfileSummary, SearchResult, SearchResultType,
        SupportingProfile, ThreadEntry, TodayCard, TodayCardType, TodayPage, TodayProjectionUpdate,
        TodayRefreshReceipt, TodayRelaySyncState, TodaySyncReceipt,
    },
    sdk::{
        SdkBlossomConfigurationRecord, SdkBlossomEvidenceRecord, SdkCapabilityRecord,
        SdkRelayStatusRecord, SdkRelayStatusReportRecord, SdkShutdownRecord,
        SdkStorageStatusRecord,
    },
};

use crate::RadrootsAppError;

pub const MOBILE_FFI_SCHEMA_VERSION: u16 = 1;
const MEDIA_FILE_MAX_BYTES: u64 = 10 * 1024 * 1024;
const MEDIA_REFERENCE_MAX_BYTES: usize = 256;

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiBuildInfoRecord {
    pub schema_version: u16,
    pub crate_name: String,
    pub crate_version: String,
    pub rustc: Option<String>,
    pub profile: Option<String>,
    pub lib_revision: Option<String>,
    pub consumer_revision: Option<String>,
    pub build_time_unix: Option<u64>,
}

// These exhaustive field-for-field adapters are verified by the generated API
// snapshot and Swift compilation. Excluding the mechanical projection glue
// keeps the coverage gate focused on validation and behavior.
#[cfg_attr(coverage_nightly, coverage(off))]
impl From<RuntimeBuildInfo> for FfiBuildInfoRecord {
    fn from(value: RuntimeBuildInfo) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            crate_name: value.crate_name,
            crate_version: value.crate_version,
            rustc: value.rustc,
            profile: value.profile,
            lib_revision: value.lib_revision,
            consumer_revision: value.consumer_revision,
            build_time_unix: value.build_time_unix,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiAppPlatformRecord {
    pub schema_version: u16,
    pub platform: Option<String>,
    pub bundle_id: Option<String>,
    pub version: Option<String>,
    pub build_number: Option<String>,
    pub build_sha: Option<String>,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<AppInfoPlatform> for FfiAppPlatformRecord {
    fn from(value: AppInfoPlatform) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            platform: value.platform,
            bundle_id: value.bundle_id,
            version: value.version,
            build_number: value.build_number,
            build_sha: value.build_sha,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiAppInfoRecord {
    pub schema_version: u16,
    pub build: FfiBuildInfoRecord,
    pub started_unix_ms: i64,
    pub uptime_millis: i64,
    pub shutting_down: bool,
    pub platform: Option<FfiAppPlatformRecord>,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<AppInfo> for FfiAppInfoRecord {
    fn from(value: AppInfo) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            build: value.build.into(),
            started_unix_ms: value.started_unix_ms,
            uptime_millis: value.uptime_millis,
            shutting_down: value.shutting_down,
            platform: value.platform.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRuntimeInfoRecord {
    pub schema_version: u16,
    pub app: FfiAppInfoRecord,
    pub sdk: FfiBuildInfoRecord,
    pub sdk_closed: bool,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<RuntimeInfo> for FfiRuntimeInfoRecord {
    fn from(value: RuntimeInfo) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            app: value.app.into(),
            sdk: value.sdk.into(),
            sdk_closed: value.sdk_closed,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiIdentityStatusRecord {
    pub schema_version: u16,
    pub public_key: String,
    pub host_signer_configured: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiTodayCardType {
    Update,
    PhotoUpdate,
    Ask,
    Event,
    FoodAvailability,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<TodayCardType> for FfiTodayCardType {
    fn from(value: TodayCardType) -> Self {
        match value {
            TodayCardType::Update => Self::Update,
            TodayCardType::PhotoUpdate => Self::PhotoUpdate,
            TodayCardType::Ask => Self::Ask,
            TodayCardType::Event => Self::Event,
            TodayCardType::FoodAvailability => Self::FoodAvailability,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiAddCommandType {
    CreateUpdate,
    CreatePhotoUpdate,
    CreateAsk,
    CreateEvent,
    CreateFoodAvailability,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<AddCommandType> for FfiAddCommandType {
    fn from(value: AddCommandType) -> Self {
        match value {
            AddCommandType::CreateUpdate => Self::CreateUpdate,
            AddCommandType::CreatePhotoUpdate => Self::CreatePhotoUpdate,
            AddCommandType::CreateAsk => Self::CreateAsk,
            AddCommandType::CreateEvent => Self::CreateEvent,
            AddCommandType::CreateFoodAvailability => Self::CreateFoodAvailability,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiCardAddParityRecord {
    pub schema_version: u16,
    pub card_type: FfiTodayCardType,
    pub command_type: FfiAddCommandType,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiLocalNetworkRecord {
    pub schema_version: u16,
    pub id: String,
    pub label: String,
    pub relay_urls: Vec<String>,
    pub locality: Option<String>,
    pub followed_authors: Vec<String>,
    pub generation: u64,
}

impl TryFrom<FfiLocalNetworkRecord> for LocalNetwork {
    type Error = RadrootsAppError;

    fn try_from(value: FfiLocalNetworkRecord) -> Result<Self, Self::Error> {
        value.try_into_with_relay_policy(LocalNetworkRelayPolicy::Public)
    }
}

impl FfiLocalNetworkRecord {
    pub(crate) fn try_into_with_relay_policy(
        self,
        relay_policy: LocalNetworkRelayPolicy,
    ) -> Result<LocalNetwork, RadrootsAppError> {
        require_schema(self.schema_version)?;
        LocalNetwork::new_for_relay_policy(
            self.id,
            self.label,
            self.relay_urls,
            self.locality,
            self.followed_authors,
            self.generation,
            relay_policy,
        )
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_local_network"))
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<LocalNetwork> for FfiLocalNetworkRecord {
    fn from(value: LocalNetwork) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            id: value.id,
            label: value.label,
            relay_urls: value.relay_urls,
            locality: value.locality,
            followed_authors: value.followed_authors,
            generation: value.generation,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiMediaVerificationState {
    Pending,
    Verified,
    Failed,
    Unavailable,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<MediaVerificationState> for FfiMediaVerificationState {
    fn from(value: MediaVerificationState) -> Self {
        match value {
            MediaVerificationState::Pending => Self::Pending,
            MediaVerificationState::Verified => Self::Verified,
            MediaVerificationState::Failed => Self::Failed,
            MediaVerificationState::Unavailable => Self::Unavailable,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiMediaReferenceRecord {
    pub schema_version: u16,
    pub url: String,
    pub sha256: Option<String>,
    pub media_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub byte_size: Option<u64>,
    pub alt: Option<String>,
    pub verification: FfiMediaVerificationState,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<MediaReference> for FfiMediaReferenceRecord {
    fn from(value: MediaReference) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            url: value.url,
            sha256: value.sha256,
            media_type: value.media_type,
            width: value.width,
            height: value.height,
            byte_size: value.byte_size,
            alt: value.alt,
            verification: value.verification.into(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiProfileRecord {
    pub schema_version: u16,
    pub author_public_key: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub about: Option<String>,
    pub picture: Option<FfiMediaReferenceRecord>,
    pub banner: Option<FfiMediaReferenceRecord>,
    pub nip05: Option<String>,
    pub website: Option<String>,
    pub lightning_address: Option<String>,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<ProfileSummary> for FfiProfileRecord {
    fn from(value: ProfileSummary) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            author_public_key: value.author_pubkey,
            name: value.name,
            display_name: value.display_name,
            about: value.about,
            picture: value.picture.map(Into::into),
            banner: value.banner.map(Into::into),
            nip05: value.nip05,
            website: value.website,
            lightning_address: value.lightning_address,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiThreadProfile {
    Profile,
    Reply,
    Comment,
    Deletion,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<SupportingProfile> for FfiThreadProfile {
    fn from(value: SupportingProfile) -> Self {
        match value {
            SupportingProfile::Profile => Self::Profile,
            SupportingProfile::Reply => Self::Reply,
            SupportingProfile::Comment => Self::Comment,
            SupportingProfile::Deletion => Self::Deletion,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiThreadEntryRecord {
    pub schema_version: u16,
    pub event_id: String,
    pub author_public_key: String,
    pub content: String,
    pub authored_at_unix_s: u64,
    pub profile: FfiThreadProfile,
    pub root: String,
    pub parent_event_id: String,
    pub author_profile: Option<FfiProfileRecord>,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<ThreadEntry> for FfiThreadEntryRecord {
    fn from(value: ThreadEntry) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            event_id: value.event_id,
            author_public_key: value.author_pubkey,
            content: value.content,
            authored_at_unix_s: value.authored_at,
            profile: value.reference.profile.into(),
            root: value.reference.root,
            parent_event_id: value.reference.parent_event_id,
            author_profile: value.author_profile.map(Into::into),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiCardLifecycleState {
    Active,
    Sold,
    Past,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<CardLifecycleState> for FfiCardLifecycleState {
    fn from(value: CardLifecycleState) -> Self {
        match value {
            CardLifecycleState::Active => Self::Active,
            CardLifecycleState::Sold => Self::Sold,
            CardLifecycleState::Past => Self::Past,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiTodayCardRecord {
    pub schema_version: u16,
    pub card_id: String,
    pub card_type: FfiTodayCardType,
    pub source_event_id: String,
    pub source_address: Option<String>,
    pub author_public_key: String,
    pub contract_id: String,
    pub title: Option<String>,
    pub content: String,
    pub authored_at_unix_s: u64,
    pub effective_at_unix_s: u64,
    pub event_start_unix_s: Option<u64>,
    pub event_end_unix_s: Option<u64>,
    pub location: Option<String>,
    pub price_amount: Option<String>,
    pub price_currency: Option<String>,
    pub price_unit: Option<String>,
    pub quantity: Option<String>,
    pub food_summary: Option<String>,
    pub food_published_at_unix_s: Option<u64>,
    pub food_status: Option<String>,
    pub context_rank: u8,
    pub inclusion_reason: String,
    pub media: Vec<FfiMediaReferenceRecord>,
    pub lifecycle: FfiCardLifecycleState,
    pub rank_digest: Option<String>,
    pub author_profile: Option<FfiProfileRecord>,
    pub thread: Vec<FfiThreadEntryRecord>,
    pub local_operation_id: Option<String>,
    pub local_operation_state: Option<String>,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<TodayCard> for FfiTodayCardRecord {
    fn from(value: TodayCard) -> Self {
        let card = value.card;
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            card_id: card.card_id.to_hex(),
            card_type: card.card_type.into(),
            source_event_id: card.source_event_id,
            source_address: card.source_address,
            author_public_key: card.author_pubkey,
            contract_id: card.contract_id,
            title: card.title,
            content: card.content,
            authored_at_unix_s: card.authored_at,
            effective_at_unix_s: card.effective_at,
            event_start_unix_s: card.event_start,
            event_end_unix_s: card.event_end,
            location: card.location,
            price_amount: card.price_amount,
            price_currency: card.price_currency,
            price_unit: card.price_unit,
            quantity: card.quantity,
            food_summary: card.food_summary,
            food_published_at_unix_s: card.food_published_at,
            food_status: card.food_status,
            context_rank: card.context_rank.value(),
            inclusion_reason: card.inclusion_reason,
            media: card.media.into_iter().map(Into::into).collect(),
            lifecycle: card.lifecycle.into(),
            rank_digest: card.rank.map(|rank| rank.digest_hex()),
            author_profile: value.author_profile.map(Into::into),
            thread: value.thread.into_iter().map(Into::into).collect(),
            local_operation_id: value
                .local_overlay
                .as_ref()
                .map(|overlay| overlay.operation_id.clone()),
            local_operation_state: value.local_overlay.map(|overlay| overlay.state),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiTodayPageRecord {
    pub schema_version: u16,
    pub as_of_unix_s: u64,
    pub items: Vec<FfiTodayCardRecord>,
    pub next_cursor: Option<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiTodayProjectionUpdate {
    Incremental,
    Rebuild,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<FfiTodayProjectionUpdate> for TodayProjectionUpdate {
    fn from(value: FfiTodayProjectionUpdate) -> Self {
        match value {
            FfiTodayProjectionUpdate::Incremental => Self::Incremental,
            FfiTodayProjectionUpdate::Rebuild => Self::Rebuild,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiTodayRefreshRecord {
    pub schema_version: u16,
    pub update: FfiTodayProjectionUpdate,
    pub source_events: u64,
    pub visible_cards: u64,
    pub profiles: u64,
    pub thread_entries: u64,
    pub content_generation: u64,
    pub changed: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiTodayRelaySyncState {
    Complete,
    Partial,
    Offline,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiTodaySyncRecord {
    pub schema_version: u16,
    pub relay_state: FfiTodayRelaySyncState,
    pub pages_fetched: u16,
    pub events_observed: u64,
    pub events_admitted: u64,
    pub events_rejected: u64,
    pub projection: FfiTodayRefreshRecord,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<TodaySyncReceipt> for FfiTodaySyncRecord {
    fn from(value: TodaySyncReceipt) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            relay_state: match value.relay_state {
                TodayRelaySyncState::Complete => FfiTodayRelaySyncState::Complete,
                TodayRelaySyncState::Partial => FfiTodayRelaySyncState::Partial,
                TodayRelaySyncState::Offline => FfiTodayRelaySyncState::Offline,
            },
            pages_fetched: value.pages_fetched,
            events_observed: value.events_observed,
            events_admitted: value.events_admitted,
            events_rejected: value.events_rejected,
            projection: value.projection.into(),
        }
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<TodayRefreshReceipt> for FfiTodayRefreshRecord {
    fn from(value: TodayRefreshReceipt) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            update: match value.update {
                TodayProjectionUpdate::Incremental => FfiTodayProjectionUpdate::Incremental,
                TodayProjectionUpdate::Rebuild => FfiTodayProjectionUpdate::Rebuild,
            },
            source_events: value.source_events,
            visible_cards: value.visible_cards,
            profiles: value.profiles,
            thread_entries: value.thread_entries,
            content_generation: value.content_generation,
            changed: value.changed,
        }
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<TodayPage> for FfiTodayPageRecord {
    fn from(value: TodayPage) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            as_of_unix_s: value.as_of,
            items: value.items.into_iter().map(Into::into).collect(),
            next_cursor: value.next_cursor,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiSearchResultType {
    Card,
    Profile,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiSearchResultRecord {
    pub schema_version: u16,
    pub result_type: FfiSearchResultType,
    pub stable_id: String,
    pub card: Option<FfiTodayCardRecord>,
    pub profile: Option<FfiProfileRecord>,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<SearchResult> for FfiSearchResultRecord {
    fn from(value: SearchResult) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            result_type: match value.result_type {
                SearchResultType::Card => FfiSearchResultType::Card,
                SearchResultType::Profile => FfiSearchResultType::Profile,
            },
            stable_id: value.stable_id,
            card: value.card.map(Into::into),
            profile: value.profile.map(Into::into),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiMeRecord {
    pub schema_version: u16,
    pub public_key: String,
    pub profile: Option<FfiProfileRecord>,
    pub cards: Vec<FfiTodayCardRecord>,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<MeSnapshot> for FfiMeRecord {
    fn from(value: MeSnapshot) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            public_key: value.public_key,
            profile: value.profile.map(Into::into),
            cards: value.cards.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiAddFieldKind {
    Text,
    MultilineText,
    Date,
    DateTime,
    Decimal,
    Choice,
    Location,
    Media,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiAddFieldRecord {
    pub schema_version: u16,
    pub id: String,
    pub label: String,
    pub kind: FfiAddFieldKind,
    pub required: bool,
    pub choices: Vec<String>,
    pub max_bytes: Option<u64>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiAddSchemaRecord {
    pub schema_version: u16,
    pub command_type: FfiAddCommandType,
    pub label: String,
    pub fields: Vec<FfiAddFieldRecord>,
}

pub fn add_schemas() -> Vec<FfiAddSchemaRecord> {
    use FfiAddCommandType as Command;
    use FfiAddFieldKind as Kind;

    let field =
        |id: &str, label: &str, kind, required, choices: &[&str], max_bytes| FfiAddFieldRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            id: id.to_owned(),
            label: label.to_owned(),
            kind,
            required,
            choices: choices.iter().map(|value| (*value).to_owned()).collect(),
            max_bytes,
        };
    vec![
        FfiAddSchemaRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            command_type: Command::CreateUpdate,
            label: "Update".to_owned(),
            fields: vec![field(
                "content",
                "Update",
                Kind::MultilineText,
                true,
                &[],
                Some(65_535),
            )],
        },
        FfiAddSchemaRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            command_type: Command::CreatePhotoUpdate,
            label: "Photo update".to_owned(),
            fields: vec![
                field(
                    "content",
                    "Update",
                    Kind::MultilineText,
                    true,
                    &[],
                    Some(65_535),
                ),
                field(
                    "media",
                    "Photos",
                    Kind::Media,
                    true,
                    &[],
                    Some(MEDIA_FILE_MAX_BYTES),
                ),
            ],
        },
        FfiAddSchemaRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            command_type: Command::CreateAsk,
            label: "Ask".to_owned(),
            fields: vec![
                field(
                    "content",
                    "Question",
                    Kind::MultilineText,
                    true,
                    &[],
                    Some(65_535),
                ),
                field(
                    "media",
                    "Photos",
                    Kind::Media,
                    false,
                    &[],
                    Some(MEDIA_FILE_MAX_BYTES),
                ),
            ],
        },
        FfiAddSchemaRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            command_type: Command::CreateEvent,
            label: "Event".to_owned(),
            fields: vec![
                field("identifier", "Identifier", Kind::Text, true, &[], Some(256)),
                field("title", "Title", Kind::Text, true, &[], Some(256)),
                field(
                    "content",
                    "Description",
                    Kind::MultilineText,
                    false,
                    &[],
                    Some(65_535),
                ),
                field("event_start", "Starts", Kind::DateTime, true, &[], None),
                field("event_end", "Ends", Kind::DateTime, false, &[], None),
                field(
                    "location",
                    "Location",
                    Kind::Location,
                    false,
                    &[],
                    Some(256),
                ),
                field(
                    "media",
                    "Photo",
                    Kind::Media,
                    false,
                    &[],
                    Some(MEDIA_FILE_MAX_BYTES),
                ),
            ],
        },
        FfiAddSchemaRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            command_type: Command::CreateFoodAvailability,
            label: "Food availability".to_owned(),
            fields: vec![
                field("identifier", "Identifier", Kind::Text, true, &[], Some(256)),
                field("title", "Food", Kind::Text, true, &[], Some(256)),
                field("summary", "Summary", Kind::Text, true, &[], Some(256)),
                field(
                    "content",
                    "Details",
                    Kind::MultilineText,
                    true,
                    &[],
                    Some(65_535),
                ),
                field(
                    "location",
                    "Pickup location",
                    Kind::Location,
                    true,
                    &[],
                    Some(256),
                ),
                field("price_amount", "Price", Kind::Decimal, true, &[], Some(64)),
                field("currency", "Currency", Kind::Choice, true, &[], Some(3)),
                field(
                    "unit",
                    "Unit",
                    Kind::Choice,
                    true,
                    &[
                        "g", "kg", "lb", "oz", "each", "dozen", "bunch", "punnet", "bag", "basket",
                    ],
                    None,
                ),
                field(
                    "quantity",
                    "Available quantity",
                    Kind::Decimal,
                    false,
                    &[],
                    Some(64),
                ),
                field(
                    "media",
                    "Photos",
                    Kind::Media,
                    false,
                    &[],
                    Some(MEDIA_FILE_MAX_BYTES),
                ),
            ],
        },
    ]
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiEventTimingKind {
    AllDay,
    Timed,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiPreparedMediaInput {
    pub schema_version: u16,
    pub opaque_reference: String,
    pub file_descriptor: u64,
    pub sha256: String,
    pub media_type: String,
    pub byte_size: u64,
    pub width: u32,
    pub height: u32,
    pub alt: String,
    pub prepared_at_unix_s: u64,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiAddDraftInput {
    pub schema_version: u16,
    pub command_type: FfiAddCommandType,
    pub content: String,
    pub identifier: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub location: Option<String>,
    pub event_timing: Option<FfiEventTimingKind>,
    pub event_start_date: Option<String>,
    pub event_end_date: Option<String>,
    pub event_start_unix_s: Option<u64>,
    pub event_end_unix_s: Option<u64>,
    pub event_timezone: Option<String>,
    pub price_amount: Option<String>,
    pub currency: Option<String>,
    pub unit: Option<String>,
    pub quantity: Option<String>,
    pub food_published_at_unix_s: Option<u64>,
    pub food_status: Option<String>,
    pub media: Vec<FfiPreparedMediaInput>,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRetractionDraftInput {
    pub schema_version: u16,
    pub command_type: FfiAddCommandType,
    pub target_card_id: String,
    pub target_event_id: String,
    pub target_kind: u32,
    pub target_address: Option<String>,
    pub reason: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiBlossomUploadInput {
    pub schema_version: u16,
    pub draft_id: String,
    pub expected_revision: u64,
    pub media: FfiPreparedMediaInput,
    pub authorization_content: String,
    pub authorization_created_at_unix_s: u64,
    pub authorization_lifetime_seconds: u64,
    pub operation_id: String,
    pub artifact_id: String,
    pub signing_deadline_unix_ms: u64,
    pub signing_cancellation: FfiCancellationPolicy,
    pub verified_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

impl FfiAddDraftInput {
    pub(crate) fn command_and_media(
        self,
        authored_at_unix_s: u64,
        blossom: Option<&radroots_sdk::transport::BlossomSlot>,
    ) -> Result<(Phase1AddCommand, Vec<Phase1MediaPrerequisite>), RadrootsAppError> {
        self.command_media_and_form(authored_at_unix_s, blossom)
            .map(|(command, media, _)| (command, media))
    }

    pub(crate) fn command_media_and_form(
        self,
        authored_at_unix_s: u64,
        blossom: Option<&radroots_sdk::transport::BlossomSlot>,
    ) -> Result<
        (
            Phase1AddCommand,
            Vec<Phase1MediaPrerequisite>,
            Phase1DraftFormSnapshot,
        ),
        RadrootsAppError,
    > {
        require_schema(self.schema_version)?;
        if authored_at_unix_s == 0 || self.media.len() > 20 {
            return Err(RadrootsAppError::invalid_argument("invalid_add_draft"));
        }
        let prepared = self
            .media
            .iter()
            .cloned()
            .map(PreparedMedia::try_from)
            .map(|media| {
                media.and_then(|media| {
                    let blossom = blossom.ok_or_else(|| {
                        RadrootsAppError::invalid_argument("blossom_not_configured")
                    })?;
                    media.bind(blossom)
                })
            })
            .collect::<Result<Vec<BoundPreparedMedia>, _>>()?;
        let prerequisites = prepared
            .iter()
            .map(|value| {
                Phase1MediaPrerequisite::new(
                    value.media.opaque_reference.clone(),
                    &value.descriptor,
                )
            })
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| RadrootsAppError::invalid_argument("invalid_media_reference"))?;
        let post_images = prepared
            .iter()
            .map(BoundPreparedMedia::post_image)
            .collect::<Result<Vec<_>, _>>()?;
        let form = self.form_snapshot(&prepared);
        let command = match self.command_type {
            FfiAddCommandType::CreateUpdate => {
                reject_media(&prepared)?;
                Phase1AddCommand::CreateUpdate(
                    CreateUpdate::new(self.content)
                        .map_err(|_| RadrootsAppError::invalid_argument("invalid_update"))?,
                )
            }
            FfiAddCommandType::CreatePhotoUpdate => Phase1AddCommand::CreatePhotoUpdate(
                CreatePhotoUpdate::new(
                    content_with_media_references(self.content, &prepared)?,
                    post_images,
                )
                .map_err(|_| RadrootsAppError::invalid_argument("invalid_photo_update"))?,
            ),
            FfiAddCommandType::CreateAsk => Phase1AddCommand::CreateAsk(
                CreateAsk::new(
                    content_with_media_references(self.content, &prepared)?,
                    post_images,
                )
                .map_err(|_| RadrootsAppError::invalid_argument("invalid_ask"))?,
            ),
            FfiAddCommandType::CreateEvent => {
                Phase1AddCommand::CreateEvent(event_command(&self, prepared.first())?)
            }
            FfiAddCommandType::CreateFoodAvailability => Phase1AddCommand::CreateFoodAvailability(
                food_command(self, authored_at_unix_s, &prepared)?,
            ),
        };
        Ok((command, prerequisites, form))
    }

    fn form_snapshot(&self, prepared: &[BoundPreparedMedia]) -> Phase1DraftFormSnapshot {
        Phase1DraftFormSnapshot {
            command_type: match self.command_type {
                FfiAddCommandType::CreateUpdate => AddCommandType::CreateUpdate,
                FfiAddCommandType::CreatePhotoUpdate => AddCommandType::CreatePhotoUpdate,
                FfiAddCommandType::CreateAsk => AddCommandType::CreateAsk,
                FfiAddCommandType::CreateEvent => AddCommandType::CreateEvent,
                FfiAddCommandType::CreateFoodAvailability => AddCommandType::CreateFoodAvailability,
            },
            content: self.content.clone(),
            identifier: self.identifier.clone(),
            title: self.title.clone(),
            summary: self.summary.clone(),
            location: self.location.clone(),
            event_timing: self.event_timing.map(|value| match value {
                FfiEventTimingKind::AllDay => Phase1DraftEventTiming::AllDay,
                FfiEventTimingKind::Timed => Phase1DraftEventTiming::Timed,
            }),
            event_start_date: self.event_start_date.clone(),
            event_end_date: self.event_end_date.clone(),
            event_start_unix_s: self.event_start_unix_s,
            event_end_unix_s: self.event_end_unix_s,
            event_timezone: self.event_timezone.clone(),
            price_amount: self.price_amount.clone(),
            currency: self.currency.clone(),
            unit: self.unit.clone(),
            quantity: self.quantity.clone(),
            food_published_at_unix_s: self.food_published_at_unix_s,
            food_status: self.food_status.clone(),
            media: prepared
                .iter()
                .map(|value| Phase1DraftMediaSnapshot {
                    opaque_reference: value.media.opaque_reference.clone(),
                    url: value.descriptor.url().as_str().to_owned(),
                    sha256: value.media.sha256.to_hex(),
                    media_type: value.media.media_type.as_str().to_owned(),
                    byte_size: value.media.byte_size,
                    width: value.media.width,
                    height: value.media.height,
                    alt: value.media.alt.clone(),
                    prepared_at_unix_s: value.media.prepared_at_unix_s,
                })
                .collect(),
        }
    }
}

pub(crate) struct PreparedMedia {
    opaque_reference: String,
    sha256: Sha256,
    byte_size: u64,
    prepared_at_unix_s: u64,
    bytes: std::sync::Arc<[u8]>,
    media_type: MediaType,
    width: u32,
    height: u32,
    alt: String,
}

struct BoundPreparedMedia {
    media: PreparedMedia,
    descriptor: radroots_blossom::ByteVerifiedDescriptor,
}

impl TryFrom<FfiPreparedMediaInput> for PreparedMedia {
    type Error = RadrootsAppError;

    fn try_from(value: FfiPreparedMediaInput) -> Result<Self, Self::Error> {
        require_schema(value.schema_version)?;
        if !opaque_media_reference_is_valid(&value.opaque_reference)
            || value.byte_size == 0
            || value.byte_size > MEDIA_FILE_MAX_BYTES
            || value.width == 0
            || value.height == 0
            || value.prepared_at_unix_s == 0
            || value.alt.trim().is_empty()
            || value.alt.len() > 1_024
        {
            return Err(RadrootsAppError::invalid_argument(
                "invalid_media_reference",
            ));
        }
        let byte_size = usize::try_from(value.byte_size)
            .map_err(|_| RadrootsAppError::invalid_argument("media_size_mismatch"))?;
        let bytes = read_media_file_descriptor(value.file_descriptor, value.byte_size, byte_size)?;
        let media_type = MediaType::parse(&value.media_type)
            .map_err(|_| RadrootsAppError::invalid_argument("invalid_media_type"))?;
        let sha256 = Sha256::from_hex(&value.sha256)
            .map_err(|_| RadrootsAppError::invalid_argument("invalid_media_digest"))?;
        if Sha256::digest(&bytes) != sha256 {
            return Err(RadrootsAppError::invalid_argument(
                "media_verification_failed",
            ));
        }
        let dimensions =
            radroots_sdk::transport::BlossomImageDimensions::new(value.width, value.height)
                .map_err(|_| RadrootsAppError::invalid_argument("invalid_image_dimensions"))?;
        let verified_at_unix_ms = value
            .prepared_at_unix_s
            .checked_mul(1_000)
            .ok_or_else(|| RadrootsAppError::invalid_argument("invalid_media_reference"))?;
        radroots_sdk::transport::BlossomUploadRequest::new(
            bytes.clone().into(),
            media_type.clone(),
            dimensions,
            verified_at_unix_ms,
        )
        .map_err(|_| RadrootsAppError::invalid_argument("media_verification_failed"))?;
        Ok(Self {
            opaque_reference: value.opaque_reference,
            sha256,
            byte_size: value.byte_size,
            prepared_at_unix_s: value.prepared_at_unix_s,
            bytes: bytes.into(),
            media_type,
            width: value.width,
            height: value.height,
            alt: value.alt,
        })
    }
}

#[cfg(unix)]
fn read_media_file_descriptor(
    file_descriptor: u64,
    expected_size: u64,
    byte_size: usize,
) -> Result<Vec<u8>, RadrootsAppError> {
    let raw_file_descriptor = RawFd::try_from(file_descriptor)
        .map_err(|_| RadrootsAppError::invalid_argument("media_handle_unavailable"))?;
    // SAFETY: `fcntl(F_DUPFD_CLOEXEC)` accepts any in-range integer descriptor
    // and reports EBADF for an unavailable one. No borrowed or owned Rust
    // descriptor is constructed until the kernel has duplicated it.
    let duplicated = unsafe { libc::fcntl(raw_file_descriptor, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicated < 0 {
        return Err(RadrootsAppError::invalid_argument(
            "media_handle_unavailable",
        ));
    }
    // SAFETY: a nonnegative F_DUPFD_CLOEXEC result is a new descriptor owned by
    // this call. The host's original descriptor remains independently owned.
    let owned = unsafe { OwnedFd::from_raw_fd(duplicated) };
    let file = std::fs::File::from(owned);
    let metadata = file
        .metadata()
        .map_err(|_| RadrootsAppError::invalid_argument("media_handle_unavailable"))?;
    if !metadata.is_file() || metadata.len() != expected_size {
        return Err(RadrootsAppError::invalid_argument("media_size_mismatch"));
    }
    let mut bytes = vec![0; byte_size];
    file.read_exact_at(&mut bytes, 0)
        .map_err(|_| RadrootsAppError::invalid_argument("media_read_failed"))?;
    Ok(bytes)
}

#[cfg(not(unix))]
fn read_media_file_descriptor(
    _file_descriptor: u64,
    _expected_size: u64,
    _byte_size: usize,
) -> Result<Vec<u8>, RadrootsAppError> {
    Err(RadrootsAppError::failure(
        "media_handle_unsupported",
        "capability",
        false,
        &[],
        "Protected media handles are unsupported on this platform.",
    ))
}

impl PreparedMedia {
    pub(crate) fn upload_request(
        &self,
        verified_at_unix_ms: u64,
    ) -> Result<radroots_sdk::transport::BlossomUploadRequest, RadrootsAppError> {
        let dimensions =
            radroots_sdk::transport::BlossomImageDimensions::new(self.width, self.height)
                .map_err(|_| RadrootsAppError::invalid_argument("invalid_image_dimensions"))?;
        radroots_sdk::transport::BlossomUploadRequest::new(
            std::sync::Arc::clone(&self.bytes),
            self.media_type.clone(),
            dimensions,
            verified_at_unix_ms,
        )
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_blossom_upload"))
    }

    fn bind(
        self,
        blossom: &radroots_sdk::transport::BlossomSlot,
    ) -> Result<BoundPreparedMedia, RadrootsAppError> {
        let verified_at_unix_ms = self
            .prepared_at_unix_s
            .checked_mul(1_000)
            .ok_or_else(|| RadrootsAppError::invalid_argument("invalid_media_reference"))?;
        let transaction = blossom
            .prepare_upload(self.upload_request(verified_at_unix_ms)?)
            .map_err(|error| RadrootsAppError::invalid_argument(error.code()))?;
        let descriptor = BlobDescriptor::new(
            transaction.expected_url().clone(),
            self.sha256,
            self.byte_size,
            self.media_type.clone(),
            self.prepared_at_unix_s,
        )
        .and_then(BlobDescriptor::approve_reference)
        .and_then(|descriptor| descriptor.verify_bytes(&self.bytes, &self.media_type))
        .map_err(|_| RadrootsAppError::invalid_argument("media_verification_failed"))?;
        Ok(BoundPreparedMedia {
            media: self,
            descriptor,
        })
    }
}

impl BoundPreparedMedia {
    fn authored_image(&self) -> Result<AuthoredImage, RadrootsAppError> {
        AuthoredImage::try_from_verified_descriptor(self.descriptor.clone())
            .map_err(|_| RadrootsAppError::invalid_argument("invalid_image_media"))
    }

    fn post_image(&self) -> Result<AuthoredPostImage, RadrootsAppError> {
        AuthoredPostImage::new(
            self.authored_image()?,
            PostImageDimensions::new(self.media.width, self.media.height)
                .map_err(|_| RadrootsAppError::invalid_argument("invalid_image_dimensions"))?,
            self.media.alt.clone(),
        )
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_image"))
    }
}

fn event_command(
    input: &FfiAddDraftInput,
    image: Option<&BoundPreparedMedia>,
) -> Result<CreateEvent, RadrootsAppError> {
    if input.media.len() > 1 {
        return Err(RadrootsAppError::invalid_argument("event_image_limit"));
    }
    let identifier = required(input.identifier.as_deref(), "event_identifier_required")?;
    let title = required(input.title.as_deref(), "event_title_required")?;
    let timing = input
        .event_timing
        .ok_or_else(|| RadrootsAppError::invalid_argument("event_timing_required"))?;
    match timing {
        FfiEventTimingKind::AllDay => {
            let start = CalendarDate::parse(required(
                input.event_start_date.as_deref(),
                "event_start_date_required",
            )?)
            .map_err(|_| RadrootsAppError::invalid_argument("invalid_event_start_date"))?;
            let mut event = AuthoredCalendarDateEvent::new(identifier, title, start)
                .map_err(|_| RadrootsAppError::invalid_argument("invalid_event"))?;
            if let Some(end) = input.event_end_date.as_deref() {
                event = event
                    .with_end(CalendarDate::parse(end).map_err(|_| {
                        RadrootsAppError::invalid_argument("invalid_event_end_date")
                    })?)
                    .map_err(|_| RadrootsAppError::invalid_argument("invalid_event_range"))?;
            }
            if !input.content.is_empty() {
                event = event
                    .with_description(input.content.clone())
                    .map_err(|_| RadrootsAppError::invalid_argument("invalid_event_description"))?;
            }
            if let Some(location) = input.location.clone() {
                event = event
                    .with_locations(vec![location])
                    .map_err(|_| RadrootsAppError::invalid_argument("invalid_event_location"))?;
            }
            if let Some(image) = image {
                event = event
                    .with_image(image.authored_image()?)
                    .map_err(|_| RadrootsAppError::invalid_argument("invalid_event_image"))?;
            }
            Ok(CreateEvent::date(event))
        }
        FfiEventTimingKind::Timed => {
            let start = input
                .event_start_unix_s
                .filter(|value| *value != 0)
                .ok_or_else(|| RadrootsAppError::invalid_argument("event_start_required"))?;
            let mut event = AuthoredCalendarTimeEvent::new(identifier, title, start)
                .map_err(|_| RadrootsAppError::invalid_argument("invalid_event"))?;
            if let Some(end) = input.event_end_unix_s {
                event = event
                    .with_end(end)
                    .map_err(|_| RadrootsAppError::invalid_argument("invalid_event_range"))?;
            }
            if let Some(timezone) = input.event_timezone.as_deref() {
                event = event
                    .with_start_tzid(timezone)
                    .map_err(|_| RadrootsAppError::invalid_argument("invalid_event_timezone"))?;
            }
            if !input.content.is_empty() {
                event = event
                    .with_description(input.content.clone())
                    .map_err(|_| RadrootsAppError::invalid_argument("invalid_event_description"))?;
            }
            if let Some(location) = input.location.clone() {
                event = event
                    .with_locations(vec![location])
                    .map_err(|_| RadrootsAppError::invalid_argument("invalid_event_location"))?;
            }
            if let Some(image) = image {
                event = event
                    .with_image(image.authored_image()?)
                    .map_err(|_| RadrootsAppError::invalid_argument("invalid_event_image"))?;
            }
            Ok(CreateEvent::time(event))
        }
    }
}

fn food_command(
    input: FfiAddDraftInput,
    authored_at_unix_s: u64,
    media: &[BoundPreparedMedia],
) -> Result<CreateFoodAvailability, RadrootsAppError> {
    let unit = FoodUnit::parse(required(input.unit.as_deref(), "food_unit_required")?)
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_food_unit"))?;
    let images = media
        .iter()
        .map(|image| {
            Ok(FoodAvailabilityImage::new(
                image.authored_image()?,
                FoodImageDimensions::new(image.media.width, image.media.height)
                    .map_err(|_| RadrootsAppError::invalid_argument("invalid_image_dimensions"))?,
            ))
        })
        .collect::<Result<Vec<_>, RadrootsAppError>>()?;
    let status = input.food_status.as_deref().unwrap_or("active");
    let details = FoodAvailabilityDetails::new(FoodAvailabilityDetailsParts {
        content: FoodContent::new(input.content)
            .map_err(|_| RadrootsAppError::invalid_argument("invalid_food_content"))?,
        identifier: FoodIdentifier::parse(required(
            input.identifier.as_deref(),
            "food_identifier_required",
        )?)
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_food_identifier"))?,
        title: FoodText::new(required(input.title, "food_title_required")?)
            .map_err(|_| RadrootsAppError::invalid_argument("invalid_food_title"))?,
        summary: FoodText::new(required(input.summary, "food_summary_required")?)
            .map_err(|_| RadrootsAppError::invalid_argument("invalid_food_summary"))?,
        published_at: FoodPublishedAt::new(
            input.food_published_at_unix_s.unwrap_or(authored_at_unix_s),
        )
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_food_published_at"))?,
        location: FoodText::new(required(input.location, "food_location_required")?)
            .map_err(|_| RadrootsAppError::invalid_argument("invalid_food_location"))?,
        price: FoodPrice::new(
            required(input.price_amount, "food_price_required")?,
            FoodCurrency::parse(required(input.currency, "food_currency_required")?)
                .map_err(|_| RadrootsAppError::invalid_argument("invalid_food_currency"))?,
            unit,
        )
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_food_price"))?,
        quantity: input
            .quantity
            .map(|quantity| FoodQuantity::new(quantity, unit))
            .transpose()
            .map_err(|_| RadrootsAppError::invalid_argument("invalid_food_quantity"))?,
        status: FoodAvailabilityStatus::parse(status)
            .map_err(|_| RadrootsAppError::invalid_argument("invalid_food_status"))?,
        images,
    })
    .map_err(|_| RadrootsAppError::invalid_argument("invalid_food_availability"))?;
    Ok(CreateFoodAvailability::new(details))
}

fn reject_media(media: &[BoundPreparedMedia]) -> Result<(), RadrootsAppError> {
    if media.is_empty() {
        Ok(())
    } else {
        Err(RadrootsAppError::invalid_argument("media_not_allowed"))
    }
}

fn content_with_media_references(
    mut content: String,
    media: &[BoundPreparedMedia],
) -> Result<String, RadrootsAppError> {
    if content.trim().is_empty() {
        return Err(RadrootsAppError::invalid_argument("content_required"));
    }
    for item in media {
        let url = item.descriptor.url().as_str();
        match content.match_indices(url).count() {
            0 => {
                if !content.ends_with('\n') {
                    content.push('\n');
                }
                content.push_str(url);
            }
            1 => {}
            _ => {
                return Err(RadrootsAppError::invalid_argument(
                    "duplicate_media_reference",
                ));
            }
        }
    }
    Ok(content)
}

fn required<T>(value: Option<T>, code: &'static str) -> Result<T, RadrootsAppError> {
    value.ok_or_else(|| RadrootsAppError::invalid_argument(code))
}

fn opaque_media_reference_is_valid(value: &str) -> bool {
    value.len() > "media:".len()
        && value.len() <= MEDIA_REFERENCE_MAX_BYTES
        && value.starts_with("media:")
        && value["media:".len()..].bytes().all(|byte| {
            byte.is_ascii_lowercase() || byte.is_ascii_digit() || matches!(byte, b'-' | b'_')
        })
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiMediaStage {
    Pending,
    Preparing,
    Uploading,
    Verified,
    Failed,
    Orphaned,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<Phase1MediaStage> for FfiMediaStage {
    fn from(value: Phase1MediaStage) -> Self {
        match value {
            Phase1MediaStage::Pending => Self::Pending,
            Phase1MediaStage::Preparing => Self::Preparing,
            Phase1MediaStage::Uploading => Self::Uploading,
            Phase1MediaStage::Verified => Self::Verified,
            Phase1MediaStage::Failed => Self::Failed,
            Phase1MediaStage::Orphaned => Self::Orphaned,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiDraftMediaRecord {
    pub schema_version: u16,
    pub url: String,
    pub stage: FfiMediaStage,
    pub upload_attempts: u8,
    pub verified_at_unix_ms: Option<u64>,
    pub possible_orphan: bool,
    pub orphan_reason_code: Option<String>,
    pub orphan_recorded_at_unix_ms: Option<u64>,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<&Phase1MediaPrerequisite> for FfiDraftMediaRecord {
    fn from(value: &Phase1MediaPrerequisite) -> Self {
        let orphan = value.orphan();
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            url: value.url().to_owned(),
            stage: value.stage().into(),
            upload_attempts: value.upload_attempts(),
            verified_at_unix_ms: value.verified_at_unix_ms(),
            possible_orphan: orphan.is_some(),
            orphan_reason_code: orphan.map(|value| value.reason_code().to_owned()),
            orphan_recorded_at_unix_ms: orphan.map(|value| value.recorded_at_unix_ms()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiOutboxState {
    Draft,
    MediaPreparing,
    MediaUploading,
    ReadyToSign,
    Signing,
    Signed,
    Queued,
    Delivering,
    PartiallyDelivered,
    Retryable,
    Terminal,
    Cancelled,
    Complete,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiDraftKind {
    Add,
    Retraction,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<Phase1DraftKind> for FfiDraftKind {
    fn from(value: Phase1DraftKind) -> Self {
        match value {
            Phase1DraftKind::Add => Self::Add,
            Phase1DraftKind::Retraction => Self::Retraction,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiDraftFormMediaRecord {
    pub schema_version: u16,
    pub opaque_reference: String,
    pub url: String,
    pub sha256: String,
    pub media_type: String,
    pub byte_size: u64,
    pub width: u32,
    pub height: u32,
    pub alt: String,
    pub prepared_at_unix_s: u64,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<&Phase1DraftMediaSnapshot> for FfiDraftFormMediaRecord {
    fn from(value: &Phase1DraftMediaSnapshot) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            opaque_reference: value.opaque_reference.clone(),
            url: value.url.clone(),
            sha256: value.sha256.clone(),
            media_type: value.media_type.clone(),
            byte_size: value.byte_size,
            width: value.width,
            height: value.height,
            alt: value.alt.clone(),
            prepared_at_unix_s: value.prepared_at_unix_s,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiDraftFormRecord {
    pub schema_version: u16,
    pub command_type: FfiAddCommandType,
    pub content: String,
    pub identifier: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub location: Option<String>,
    pub event_timing: Option<FfiEventTimingKind>,
    pub event_start_date: Option<String>,
    pub event_end_date: Option<String>,
    pub event_start_unix_s: Option<u64>,
    pub event_end_unix_s: Option<u64>,
    pub event_timezone: Option<String>,
    pub price_amount: Option<String>,
    pub currency: Option<String>,
    pub unit: Option<String>,
    pub quantity: Option<String>,
    pub food_published_at_unix_s: Option<u64>,
    pub food_status: Option<String>,
    pub media: Vec<FfiDraftFormMediaRecord>,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<&Phase1DraftFormSnapshot> for FfiDraftFormRecord {
    fn from(value: &Phase1DraftFormSnapshot) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            command_type: value.command_type.into(),
            content: value.content.clone(),
            identifier: value.identifier.clone(),
            title: value.title.clone(),
            summary: value.summary.clone(),
            location: value.location.clone(),
            event_timing: value.event_timing.map(|value| match value {
                Phase1DraftEventTiming::AllDay => FfiEventTimingKind::AllDay,
                Phase1DraftEventTiming::Timed => FfiEventTimingKind::Timed,
            }),
            event_start_date: value.event_start_date.clone(),
            event_end_date: value.event_end_date.clone(),
            event_start_unix_s: value.event_start_unix_s,
            event_end_unix_s: value.event_end_unix_s,
            event_timezone: value.event_timezone.clone(),
            price_amount: value.price_amount.clone(),
            currency: value.currency.clone(),
            unit: value.unit.clone(),
            quantity: value.quantity.clone(),
            food_published_at_unix_s: value.food_published_at_unix_s,
            food_status: value.food_status.clone(),
            media: value.media.iter().map(Into::into).collect(),
        }
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<Phase1OutboxState> for FfiOutboxState {
    fn from(value: Phase1OutboxState) -> Self {
        match value {
            Phase1OutboxState::Draft => Self::Draft,
            Phase1OutboxState::MediaPreparing => Self::MediaPreparing,
            Phase1OutboxState::MediaUploading => Self::MediaUploading,
            Phase1OutboxState::ReadyToSign => Self::ReadyToSign,
            Phase1OutboxState::Signing => Self::Signing,
            Phase1OutboxState::Signed => Self::Signed,
            Phase1OutboxState::Queued => Self::Queued,
            Phase1OutboxState::Delivering => Self::Delivering,
            Phase1OutboxState::PartiallyDelivered => Self::PartiallyDelivered,
            Phase1OutboxState::Retryable => Self::Retryable,
            Phase1OutboxState::Terminal => Self::Terminal,
            Phase1OutboxState::Cancelled => Self::Cancelled,
            Phase1OutboxState::Complete => Self::Complete,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiDraftStatusRecord {
    pub schema_version: u16,
    pub draft_id: String,
    pub revision: u64,
    pub author_public_key: String,
    pub kind: FfiDraftKind,
    pub command_type: FfiAddCommandType,
    pub form: Option<FfiDraftFormRecord>,
    pub state: FfiOutboxState,
    pub card_id: String,
    pub operation_id: Option<String>,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub media: Vec<FfiDraftMediaRecord>,
    pub settlement: Option<FfiOperationSettlementRecord>,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<Phase1DraftStatus> for FfiDraftStatusRecord {
    fn from(value: Phase1DraftStatus) -> Self {
        let draft = value.draft();
        let settlement = value.push().map(|push| push.settlement());
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            draft_id: hex::encode(draft.draft_id().as_bytes()),
            revision: draft.revision().get(),
            author_public_key: hex::encode(draft.author()),
            kind: value.kind().into(),
            command_type: value.command_type().into(),
            form: value.form().map(Into::into),
            state: value.state().into(),
            card_id: value.card_id().to_hex(),
            operation_id: draft.operation_id().map(|id| hex::encode(id.as_bytes())),
            created_at_unix_ms: draft.created_at_unix_ms(),
            updated_at_unix_ms: draft.updated_at_unix_ms(),
            media: value.media().iter().map(Into::into).collect(),
            settlement: settlement.map(Into::into),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiOperationSettlementRecord {
    pub schema_version: u16,
    pub artifacts: u16,
    pub signed: u16,
    pub admitted: u16,
    pub pending: u16,
    pub retryable: u16,
    pub indeterminate: u16,
    pub failed_terminal: u16,
    pub cancelled: u16,
    pub delivery_plans: u16,
    pub delivery_satisfied: u16,
    pub delivery_pending: u16,
    pub delivery_retryable: u16,
    pub delivery_exhausted: u16,
    pub delivery_failed_terminal: u16,
    pub delivery_cancelled: u16,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<radroots_storage::authored::OperationSettlement> for FfiOperationSettlementRecord {
    fn from(value: radroots_storage::authored::OperationSettlement) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            artifacts: value.artifacts(),
            signed: value.signed(),
            admitted: value.admitted(),
            pending: value.pending(),
            retryable: value.retryable(),
            indeterminate: value.indeterminate(),
            failed_terminal: value.failed_terminal(),
            cancelled: value.cancelled(),
            delivery_plans: value.delivery_plans(),
            delivery_satisfied: value.delivery_satisfied(),
            delivery_pending: value.delivery_pending(),
            delivery_retryable: value.delivery_retryable(),
            delivery_exhausted: value.delivery_exhausted(),
            delivery_failed_terminal: value.delivery_failed_terminal(),
            delivery_cancelled: value.delivery_cancelled(),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRelaySatisfaction {
    AnyAccepted,
    AllAccepted,
    AnyDelivered,
    AllDelivered,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiCancellationPolicy {
    PreservePublishedRequest,
    LocalCooperative,
}

impl FfiCancellationPolicy {
    pub(crate) const fn core(self) -> Phase1CancellationPolicy {
        match self {
            Self::PreservePublishedRequest => Phase1CancellationPolicy::PreservePublishedRequest,
            Self::LocalCooperative => Phase1CancellationPolicy::LocalCooperative,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiQueuePolicyRecord {
    pub schema_version: u16,
    pub relay_urls: Vec<String>,
    pub satisfaction: FfiRelaySatisfaction,
    pub delivery_deadline_unix_ms: u64,
    pub cancellation: FfiCancellationPolicy,
}

impl TryFrom<FfiQueuePolicyRecord> for Phase1QueuePolicy {
    type Error = RadrootsAppError;

    fn try_from(value: FfiQueuePolicyRecord) -> Result<Self, Self::Error> {
        require_schema(value.schema_version)?;
        Phase1QueuePolicy::new(
            value.relay_urls,
            match value.satisfaction {
                FfiRelaySatisfaction::AnyAccepted => Phase1RelaySatisfaction::AnyAccepted,
                FfiRelaySatisfaction::AllAccepted => Phase1RelaySatisfaction::AllAccepted,
                FfiRelaySatisfaction::AnyDelivered => Phase1RelaySatisfaction::AnyDelivered,
                FfiRelaySatisfaction::AllDelivered => Phase1RelaySatisfaction::AllDelivered,
            },
            value.delivery_deadline_unix_ms,
            match value.cancellation {
                FfiCancellationPolicy::PreservePublishedRequest => {
                    Phase1CancellationPolicy::PreservePublishedRequest
                }
                FfiCancellationPolicy::LocalCooperative => {
                    Phase1CancellationPolicy::LocalCooperative
                }
            },
        )
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_queue_policy"))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiCapabilityRecord {
    pub schema_version: u16,
    pub id: String,
    pub compiled: bool,
    pub configured: bool,
    pub availability: String,
    pub maturity: String,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<SdkCapabilityRecord> for FfiCapabilityRecord {
    fn from(value: SdkCapabilityRecord) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            id: value.id,
            compiled: value.compiled,
            configured: value.configured,
            availability: value.availability,
            maturity: value.maturity,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiStorageStatusRecord {
    pub schema_version: u16,
    pub backend: String,
    pub open_mode: String,
    pub shutdown: String,
    pub integrity: String,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<SdkStorageStatusRecord> for FfiStorageStatusRecord {
    fn from(value: SdkStorageStatusRecord) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            backend: value.backend,
            open_mode: value.open_mode,
            shutdown: value.shutdown,
            integrity: value.integrity,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRelayStatusRecord {
    pub schema_version: u16,
    pub relay_url: String,
    pub access: String,
    pub read_state: String,
    pub write_state: String,
    pub read_last_attempt_unix_ms: Option<u64>,
    pub write_last_attempt_unix_ms: Option<u64>,
    pub read_next_attempt_unix_ms: Option<u64>,
    pub write_next_attempt_unix_ms: Option<u64>,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<SdkRelayStatusRecord> for FfiRelayStatusRecord {
    fn from(value: SdkRelayStatusRecord) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            relay_url: value.relay_url,
            access: value.access,
            read_state: value.read_state,
            write_state: value.write_state,
            read_last_attempt_unix_ms: value.read_last_attempt_unix_ms,
            write_last_attempt_unix_ms: value.write_last_attempt_unix_ms,
            read_next_attempt_unix_ms: value.read_next_attempt_unix_ms,
            write_next_attempt_unix_ms: value.write_next_attempt_unix_ms,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRelayStatusReportRecord {
    pub schema_version: u16,
    pub profile: String,
    pub state: String,
    pub read_availability: String,
    pub write_availability: String,
    pub relays: Vec<FfiRelayStatusRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiBlossomHostKind {
    Native,
    Simulator,
    PhysicalDevice,
}

impl From<FfiBlossomHostKind> for radroots_sdk::transport::BlossomHostKind {
    fn from(value: FfiBlossomHostKind) -> Self {
        match value {
            FfiBlossomHostKind::Native => Self::Native,
            FfiBlossomHostKind::Simulator => Self::Simulator,
            FfiBlossomHostKind::PhysicalDevice => Self::PhysicalDevice,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiBlossomEndpointAuthority {
    PublicWebPki,
    LoopbackDevelopment,
    PrivateNetworkDevelopment,
}

impl From<FfiBlossomEndpointAuthority> for radroots_sdk::transport::BlossomEndpointAuthority {
    fn from(value: FfiBlossomEndpointAuthority) -> Self {
        match value {
            FfiBlossomEndpointAuthority::PublicWebPki => Self::PublicWebPki,
            FfiBlossomEndpointAuthority::LoopbackDevelopment => Self::LoopbackDevelopment,
            FfiBlossomEndpointAuthority::PrivateNetworkDevelopment => {
                Self::PrivateNetworkDevelopment
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiBlossomConfigurationRecord {
    pub schema_version: u16,
    pub host_kind: String,
    pub endpoint_authority: String,
    pub primary_origin: String,
    pub fallback_origins: Vec<String>,
    pub config_fingerprint: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiBlossomEvidenceRecord {
    pub schema_version: u16,
    pub origin: String,
    pub config_fingerprint: String,
    pub state: String,
    pub last_successful_state: String,
    pub transport_security: String,
    pub observed_at_unix_ms: Option<u64>,
    pub http_status: Option<u16>,
    pub error_code: Option<String>,
    pub server_error_code: Option<String>,
    pub error_phase: Option<String>,
    pub retryable: bool,
    pub possible_orphan: bool,
    pub attempts: u8,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<SdkBlossomConfigurationRecord> for FfiBlossomConfigurationRecord {
    fn from(value: SdkBlossomConfigurationRecord) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            host_kind: value.host_kind,
            endpoint_authority: value.endpoint_authority,
            primary_origin: value.primary_origin,
            fallback_origins: value.fallback_origins,
            config_fingerprint: value.config_fingerprint,
        }
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<SdkBlossomEvidenceRecord> for FfiBlossomEvidenceRecord {
    fn from(value: SdkBlossomEvidenceRecord) -> Self {
        Self {
            schema_version: value.schema_version,
            origin: value.origin,
            config_fingerprint: value.config_fingerprint,
            state: value.state,
            last_successful_state: value.last_successful_state,
            transport_security: value.transport_security,
            observed_at_unix_ms: value.observed_at_unix_ms,
            http_status: value.http_status,
            error_code: value.error_code,
            server_error_code: value.server_error_code,
            error_phase: value.error_phase,
            retryable: value.retryable,
            possible_orphan: value.possible_orphan,
            attempts: value.attempts,
        }
    }
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<SdkRelayStatusReportRecord> for FfiRelayStatusReportRecord {
    fn from(value: SdkRelayStatusReportRecord) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            profile: value.profile,
            state: value.state,
            read_availability: value.read_availability,
            write_availability: value.write_availability,
            relays: value.relays.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiShutdownRecord {
    pub schema_version: u16,
    pub state: String,
    pub already_closed: bool,
}

#[cfg_attr(coverage_nightly, coverage(off))]
impl From<SdkShutdownRecord> for FfiShutdownRecord {
    fn from(value: SdkShutdownRecord) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            state: value.state,
            already_closed: value.already_closed,
        }
    }
}

pub(crate) fn decode_id(value: &str, code: &'static str) -> Result<[u8; 16], RadrootsAppError> {
    if value.len() != 32 {
        return Err(RadrootsAppError::invalid_argument(code));
    }
    let bytes = hex::decode(value).map_err(|_| RadrootsAppError::invalid_argument(code))?;
    bytes
        .try_into()
        .map_err(|_| RadrootsAppError::invalid_argument(code))
}

fn require_schema(schema_version: u16) -> Result<(), RadrootsAppError> {
    if schema_version == MOBILE_FFI_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(RadrootsAppError::invalid_argument(
            "unsupported_schema_version",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    use std::os::fd::AsRawFd;

    use super::*;

    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    fn blossom_slot() -> radroots_sdk::transport::BlossomSlot {
        let profile = radroots_sdk::transport::BlossomProfile::new(
            radroots_sdk::transport::BlossomHostKind::Simulator,
            radroots_sdk::transport::BlossomEndpointAuthority::LoopbackDevelopment,
            "http://127.0.0.1:3000",
            std::iter::empty::<&str>(),
        )
        .unwrap();
        let slot = radroots_sdk::transport::BlossomSlot::new();
        slot.configure(radroots_sdk::transport::BlossomConfig::from_profile(
            profile,
        ))
        .unwrap();
        slot
    }

    fn photo_input(file_descriptor: u64, bytes: &[u8], digest: String) -> FfiAddDraftInput {
        FfiAddDraftInput {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            command_type: FfiAddCommandType::CreatePhotoUpdate,
            content: "Fresh carrots from this morning.".to_owned(),
            identifier: None,
            title: None,
            summary: None,
            location: None,
            event_timing: None,
            event_start_date: None,
            event_end_date: None,
            event_start_unix_s: None,
            event_end_unix_s: None,
            event_timezone: None,
            price_amount: None,
            currency: None,
            unit: None,
            quantity: None,
            food_published_at_unix_s: None,
            food_status: None,
            media: vec![FfiPreparedMediaInput {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                opaque_reference: "media:carrots-01".to_owned(),
                file_descriptor,
                sha256: digest,
                media_type: "image/png".to_owned(),
                byte_size: bytes.len() as u64,
                width: 2,
                height: 2,
                alt: "A basket of carrots".to_owned(),
                prepared_at_unix_s: 1_800_000_000,
            }],
        }
    }

    fn text_input(command_type: FfiAddCommandType) -> FfiAddDraftInput {
        FfiAddDraftInput {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            command_type,
            content: "Fresh from the farm".to_owned(),
            identifier: None,
            title: None,
            summary: None,
            location: None,
            event_timing: None,
            event_start_date: None,
            event_end_date: None,
            event_start_unix_s: None,
            event_end_unix_s: None,
            event_timezone: None,
            price_amount: None,
            currency: None,
            unit: None,
            quantity: None,
            food_published_at_unix_s: None,
            food_status: None,
            media: Vec::new(),
        }
    }

    #[test]
    fn exact_five_add_inputs_build_their_typed_core_commands() {
        let (update, media) = text_input(FfiAddCommandType::CreateUpdate)
            .command_and_media(1_800_000_000, None)
            .expect("update");
        assert!(matches!(update, Phase1AddCommand::CreateUpdate(_)));
        assert!(media.is_empty());

        let (ask, media) = text_input(FfiAddCommandType::CreateAsk)
            .command_and_media(1_800_000_000, None)
            .expect("ask");
        assert!(matches!(ask, Phase1AddCommand::CreateAsk(_)));
        assert!(media.is_empty());

        let mut all_day = text_input(FfiAddCommandType::CreateEvent);
        all_day.identifier = Some("market-day".to_owned());
        all_day.title = Some("Farmers market".to_owned());
        all_day.location = Some("Town square".to_owned());
        all_day.event_timing = Some(FfiEventTimingKind::AllDay);
        all_day.event_start_date = Some("2026-08-08".to_owned());
        all_day.event_end_date = Some("2026-08-09".to_owned());
        let (event, media) = all_day
            .command_and_media(1_800_000_000, None)
            .expect("all-day event");
        assert!(matches!(event, Phase1AddCommand::CreateEvent(_)));
        assert!(media.is_empty());

        let mut timed = text_input(FfiAddCommandType::CreateEvent);
        timed.identifier = Some("harvest-tour".to_owned());
        timed.title = Some("Harvest tour".to_owned());
        timed.event_timing = Some(FfiEventTimingKind::Timed);
        timed.event_start_unix_s = Some(1_800_000_000);
        timed.event_end_unix_s = Some(1_800_003_600);
        timed.event_timezone = Some("America/Vancouver".to_owned());
        let (event, media) = timed
            .command_and_media(1_800_000_000, None)
            .expect("timed event");
        assert!(matches!(event, Phase1AddCommand::CreateEvent(_)));
        assert!(media.is_empty());

        let bytes = png(2, 2);
        let mut file = tempfile::NamedTempFile::new().expect("media file");
        file.write_all(&bytes).expect("write media");
        file.flush().expect("flush media");
        let digest = Sha256::digest(&bytes).to_hex();
        let mut food = photo_input(file.as_file().as_raw_fd() as u64, &bytes, digest);
        food.command_type = FfiAddCommandType::CreateFoodAvailability;
        food.identifier = Some("carrots-2026-08".to_owned());
        food.title = Some("Carrots".to_owned());
        food.summary = Some("Fresh bunches".to_owned());
        food.location = Some("Victoria".to_owned());
        food.price_amount = Some("4.5".to_owned());
        food.currency = Some("CAD".to_owned());
        food.unit = Some("bunch".to_owned());
        food.quantity = Some("12".to_owned());
        food.food_status = Some("active".to_owned());
        let blossom = blossom_slot();
        let (food, media) = food
            .command_and_media(1_800_000_000, Some(&blossom))
            .expect("food availability");
        assert!(matches!(food, Phase1AddCommand::CreateFoodAvailability(_)));
        assert_eq!(media.len(), 1);
    }

    #[test]
    fn add_validation_reports_schema_shape_media_and_required_field_failures() {
        let mut wrong_schema = text_input(FfiAddCommandType::CreateUpdate);
        wrong_schema.schema_version = MOBILE_FFI_SCHEMA_VERSION + 1;
        assert_eq!(
            wrong_schema
                .command_and_media(1_800_000_000, None)
                .expect_err("schema")
                .report()
                .code,
            "unsupported_schema_version"
        );
        assert_eq!(
            text_input(FfiAddCommandType::CreateUpdate)
                .command_and_media(0, None)
                .expect_err("authored time")
                .report()
                .code,
            "invalid_add_draft"
        );
        assert_eq!(
            text_input(FfiAddCommandType::CreateEvent)
                .command_and_media(1_800_000_000, None)
                .expect_err("event identity")
                .report()
                .code,
            "event_identifier_required"
        );
        let mut food = text_input(FfiAddCommandType::CreateFoodAvailability);
        food.unit = Some("crate".to_owned());
        assert_eq!(
            food.command_and_media(1_800_000_000, None)
                .expect_err("food unit")
                .report()
                .code,
            "invalid_food_unit"
        );
    }

    #[test]
    fn prepared_media_accepts_only_the_exact_bounded_file_descriptor_bytes() {
        let bytes = png(2, 2);
        let mut file = tempfile::NamedTempFile::new().expect("media file");
        file.write_all(&bytes).expect("write media");
        file.flush().expect("flush media");
        let digest = Sha256::digest(&bytes).to_hex();
        let input = photo_input(file.as_file().as_raw_fd() as u64, &bytes, digest.clone());
        let blossom = blossom_slot();

        let (command, media) = input
            .command_and_media(1_800_000_000, Some(&blossom))
            .expect("verified media input");
        assert!(matches!(command, Phase1AddCommand::CreatePhotoUpdate(_)));
        assert_eq!(media.len(), 1);
        assert_eq!(
            media[0].url(),
            format!("http://127.0.0.1:3000/{digest}.png")
        );
        assert_eq!(
            file.as_file().metadata().expect("caller-owned media").len(),
            bytes.len() as u64
        );
    }

    #[test]
    fn prepared_media_rejects_file_descriptors_outside_the_platform_range() {
        let bytes = png(2, 2);
        let blossom = blossom_slot();
        let input = photo_input(u64::MAX, &bytes, Sha256::digest(&bytes).to_hex());

        assert_eq!(
            input
                .command_and_media(1_800_000_000, Some(&blossom))
                .expect_err("out-of-range descriptor")
                .report()
                .code,
            "media_handle_unavailable"
        );
    }

    #[cfg(unix)]
    #[test]
    fn prepared_media_rejects_unavailable_in_range_file_descriptors() {
        let bytes = png(2, 2);
        let blossom = blossom_slot();
        let input = photo_input(i32::MAX as u64, &bytes, Sha256::digest(&bytes).to_hex());

        assert_eq!(
            input
                .command_and_media(1_800_000_000, Some(&blossom))
                .expect_err("unavailable in-range descriptor")
                .report()
                .code,
            "media_handle_unavailable"
        );
    }

    #[test]
    fn prepared_media_rejects_digest_tamper_and_path_like_references() {
        let bytes = png(2, 2);
        let mut file = tempfile::NamedTempFile::new().expect("media file");
        file.write_all(&bytes).expect("write media");
        file.flush().expect("flush media");

        let tampered = photo_input(
            file.as_file().as_raw_fd() as u64,
            &bytes,
            Sha256::digest(b"other").to_hex(),
        );
        let blossom = blossom_slot();
        assert_eq!(
            tampered
                .command_and_media(1_800_000_000, Some(&blossom))
                .expect_err("digest mismatch")
                .report()
                .code,
            "media_verification_failed"
        );

        let mut path_like = photo_input(
            file.as_file().as_raw_fd() as u64,
            &bytes,
            Sha256::digest(&bytes).to_hex(),
        );
        path_like.media[0].opaque_reference = "file:/private/media.jpg".to_owned();
        assert_eq!(
            path_like
                .command_and_media(1_800_000_000, Some(&blossom))
                .expect_err("path-like reference")
                .report()
                .code,
            "invalid_media_reference"
        );
    }

    #[test]
    fn prepared_media_constructs_a_bounded_verified_upload_request() {
        let bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR\0\0\0\x02\0\0\0\x02";
        let mut file = tempfile::NamedTempFile::new().expect("media file");
        file.write_all(bytes).expect("write media");
        file.flush().expect("flush media");
        let digest = Sha256::digest(bytes).to_hex();
        let mut input = photo_input(file.as_file().as_raw_fd() as u64, bytes, digest.clone());
        input.media[0].media_type = "image/png".to_owned();

        let prepared = PreparedMedia::try_from(input.media.remove(0)).expect("prepared media");
        let request = prepared
            .upload_request(1_800_000_000_000)
            .expect("bounded upload request");
        assert_eq!(request.sha256().to_hex(), digest);
        assert_eq!(request.byte_size(), bytes.len() as u64);
        assert_eq!(request.media_type().as_str(), "image/png");
        assert_eq!(request.dimensions().width(), 2);
        assert_eq!(request.dimensions().height(), 2);
    }

    #[test]
    fn media_validation_executes_every_bounded_shape_guard() {
        let bytes = png(2, 2);
        let mut file = tempfile::NamedTempFile::new().expect("media file");
        file.write_all(&bytes).expect("write media");
        file.flush().expect("flush media");
        let digest = Sha256::digest(&bytes).to_hex();
        let valid = photo_input(file.as_file().as_raw_fd() as u64, &bytes, digest).media[0].clone();

        let mut invalid_values = Vec::new();
        let mut value = valid.clone();
        value.byte_size = 0;
        invalid_values.push(value);
        let mut value = valid.clone();
        value.byte_size = MEDIA_FILE_MAX_BYTES + 1;
        invalid_values.push(value);
        let mut value = valid.clone();
        value.width = 0;
        invalid_values.push(value);
        let mut value = valid.clone();
        value.height = 0;
        invalid_values.push(value);
        let mut value = valid.clone();
        value.prepared_at_unix_s = 0;
        invalid_values.push(value);
        let mut value = valid.clone();
        value.alt = "   ".to_owned();
        invalid_values.push(value);
        let mut value = valid.clone();
        value.alt = "a".repeat(1_025);
        invalid_values.push(value);

        for value in invalid_values {
            assert_eq!(
                PreparedMedia::try_from(value)
                    .err()
                    .expect("invalid bounded media")
                    .report()
                    .code,
                "invalid_media_reference"
            );
        }

        let mut wrong_size = valid.clone();
        wrong_size.byte_size += 1;
        assert_eq!(
            PreparedMedia::try_from(wrong_size)
                .err()
                .expect("descriptor size")
                .report()
                .code,
            "media_size_mismatch"
        );
        for reference in ["media:", "Media:item", "media:BAD", "media:item/path"] {
            assert!(!opaque_media_reference_is_valid(reference));
        }
        assert!(opaque_media_reference_is_valid("media:item_01-a"));
        assert!(!opaque_media_reference_is_valid(&format!(
            "media:{}",
            "a".repeat(MEDIA_REFERENCE_MAX_BYTES)
        )));
    }

    #[test]
    fn event_and_post_optional_branches_remain_strict_and_complete() {
        let mut minimal_date = text_input(FfiAddCommandType::CreateEvent);
        minimal_date.content.clear();
        minimal_date.identifier = Some("minimal-date".to_owned());
        minimal_date.title = Some("Minimal date".to_owned());
        minimal_date.event_timing = Some(FfiEventTimingKind::AllDay);
        minimal_date.event_start_date = Some("2026-08-08".to_owned());
        assert!(minimal_date.command_and_media(1_800_000_000, None).is_ok());

        let mut minimal_time = text_input(FfiAddCommandType::CreateEvent);
        minimal_time.content.clear();
        minimal_time.identifier = Some("minimal-time".to_owned());
        minimal_time.title = Some("Minimal time".to_owned());
        minimal_time.event_timing = Some(FfiEventTimingKind::Timed);
        minimal_time.event_start_unix_s = Some(1_800_000_000);
        assert!(minimal_time.command_and_media(1_800_000_000, None).is_ok());

        let bytes = png(2, 2);
        let mut file = tempfile::NamedTempFile::new().expect("media file");
        file.write_all(&bytes).expect("write media");
        file.flush().expect("flush media");
        let digest = Sha256::digest(&bytes).to_hex();
        let mut event = photo_input(file.as_file().as_raw_fd() as u64, &bytes, digest);
        event.command_type = FfiAddCommandType::CreateEvent;
        event.identifier = Some("event-image".to_owned());
        event.title = Some("Event image".to_owned());
        event.event_timing = Some(FfiEventTimingKind::Timed);
        event.event_start_unix_s = Some(1_800_000_000);
        let blossom = blossom_slot();
        assert!(
            event
                .clone()
                .command_and_media(1_800_000_000, Some(&blossom))
                .is_ok()
        );

        let mut second = event.media[0].clone();
        second.opaque_reference = "media:event-image-two".to_owned();
        event.media.push(second);
        assert_eq!(
            event
                .command_and_media(1_800_000_000, Some(&blossom))
                .expect_err("event media limit")
                .report()
                .code,
            "event_image_limit"
        );
    }

    #[test]
    fn content_references_and_identifier_decoding_cover_all_outcomes() {
        let bytes = png(2, 2);
        let mut file = tempfile::NamedTempFile::new().expect("media file");
        file.write_all(&bytes).expect("write media");
        file.flush().expect("flush media");
        let digest = Sha256::digest(&bytes).to_hex();
        let input = photo_input(file.as_file().as_raw_fd() as u64, &bytes, digest);
        let blossom = blossom_slot();
        let prepared = PreparedMedia::try_from(input.media[0].clone())
            .expect("prepared media")
            .bind(&blossom)
            .expect("bound media");
        let url = prepared.descriptor.url().as_str().to_owned();

        assert_eq!(
            content_with_media_references("   ".to_owned(), std::slice::from_ref(&prepared))
                .expect_err("blank content")
                .report()
                .code,
            "content_required"
        );
        assert_eq!(
            content_with_media_references(
                format!("caption\n{url}"),
                std::slice::from_ref(&prepared)
            )
            .expect("one existing reference")
            .match_indices(&url)
            .count(),
            1
        );
        assert!(
            content_with_media_references("caption\n".to_owned(), std::slice::from_ref(&prepared))
                .expect("newline append")
                .ends_with(&url)
        );
        assert_eq!(
            content_with_media_references(
                format!("{url}\n{url}"),
                std::slice::from_ref(&prepared),
            )
            .expect_err("duplicate reference")
            .report()
            .code,
            "duplicate_media_reference"
        );

        let mut update = text_input(FfiAddCommandType::CreateUpdate);
        update.media.push(input.media[0].clone());
        assert_eq!(
            update
                .command_and_media(1_800_000_000, Some(&blossom))
                .expect_err("update media")
                .report()
                .code,
            "media_not_allowed"
        );
        let mut over_limit = text_input(FfiAddCommandType::CreateUpdate);
        over_limit.media = vec![input.media[0].clone(); 21];
        assert_eq!(
            over_limit
                .command_and_media(1_800_000_000, None)
                .expect_err("media count")
                .report()
                .code,
            "invalid_add_draft"
        );

        assert_eq!(decode_id(&"01".repeat(16), "bad").expect("id"), [1; 16]);
        assert_eq!(
            decode_id("01", "bad").expect_err("short id").report().code,
            "bad"
        );
        assert_eq!(
            decode_id(&"gg".repeat(16), "bad")
                .expect_err("non-hex id")
                .report()
                .code,
            "bad"
        );
    }
}
