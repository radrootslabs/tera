//! Versioned, secret-safe mobile identity and product configuration policy.

use std::collections::BTreeSet;

use radroots_event::{
    media::AuthoredImage,
    profile::{AuthoredProfile, Nip05Identifier},
};
use radroots_identity::PublicKey;
use radroots_storage::projection::{
    ProjectionDocument, ProjectionGeneration, ProjectionId, ProjectionStore,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::super::RadrootsRuntime;

pub use radroots_sdk::transport::DEFAULT_PUBLIC_RELAY;

pub const MOBILE_SETTINGS_SCHEMA_VERSION: u16 = 1;
pub const DEFAULT_PUBLIC_BLOSSOM_ORIGIN: &str = "https://blossom.radroots.org";
pub const DEFAULT_SIMULATOR_RELAY: &str = "ws://127.0.0.1:21000";
pub const DEFAULT_SIMULATOR_BLOSSOM_ORIGIN: &str = "http://127.0.0.1:21100";

const SETTINGS_PROJECTION_ID: &str = "radroots.mobile.settings.v1";
const SETTINGS_DOCUMENT_KEY: &str = "settings.current";
const SETTINGS_GENERATION_DOMAIN: &[u8] = b"radroots.mobile.settings.generation.v1";
const IDENTITY_ID_MAX_BYTES: usize = 128;
const OPERATION_ID_MAX_BYTES: usize = 128;
const PROFILE_NAME_MAX_BYTES: usize = 256;
const PROFILE_DISPLAY_NAME_MAX_BYTES: usize = 512;
const PROFILE_ABOUT_MAX_BYTES: usize = 8 * 1024;
const RELAY_ENDPOINT_MAX: usize = 32;
const BLOSSOM_FALLBACK_MAX: usize = 15;
const MEDIA_CACHE_MIN_BYTES: u64 = 16 * 1024 * 1024;
const MEDIA_CACHE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const MEDIA_CACHE_MAX_ARTIFACTS: u32 = 10_000;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentityLockState {
    Locked,
    Unlocked,
}

/// Secret-safe reference to one Apple-custodied identity.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityRecord {
    id: String,
    public_key_hex: String,
}

impl IdentityRecord {
    pub fn new(id: impl Into<String>, public_key_hex: &str) -> Result<Self, IdentityError> {
        let id = id.into();
        validate_identifier(&id, IDENTITY_ID_MAX_BYTES)
            .then_some(())
            .ok_or(IdentityError::InvalidIdentityId)?;
        let public_key =
            PublicKey::from_hex(public_key_hex).map_err(|_| IdentityError::InvalidPublicKey)?;
        Ok(Self {
            id,
            public_key_hex: public_key.to_hex(),
        })
    }

    pub fn id(&self) -> &str {
        self.id.as_str()
    }

    pub fn public_key_hex(&self) -> &str {
        self.public_key_hex.as_str()
    }
}

/// Durable identity selection. It contains public metadata only; key material
/// and user-presence state remain in the native custody provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct IdentityState {
    identities: Vec<IdentityRecord>,
    active_identity_id: Option<String>,
    lock_state: IdentityLockState,
    pending_import_operation_id: Option<String>,
}

impl Default for IdentityState {
    fn default() -> Self {
        Self {
            identities: Vec::new(),
            active_identity_id: None,
            lock_state: IdentityLockState::Locked,
            pending_import_operation_id: None,
        }
    }
}

impl IdentityState {
    pub fn new(
        identities: Vec<IdentityRecord>,
        active_identity_id: Option<String>,
        lock_state: IdentityLockState,
        pending_import_operation_id: Option<String>,
    ) -> Result<Self, IdentityError> {
        let mut ids = BTreeSet::new();
        let mut public_keys = BTreeSet::new();
        for identity in &identities {
            if !ids.insert(identity.id()) {
                return Err(IdentityError::DuplicateIdentityId);
            }
            if !public_keys.insert(identity.public_key_hex()) {
                return Err(IdentityError::DuplicatePublicKey);
            }
        }
        if let Some(active) = active_identity_id.as_deref()
            && !ids.contains(active)
        {
            return Err(IdentityError::UnknownIdentity);
        }
        if let Some(operation_id) = pending_import_operation_id.as_deref()
            && !validate_identifier(operation_id, OPERATION_ID_MAX_BYTES)
        {
            return Err(IdentityError::InvalidOperationId);
        }
        if active_identity_id.is_none() && lock_state == IdentityLockState::Unlocked {
            return Err(IdentityError::NoActiveIdentity);
        }
        Ok(Self {
            identities,
            active_identity_id,
            lock_state,
            pending_import_operation_id,
        })
    }

    pub fn identities(&self) -> &[IdentityRecord] {
        self.identities.as_slice()
    }

    pub fn active_identity_id(&self) -> Option<&str> {
        self.active_identity_id.as_deref()
    }

    pub const fn lock_state(&self) -> IdentityLockState {
        self.lock_state
    }

    pub fn pending_import_operation_id(&self) -> Option<&str> {
        self.pending_import_operation_id.as_deref()
    }

    /// Applies a secret-free identity state transition after the native host
    /// has completed any required Keychain or user-presence operation.
    pub fn apply(&self, command: IdentityCommand) -> Result<Self, IdentityError> {
        let mut next = self.clone();
        match command {
            IdentityCommand::BeginImport { operation_id } => {
                if next.pending_import_operation_id.is_some() {
                    return Err(IdentityError::ImportAlreadyPending);
                }
                if !validate_identifier(&operation_id, OPERATION_ID_MAX_BYTES) {
                    return Err(IdentityError::InvalidOperationId);
                }
                next.pending_import_operation_id = Some(operation_id);
            }
            IdentityCommand::CompleteImport {
                operation_id,
                identity,
            } => {
                if next.pending_import_operation_id.as_deref() != Some(operation_id.as_str()) {
                    return Err(IdentityError::ImportOperationMismatch);
                }
                if next
                    .identities
                    .iter()
                    .any(|value| value.id() == identity.id())
                {
                    return Err(IdentityError::DuplicateIdentityId);
                }
                if next
                    .identities
                    .iter()
                    .any(|value| value.public_key_hex() == identity.public_key_hex())
                {
                    return Err(IdentityError::DuplicatePublicKey);
                }
                next.active_identity_id = Some(identity.id().to_owned());
                next.identities.push(identity);
                next.lock_state = IdentityLockState::Locked;
                next.pending_import_operation_id = None;
            }
            IdentityCommand::CancelImport { operation_id } => {
                if next.pending_import_operation_id.as_deref() != Some(operation_id.as_str()) {
                    return Err(IdentityError::ImportOperationMismatch);
                }
                next.pending_import_operation_id = None;
            }
            IdentityCommand::Select { identity_id } => {
                if !next
                    .identities
                    .iter()
                    .any(|identity| identity.id() == identity_id)
                {
                    return Err(IdentityError::UnknownIdentity);
                }
                next.active_identity_id = Some(identity_id);
                next.lock_state = IdentityLockState::Locked;
            }
            IdentityCommand::Lock => {
                if next.active_identity_id.is_none() {
                    return Err(IdentityError::NoActiveIdentity);
                }
                next.lock_state = IdentityLockState::Locked;
            }
            IdentityCommand::Unlock => {
                if next.active_identity_id.is_none() {
                    return Err(IdentityError::NoActiveIdentity);
                }
                next.lock_state = IdentityLockState::Unlocked;
            }
            IdentityCommand::Recover => {
                if next.active_identity_id.is_none() {
                    return Err(IdentityError::NoActiveIdentity);
                }
                next.lock_state = IdentityLockState::Locked;
                next.pending_import_operation_id = None;
            }
        }
        Ok(next)
    }
}

/// Secret-free intent/result commands. `CompleteImport` carries only the
/// public identity returned by the Apple custody provider.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum IdentityCommand {
    BeginImport {
        operation_id: String,
    },
    CompleteImport {
        operation_id: String,
        identity: IdentityRecord,
    },
    CancelImport {
        operation_id: String,
    },
    Select {
        identity_id: String,
    },
    Lock,
    Unlock,
    Recover,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum IdentityError {
    #[error("identity id is invalid")]
    InvalidIdentityId,
    #[error("identity public key is invalid")]
    InvalidPublicKey,
    #[error("identity operation id is invalid")]
    InvalidOperationId,
    #[error("identity id is duplicated")]
    DuplicateIdentityId,
    #[error("identity public key is duplicated")]
    DuplicatePublicKey,
    #[error("identity is unknown")]
    UnknownIdentity,
    #[error("no active identity exists")]
    NoActiveIdentity,
    #[error("an identity import is already pending")]
    ImportAlreadyPending,
    #[error("identity import operation does not match")]
    ImportOperationMismatch,
}

impl IdentityError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidIdentityId => "invalid_identity_id",
            Self::InvalidPublicKey => "invalid_public_key",
            Self::InvalidOperationId => "invalid_identity_operation_id",
            Self::DuplicateIdentityId => "duplicate_identity_id",
            Self::DuplicatePublicKey => "duplicate_identity_public_key",
            Self::UnknownIdentity => "unknown_identity",
            Self::NoActiveIdentity => "no_active_identity",
            Self::ImportAlreadyPending => "identity_import_already_pending",
            Self::ImportOperationMismatch => "identity_import_operation_mismatch",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RelayAccessPreference {
    ReadOnly,
    ReadWrite,
}

impl RelayAccessPreference {
    pub const fn can_write(self) -> bool {
        matches!(self, Self::ReadWrite)
    }

    fn parse(value: &str) -> Result<Self, SettingsError> {
        match value {
            "read_only" => Ok(Self::ReadOnly),
            "read_write" => Ok(Self::ReadWrite),
            _ => Err(SettingsError::UnknownRelayAccess),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::ReadWrite => "read_write",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MobileNetworkEnvironment {
    Public,
    Simulator,
    PhysicalDevice,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayEndpointPreference {
    url: String,
    access: RelayAccessPreference,
}

impl RelayEndpointPreference {
    pub fn new(
        environment: MobileNetworkEnvironment,
        url: impl AsRef<str>,
        access: RelayAccessPreference,
    ) -> Result<Self, SettingsError> {
        let policy = match environment {
            MobileNetworkEnvironment::Public => radroots_sdk::transport::RelayUrlPolicy::Public,
            MobileNetworkEnvironment::Simulator => radroots_sdk::transport::RelayUrlPolicy::Local,
            MobileNetworkEnvironment::PhysicalDevice => {
                radroots_sdk::transport::RelayUrlPolicy::PrivateNetwork
            }
        };
        let url = radroots_sdk::transport::RelayUrl::parse(url, policy)
            .map_err(|_| SettingsError::InvalidRelayEndpoint)?;
        Ok(Self {
            url: url.to_string(),
            access,
        })
    }

    pub fn url(&self) -> &str {
        self.url.as_str()
    }

    pub const fn access(&self) -> RelayAccessPreference {
        self.access
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RelayPreferences {
    environment: MobileNetworkEnvironment,
    endpoints: Vec<RelayEndpointPreference>,
}

impl RelayPreferences {
    pub fn new(
        environment: MobileNetworkEnvironment,
        endpoints: Vec<RelayEndpointPreference>,
    ) -> Result<Self, SettingsError> {
        if endpoints.is_empty() || endpoints.len() > RELAY_ENDPOINT_MAX {
            return Err(SettingsError::InvalidRelayEndpointCount);
        }
        let mut seen = BTreeSet::new();
        for endpoint in &endpoints {
            let validated =
                RelayEndpointPreference::new(environment, endpoint.url(), endpoint.access())?;
            if validated != *endpoint || !seen.insert(endpoint.url()) {
                return Err(SettingsError::DuplicateRelayEndpoint);
            }
        }
        Ok(Self {
            environment,
            endpoints,
        })
    }

    pub fn production_default() -> Self {
        Self::new(
            MobileNetworkEnvironment::Public,
            vec![
                RelayEndpointPreference::new(
                    MobileNetworkEnvironment::Public,
                    DEFAULT_PUBLIC_RELAY,
                    RelayAccessPreference::ReadWrite,
                )
                .expect("bundled public relay is valid"),
            ],
        )
        .expect("bundled public relay profile is valid")
    }

    pub fn simulator_default() -> Self {
        Self::new(
            MobileNetworkEnvironment::Simulator,
            vec![
                RelayEndpointPreference::new(
                    MobileNetworkEnvironment::Simulator,
                    DEFAULT_SIMULATOR_RELAY,
                    RelayAccessPreference::ReadWrite,
                )
                .expect("bundled simulator relay is valid"),
            ],
        )
        .expect("bundled simulator relay profile is valid")
    }

    pub const fn environment(&self) -> MobileNetworkEnvironment {
        self.environment
    }

    pub fn endpoints(&self) -> &[RelayEndpointPreference] {
        self.endpoints.as_slice()
    }

    pub fn sdk_profile(&self) -> Result<radroots_sdk::transport::RelayProfile, SettingsError> {
        let kind = match self.environment {
            MobileNetworkEnvironment::Public => radroots_sdk::transport::RelayProfileKind::Public,
            MobileNetworkEnvironment::Simulator => {
                radroots_sdk::transport::RelayProfileKind::Simulator
            }
            MobileNetworkEnvironment::PhysicalDevice => {
                radroots_sdk::transport::RelayProfileKind::Device
            }
        };
        radroots_sdk::transport::RelayProfile::explicit(
            kind,
            self.endpoints.iter().map(|endpoint| {
                let access = match endpoint.access {
                    RelayAccessPreference::ReadOnly => {
                        radroots_sdk::transport::RelayAccess::ReadOnly
                    }
                    RelayAccessPreference::ReadWrite => {
                        radroots_sdk::transport::RelayAccess::ReadWrite
                    }
                };
                (endpoint.url.as_str(), access)
            }),
        )
        .map_err(|_| SettingsError::InvalidRelayEndpoint)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlossomEndpointAuthorityPreference {
    PublicWebPki,
    LoopbackDevelopment,
    PrivateNetworkDevelopment,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlossomPreferences {
    environment: MobileNetworkEnvironment,
    authority: BlossomEndpointAuthorityPreference,
    primary_origin: String,
    fallback_origins: Vec<String>,
}

impl BlossomPreferences {
    pub fn new(
        environment: MobileNetworkEnvironment,
        authority: BlossomEndpointAuthorityPreference,
        primary_origin: impl Into<String>,
        fallback_origins: Vec<String>,
    ) -> Result<Self, SettingsError> {
        if fallback_origins.len() > BLOSSOM_FALLBACK_MAX {
            return Err(SettingsError::InvalidBlossomEndpointCount);
        }
        let candidate = Self {
            environment,
            authority,
            primary_origin: primary_origin.into(),
            fallback_origins,
        };
        let profile = candidate.sdk_profile()?;
        Ok(Self {
            primary_origin: profile.primary().origin().to_owned(),
            fallback_origins: profile
                .fallbacks()
                .iter()
                .map(|endpoint| endpoint.origin().to_owned())
                .collect(),
            ..candidate
        })
    }

    pub fn production_default() -> Self {
        Self::new(
            MobileNetworkEnvironment::Public,
            BlossomEndpointAuthorityPreference::PublicWebPki,
            DEFAULT_PUBLIC_BLOSSOM_ORIGIN,
            Vec::new(),
        )
        .expect("bundled public Blossom origin is valid")
    }

    pub fn simulator_default() -> Self {
        Self::new(
            MobileNetworkEnvironment::Simulator,
            BlossomEndpointAuthorityPreference::LoopbackDevelopment,
            DEFAULT_SIMULATOR_BLOSSOM_ORIGIN,
            Vec::new(),
        )
        .expect("bundled simulator Blossom origin is valid")
    }

    pub const fn environment(&self) -> MobileNetworkEnvironment {
        self.environment
    }

    pub const fn authority(&self) -> BlossomEndpointAuthorityPreference {
        self.authority
    }

    pub fn primary_origin(&self) -> &str {
        self.primary_origin.as_str()
    }

    pub fn fallback_origins(&self) -> &[String] {
        self.fallback_origins.as_slice()
    }

    pub fn sdk_profile(&self) -> Result<radroots_sdk::transport::BlossomProfile, SettingsError> {
        let host_kind = match self.environment {
            MobileNetworkEnvironment::Public => radroots_sdk::transport::BlossomHostKind::Native,
            MobileNetworkEnvironment::Simulator => {
                radroots_sdk::transport::BlossomHostKind::Simulator
            }
            MobileNetworkEnvironment::PhysicalDevice => {
                radroots_sdk::transport::BlossomHostKind::PhysicalDevice
            }
        };
        let authority = match self.authority {
            BlossomEndpointAuthorityPreference::PublicWebPki => {
                radroots_sdk::transport::BlossomEndpointAuthority::PublicWebPki
            }
            BlossomEndpointAuthorityPreference::LoopbackDevelopment => {
                radroots_sdk::transport::BlossomEndpointAuthority::LoopbackDevelopment
            }
            BlossomEndpointAuthorityPreference::PrivateNetworkDevelopment => {
                radroots_sdk::transport::BlossomEndpointAuthority::PrivateNetworkDevelopment
            }
        };
        radroots_sdk::transport::BlossomProfile::new(
            host_kind,
            authority,
            self.primary_origin.as_str(),
            self.fallback_origins.iter().map(String::as_str),
        )
        .map_err(|_| SettingsError::InvalidBlossomEndpoint)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaNetworkPolicy {
    allow_cellular_downloads: bool,
    allow_cellular_uploads: bool,
    allow_background_transfers: bool,
}

impl MediaNetworkPolicy {
    pub const fn new(
        allow_cellular_downloads: bool,
        allow_cellular_uploads: bool,
        allow_background_transfers: bool,
    ) -> Self {
        Self {
            allow_cellular_downloads,
            allow_cellular_uploads,
            allow_background_transfers,
        }
    }

    pub const fn allow_cellular_downloads(&self) -> bool {
        self.allow_cellular_downloads
    }

    pub const fn allow_cellular_uploads(&self) -> bool {
        self.allow_cellular_uploads
    }

    pub const fn allow_background_transfers(&self) -> bool {
        self.allow_background_transfers
    }
}

impl Default for MediaNetworkPolicy {
    fn default() -> Self {
        Self::new(true, true, true)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalStoragePolicy {
    media_cache_bytes: u64,
    media_cache_artifacts: u32,
}

impl LocalStoragePolicy {
    pub fn new(media_cache_bytes: u64, media_cache_artifacts: u32) -> Result<Self, SettingsError> {
        if !(MEDIA_CACHE_MIN_BYTES..=MEDIA_CACHE_MAX_BYTES).contains(&media_cache_bytes) {
            return Err(SettingsError::InvalidMediaCacheBytes);
        }
        if media_cache_artifacts == 0 || media_cache_artifacts > MEDIA_CACHE_MAX_ARTIFACTS {
            return Err(SettingsError::InvalidMediaCacheArtifacts);
        }
        Ok(Self {
            media_cache_bytes,
            media_cache_artifacts,
        })
    }

    pub const fn media_cache_bytes(&self) -> u64 {
        self.media_cache_bytes
    }

    pub const fn media_cache_artifacts(&self) -> u32 {
        self.media_cache_artifacts
    }
}

impl Default for LocalStoragePolicy {
    fn default() -> Self {
        Self::new(256 * 1024 * 1024, 2_000).expect("default storage policy is valid")
    }
}

/// Complete replacement command for the adopted kind-0 metadata surface.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ProfileMetadataCommand(AuthoredProfile);

impl ProfileMetadataCommand {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        name: String,
        display_name: Option<String>,
        about: Option<String>,
        picture: Option<AuthoredImage>,
        banner: Option<AuthoredImage>,
        nip05: Option<String>,
        bot: Option<bool>,
    ) -> Result<Self, ProfileMetadataError> {
        validate_profile_text(&name, PROFILE_NAME_MAX_BYTES, false)
            .then_some(())
            .ok_or(ProfileMetadataError::InvalidName)?;
        if display_name.as_deref().is_some_and(|value| {
            !validate_profile_text(value, PROFILE_DISPLAY_NAME_MAX_BYTES, true)
        }) {
            return Err(ProfileMetadataError::InvalidDisplayName);
        }
        if about
            .as_deref()
            .is_some_and(|value| !validate_profile_text(value, PROFILE_ABOUT_MAX_BYTES, true))
        {
            return Err(ProfileMetadataError::InvalidAbout);
        }
        let nip05 = nip05
            .as_deref()
            .map(Nip05Identifier::parse)
            .transpose()
            .map_err(|_| ProfileMetadataError::InvalidNip05)?;
        let mut profile =
            AuthoredProfile::new(name).map_err(|_| ProfileMetadataError::InvalidName)?;
        if let Some(value) = display_name {
            profile = profile.with_display_name(value);
        }
        if let Some(value) = about {
            profile = profile.with_about(value);
        }
        if let Some(value) = picture {
            profile = profile.with_picture(value);
        }
        if let Some(value) = banner {
            profile = profile.with_banner(value);
        }
        if let Some(value) = nip05 {
            profile = profile.with_nip05(value);
        }
        if let Some(value) = bot {
            profile = profile.with_bot(value);
        }
        Ok(Self(profile))
    }

    pub const fn authored(&self) -> &AuthoredProfile {
        &self.0
    }
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ProfileMetadataError {
    #[error("profile name is invalid")]
    InvalidName,
    #[error("profile display name is invalid")]
    InvalidDisplayName,
    #[error("profile about text is invalid")]
    InvalidAbout,
    #[error("profile NIP-05 identifier is invalid")]
    InvalidNip05,
}

impl ProfileMetadataError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::InvalidName => "invalid_profile_name",
            Self::InvalidDisplayName => "invalid_profile_display_name",
            Self::InvalidAbout => "invalid_profile_about",
            Self::InvalidNip05 => "invalid_profile_nip05",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MobileSettings {
    revision: u64,
    identity: IdentityState,
    relays: RelayPreferences,
    blossom: BlossomPreferences,
    media_network: MediaNetworkPolicy,
    local_storage: LocalStoragePolicy,
}

impl Default for MobileSettings {
    fn default() -> Self {
        Self {
            revision: 1,
            identity: IdentityState::default(),
            relays: RelayPreferences::production_default(),
            blossom: BlossomPreferences::production_default(),
            media_network: MediaNetworkPolicy::default(),
            local_storage: LocalStoragePolicy::default(),
        }
    }
}

impl MobileSettings {
    pub const fn revision(&self) -> u64 {
        self.revision
    }

    pub const fn identity(&self) -> &IdentityState {
        &self.identity
    }

    pub const fn relays(&self) -> &RelayPreferences {
        &self.relays
    }

    pub const fn blossom(&self) -> &BlossomPreferences {
        &self.blossom
    }

    pub const fn media_network(&self) -> &MediaNetworkPolicy {
        &self.media_network
    }

    pub const fn local_storage(&self) -> &LocalStoragePolicy {
        &self.local_storage
    }

    #[must_use]
    pub fn with_identity(mut self, identity: IdentityState) -> Self {
        self.identity = identity;
        self
    }

    #[must_use]
    pub fn with_relays(mut self, relays: RelayPreferences) -> Self {
        self.relays = relays;
        self
    }

    #[must_use]
    pub fn with_blossom(mut self, blossom: BlossomPreferences) -> Self {
        self.blossom = blossom;
        self
    }

    #[must_use]
    pub fn with_media_network(mut self, media_network: MediaNetworkPolicy) -> Self {
        self.media_network = media_network;
        self
    }

    #[must_use]
    pub fn with_local_storage(mut self, local_storage: LocalStoragePolicy) -> Self {
        self.local_storage = local_storage;
        self
    }

    fn validate(&self) -> Result<(), SettingsError> {
        if self.relays.environment() != self.blossom.environment() {
            return Err(SettingsError::NetworkEnvironmentMismatch);
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReplaceMobileSettings {
    expected_revision: u64,
    settings: MobileSettings,
}

impl ReplaceMobileSettings {
    pub fn new(expected_revision: u64, settings: MobileSettings) -> Result<Self, SettingsError> {
        if expected_revision == 0 || settings.revision != expected_revision {
            return Err(SettingsError::RevisionConflict);
        }
        settings.validate()?;
        Ok(Self {
            expected_revision,
            settings,
        })
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SettingsTransition {
    pub settings: MobileSettings,
    pub runtime_restart_required: bool,
    pub outbox_requeue_required: bool,
    pub media_cache_invalidation_required: bool,
}

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum SettingsError {
    #[error("relay access value is unknown")]
    UnknownRelayAccess,
    #[error("relay endpoint is invalid")]
    InvalidRelayEndpoint,
    #[error("relay endpoint count is invalid")]
    InvalidRelayEndpointCount,
    #[error("relay endpoint is duplicated")]
    DuplicateRelayEndpoint,
    #[error("Blossom endpoint is invalid")]
    InvalidBlossomEndpoint,
    #[error("Blossom endpoint count is invalid")]
    InvalidBlossomEndpointCount,
    #[error("relay and Blossom network environments do not match")]
    NetworkEnvironmentMismatch,
    #[error("media cache byte quota is invalid")]
    InvalidMediaCacheBytes,
    #[error("media cache artifact quota is invalid")]
    InvalidMediaCacheArtifacts,
    #[error("settings revision conflicts with durable state")]
    RevisionConflict,
    #[error("settings revision is exhausted")]
    RevisionExhausted,
    #[error("settings schema version is unsupported")]
    UnsupportedSchema,
    #[error("settings document is corrupt")]
    CorruptDocument,
    #[error("settings storage is unavailable")]
    Storage,
    #[error("identity settings are invalid: {0}")]
    Identity(#[from] IdentityError),
}

impl SettingsError {
    pub const fn code(&self) -> &'static str {
        match self {
            Self::UnknownRelayAccess => "unknown_relay_access",
            Self::InvalidRelayEndpoint => "invalid_relay_endpoint",
            Self::InvalidRelayEndpointCount => "invalid_relay_endpoint_count",
            Self::DuplicateRelayEndpoint => "duplicate_relay_endpoint",
            Self::InvalidBlossomEndpoint => "invalid_blossom_endpoint",
            Self::InvalidBlossomEndpointCount => "invalid_blossom_endpoint_count",
            Self::NetworkEnvironmentMismatch => "network_environment_mismatch",
            Self::InvalidMediaCacheBytes => "invalid_media_cache_bytes",
            Self::InvalidMediaCacheArtifacts => "invalid_media_cache_artifacts",
            Self::RevisionConflict => "settings_revision_conflict",
            Self::RevisionExhausted => "settings_revision_exhausted",
            Self::UnsupportedSchema => "unsupported_settings_schema",
            Self::CorruptDocument => "corrupt_settings_document",
            Self::Storage => "settings_storage_unavailable",
            Self::Identity(error) => error.code(),
        }
    }
}

impl RadrootsRuntime {
    pub async fn phase1_settings(&self) -> Result<MobileSettings, SettingsError> {
        let storage = self.client.storage().map_err(|_| SettingsError::Storage)?;
        let mut settings = load_settings(storage).await?;
        let session = self.identity_session.read().await;
        if let Some((revision, identity)) = session.as_ref()
            && *revision == settings.revision
        {
            settings.identity = identity.clone();
        }
        Ok(settings)
    }

    pub async fn phase1_replace_settings(
        &self,
        command: ReplaceMobileSettings,
    ) -> Result<SettingsTransition, SettingsError> {
        let _guard = self.settings_lock.lock().await;
        let storage = self.client.storage().map_err(|_| SettingsError::Storage)?;
        let transition = replace_settings(storage, command).await?;
        *self.identity_session.write().await = None;
        Ok(transition)
    }

    /// Applies one secret-free identity transition atomically against the
    /// current settings revision. Unlock evidence is process-local: it is
    /// observable for the current runtime but never written to durable state.
    pub async fn phase1_apply_identity_command(
        &self,
        expected_revision: u64,
        command: IdentityCommand,
    ) -> Result<SettingsTransition, SettingsError> {
        let _guard = self.settings_lock.lock().await;
        let storage = self.client.storage().map_err(|_| SettingsError::Storage)?;
        let mut prior = load_settings(storage).await?;
        if prior.revision != expected_revision {
            return Err(SettingsError::RevisionConflict);
        }
        if let Some((revision, identity)) = self.identity_session.read().await.as_ref()
            && *revision == prior.revision
        {
            prior.identity = identity.clone();
        }
        let next_identity = prior.identity.apply(command.clone())?;
        if matches!(command, IdentityCommand::Unlock) {
            *self.identity_session.write().await = Some((prior.revision, next_identity.clone()));
            return Ok(SettingsTransition {
                settings: prior.with_identity(next_identity),
                runtime_restart_required: false,
                outbox_requeue_required: false,
                media_cache_invalidation_required: false,
            });
        }
        let command =
            ReplaceMobileSettings::new(prior.revision, prior.with_identity(next_identity))?;
        let transition = replace_settings(storage, command).await?;
        *self.identity_session.write().await = None;
        Ok(transition)
    }
}

async fn load_settings(
    storage: &dyn radroots_storage::Storage,
) -> Result<MobileSettings, SettingsError> {
    let document = ProjectionStore::projection_document(
        storage,
        settings_projection_id()?,
        settings_generation()?,
        SETTINGS_DOCUMENT_KEY.to_owned(),
    )
    .await
    .map_err(|_| SettingsError::Storage)?;
    document
        .map(|document| decode_settings(document.value()))
        .transpose()
        .map(Option::unwrap_or_default)
}

async fn replace_settings(
    storage: &dyn radroots_storage::Storage,
    command: ReplaceMobileSettings,
) -> Result<SettingsTransition, SettingsError> {
    let prior = load_settings(storage).await?;
    if prior.revision != command.expected_revision {
        return Err(SettingsError::RevisionConflict);
    }
    let mut next = command.settings;
    next.revision = prior
        .revision
        .checked_add(1)
        .ok_or(SettingsError::RevisionExhausted)?;
    // Unlock is a native, process-local custody result. A durable settings
    // write must never make a later process assume user presence succeeded.
    next.identity.lock_state = IdentityLockState::Locked;
    let transition = settings_transition(&prior, next);
    ProjectionStore::put_projection_document(
        storage,
        settings_projection_id()?,
        settings_generation()?,
        ProjectionDocument::new(
            SETTINGS_DOCUMENT_KEY.to_owned(),
            encode_settings(&transition.settings)?,
        )
        .map_err(|_| SettingsError::CorruptDocument)?,
    )
    .await
    .map_err(|_| SettingsError::Storage)?;
    Ok(transition)
}

fn settings_transition(prior: &MobileSettings, settings: MobileSettings) -> SettingsTransition {
    let identity_changed = prior.identity != settings.identity;
    let relay_changed = prior.relays != settings.relays;
    let blossom_changed = prior.blossom != settings.blossom;
    let media_changed = prior.media_network != settings.media_network;
    let storage_changed = prior.local_storage != settings.local_storage;
    SettingsTransition {
        runtime_restart_required: identity_changed || relay_changed || blossom_changed,
        outbox_requeue_required: identity_changed || relay_changed || blossom_changed,
        media_cache_invalidation_required: identity_changed
            || blossom_changed
            || media_changed
            || storage_changed,
        settings,
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSettingsV1 {
    schema_version: u16,
    revision: u64,
    identity: StoredIdentity,
    relays: StoredRelays,
    blossom: StoredBlossom,
    media_network: MediaNetworkPolicy,
    local_storage: StoredLocalStorage,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredSettingsV0 {
    schema_version: u16,
    revision: u64,
    identity: StoredIdentity,
    relays: StoredRelays,
    blossom: StoredBlossom,
    allow_cellular_downloads: bool,
    allow_cellular_uploads: bool,
    media_cache_bytes: u64,
    media_cache_artifacts: u32,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredIdentity {
    identities: Vec<StoredIdentityRecord>,
    active_identity_id: Option<String>,
    lock_state: IdentityLockState,
    pending_import_operation_id: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredIdentityRecord {
    id: String,
    public_key_hex: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRelays {
    environment: MobileNetworkEnvironment,
    endpoints: Vec<StoredRelayEndpoint>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredRelayEndpoint {
    url: String,
    access: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredBlossom {
    environment: MobileNetworkEnvironment,
    authority: BlossomEndpointAuthorityPreference,
    primary_origin: String,
    fallback_origins: Vec<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredLocalStorage {
    media_cache_bytes: u64,
    media_cache_artifacts: u32,
}

#[derive(Deserialize)]
struct VersionProbe {
    schema_version: u16,
}

fn encode_settings(settings: &MobileSettings) -> Result<Vec<u8>, SettingsError> {
    serde_json::to_vec(&StoredSettingsV1::from(settings))
        .map_err(|_| SettingsError::CorruptDocument)
}

fn decode_settings(value: &[u8]) -> Result<MobileSettings, SettingsError> {
    let version = serde_json::from_slice::<VersionProbe>(value)
        .map_err(|_| SettingsError::CorruptDocument)?
        .schema_version;
    match version {
        0 => serde_json::from_slice::<StoredSettingsV0>(value)
            .map_err(|_| SettingsError::CorruptDocument)?
            .try_into(),
        MOBILE_SETTINGS_SCHEMA_VERSION => serde_json::from_slice::<StoredSettingsV1>(value)
            .map_err(|_| SettingsError::CorruptDocument)?
            .try_into(),
        _ => Err(SettingsError::UnsupportedSchema),
    }
}

impl From<&MobileSettings> for StoredSettingsV1 {
    fn from(value: &MobileSettings) -> Self {
        Self {
            schema_version: MOBILE_SETTINGS_SCHEMA_VERSION,
            revision: value.revision,
            identity: StoredIdentity::from(&value.identity),
            relays: StoredRelays::from(&value.relays),
            blossom: StoredBlossom::from(&value.blossom),
            media_network: value.media_network.clone(),
            local_storage: StoredLocalStorage::from(&value.local_storage),
        }
    }
}

impl TryFrom<StoredSettingsV1> for MobileSettings {
    type Error = SettingsError;

    fn try_from(value: StoredSettingsV1) -> Result<Self, Self::Error> {
        if value.schema_version != MOBILE_SETTINGS_SCHEMA_VERSION || value.revision == 0 {
            return Err(SettingsError::CorruptDocument);
        }
        let settings = Self {
            revision: value.revision,
            identity: value.identity.try_into()?,
            relays: value.relays.try_into()?,
            blossom: value.blossom.try_into()?,
            media_network: value.media_network,
            local_storage: value.local_storage.try_into()?,
        };
        settings.validate()?;
        Ok(settings)
    }
}

impl TryFrom<StoredSettingsV0> for MobileSettings {
    type Error = SettingsError;

    fn try_from(value: StoredSettingsV0) -> Result<Self, Self::Error> {
        if value.schema_version != 0 || value.revision == 0 {
            return Err(SettingsError::CorruptDocument);
        }
        let settings = Self {
            revision: value.revision,
            identity: value.identity.try_into()?,
            relays: value.relays.try_into()?,
            blossom: value.blossom.try_into()?,
            media_network: MediaNetworkPolicy::new(
                value.allow_cellular_downloads,
                value.allow_cellular_uploads,
                false,
            ),
            local_storage: LocalStoragePolicy::new(
                value.media_cache_bytes,
                value.media_cache_artifacts,
            )?,
        };
        settings.validate()?;
        Ok(settings)
    }
}

impl From<&IdentityState> for StoredIdentity {
    fn from(value: &IdentityState) -> Self {
        Self {
            identities: value
                .identities
                .iter()
                .map(|identity| StoredIdentityRecord {
                    id: identity.id.clone(),
                    public_key_hex: identity.public_key_hex.clone(),
                })
                .collect(),
            active_identity_id: value.active_identity_id.clone(),
            lock_state: IdentityLockState::Locked,
            pending_import_operation_id: value.pending_import_operation_id.clone(),
        }
    }
}

impl TryFrom<StoredIdentity> for IdentityState {
    type Error = SettingsError;

    fn try_from(value: StoredIdentity) -> Result<Self, Self::Error> {
        let identities = value
            .identities
            .into_iter()
            .map(|identity| IdentityRecord::new(identity.id, &identity.public_key_hex))
            .collect::<Result<Vec<_>, _>>()?;
        IdentityState::new(
            identities,
            value.active_identity_id,
            IdentityLockState::Locked,
            value.pending_import_operation_id,
        )
        .map_err(SettingsError::from)
    }
}

impl From<&RelayPreferences> for StoredRelays {
    fn from(value: &RelayPreferences) -> Self {
        Self {
            environment: value.environment,
            endpoints: value
                .endpoints
                .iter()
                .map(|endpoint| StoredRelayEndpoint {
                    url: endpoint.url.clone(),
                    access: endpoint.access.as_str().to_owned(),
                })
                .collect(),
        }
    }
}

impl TryFrom<StoredRelays> for RelayPreferences {
    type Error = SettingsError;

    fn try_from(value: StoredRelays) -> Result<Self, Self::Error> {
        let endpoints = value
            .endpoints
            .into_iter()
            .map(|endpoint| {
                RelayEndpointPreference::new(
                    value.environment,
                    endpoint.url,
                    RelayAccessPreference::parse(&endpoint.access)?,
                )
            })
            .collect::<Result<Vec<_>, _>>()?;
        Self::new(value.environment, endpoints)
    }
}

impl From<&BlossomPreferences> for StoredBlossom {
    fn from(value: &BlossomPreferences) -> Self {
        Self {
            environment: value.environment,
            authority: value.authority,
            primary_origin: value.primary_origin.clone(),
            fallback_origins: value.fallback_origins.clone(),
        }
    }
}

impl TryFrom<StoredBlossom> for BlossomPreferences {
    type Error = SettingsError;

    fn try_from(value: StoredBlossom) -> Result<Self, Self::Error> {
        Self::new(
            value.environment,
            value.authority,
            value.primary_origin,
            value.fallback_origins,
        )
    }
}

impl From<&LocalStoragePolicy> for StoredLocalStorage {
    fn from(value: &LocalStoragePolicy) -> Self {
        Self {
            media_cache_bytes: value.media_cache_bytes,
            media_cache_artifacts: value.media_cache_artifacts,
        }
    }
}

impl TryFrom<StoredLocalStorage> for LocalStoragePolicy {
    type Error = SettingsError;

    fn try_from(value: StoredLocalStorage) -> Result<Self, Self::Error> {
        Self::new(value.media_cache_bytes, value.media_cache_artifacts)
    }
}

fn settings_projection_id() -> Result<ProjectionId, SettingsError> {
    ProjectionId::parse(SETTINGS_PROJECTION_ID).map_err(|_| SettingsError::CorruptDocument)
}

fn settings_generation() -> Result<ProjectionGeneration, SettingsError> {
    ProjectionGeneration::new(Sha256::digest(SETTINGS_GENERATION_DOMAIN).into())
        .map_err(|_| SettingsError::CorruptDocument)
}

fn validate_identifier(value: &str, max_bytes: usize) -> bool {
    !value.is_empty()
        && value.len() <= max_bytes
        && value == value.trim()
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-' | b'.' | b':'))
}

fn validate_profile_text(value: &str, max_bytes: usize, allow_empty: bool) -> bool {
    value.len() <= max_bytes
        && (allow_empty || !value.trim().is_empty())
        && !value.chars().any(char::is_control)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::{
        builder::RuntimeBuilder,
        store::{MobileUserStoreConfig, ProtectedDataAvailability},
    };

    const PUBLIC_KEY: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

    #[test]
    fn identity_transitions_never_accept_or_serialize_private_key_material() {
        let state = IdentityState::default()
            .apply(IdentityCommand::BeginImport {
                operation_id: "import:1".to_owned(),
            })
            .unwrap()
            .apply(IdentityCommand::CompleteImport {
                operation_id: "import:1".to_owned(),
                identity: IdentityRecord::new("primary", PUBLIC_KEY).unwrap(),
            })
            .unwrap();
        assert_eq!(state.active_identity_id(), Some("primary"));
        assert_eq!(state.lock_state(), IdentityLockState::Locked);
        let settings = MobileSettings::default().with_identity(state);
        let encoded = String::from_utf8(encode_settings(&settings).unwrap()).unwrap();
        assert!(encoded.contains(PUBLIC_KEY));
        assert!(!encoded.contains("private"));
        assert!(!encoded.contains("secret"));
    }

    #[test]
    fn durable_identity_state_always_reopens_locked() {
        let identity = IdentityRecord::new("primary", PUBLIC_KEY).unwrap();
        let state = IdentityState::new(
            vec![identity],
            Some("primary".to_owned()),
            IdentityLockState::Unlocked,
            None,
        )
        .unwrap();
        let settings = MobileSettings::default().with_identity(state);
        let encoded = encode_settings(&settings).unwrap();
        assert!(!String::from_utf8_lossy(&encoded).contains("unlocked"));
        assert_eq!(
            decode_settings(&encoded).unwrap().identity().lock_state(),
            IdentityLockState::Locked
        );
    }

    #[test]
    fn unknown_relay_access_fails_closed_during_decode() {
        let mut stored = StoredSettingsV1::from(&MobileSettings::default());
        stored.relays.endpoints[0].access = "write".to_owned();
        let encoded = serde_json::to_vec(&stored).unwrap();
        assert_eq!(
            decode_settings(&encoded).unwrap_err(),
            SettingsError::UnknownRelayAccess
        );
    }

    #[test]
    fn validated_relay_preferences_preserve_read_only_access() {
        let preferences = RelayPreferences::new(
            MobileNetworkEnvironment::Public,
            vec![
                RelayEndpointPreference::new(
                    MobileNetworkEnvironment::Public,
                    "wss://read.example",
                    RelayAccessPreference::ReadOnly,
                )
                .unwrap(),
            ],
        )
        .unwrap();
        let profile = preferences.sdk_profile().unwrap();
        assert_eq!(profile.endpoints().len(), 1);
        assert!(!profile.endpoints()[0].access().can_write());
    }

    #[test]
    fn production_and_simulator_defaults_use_canonical_origins() {
        let production = MobileSettings::default();
        assert_eq!(
            production.relays().endpoints()[0].url(),
            DEFAULT_PUBLIC_RELAY
        );
        assert_eq!(
            production.blossom().primary_origin(),
            DEFAULT_PUBLIC_BLOSSOM_ORIGIN
        );
        assert_eq!(
            RelayPreferences::simulator_default().endpoints()[0].url(),
            DEFAULT_SIMULATOR_RELAY
        );
        assert_eq!(
            BlossomPreferences::simulator_default().primary_origin(),
            DEFAULT_SIMULATOR_BLOSSOM_ORIGIN
        );
    }

    #[test]
    fn version_zero_migrates_background_transfers_to_disabled() {
        let current = StoredSettingsV1::from(&MobileSettings::default());
        let legacy = serde_json::json!({
            "schema_version": 0,
            "revision": current.revision,
            "identity": current.identity,
            "relays": current.relays,
            "blossom": current.blossom,
            "allow_cellular_downloads": true,
            "allow_cellular_uploads": false,
            "media_cache_bytes": current.local_storage.media_cache_bytes,
            "media_cache_artifacts": current.local_storage.media_cache_artifacts,
        });
        let migrated = decode_settings(&serde_json::to_vec(&legacy).unwrap()).unwrap();
        assert!(!migrated.media_network().allow_background_transfers());
        assert!(!migrated.media_network().allow_cellular_uploads());
    }

    #[test]
    fn future_or_unknown_fields_fail_safely() {
        let mut future =
            serde_json::to_value(StoredSettingsV1::from(&MobileSettings::default())).unwrap();
        future["schema_version"] = serde_json::json!(2);
        assert_eq!(
            decode_settings(&serde_json::to_vec(&future).unwrap()).unwrap_err(),
            SettingsError::UnsupportedSchema
        );

        let mut unknown =
            serde_json::to_value(StoredSettingsV1::from(&MobileSettings::default())).unwrap();
        unknown["write_all_relays"] = serde_json::json!(true);
        assert_eq!(
            decode_settings(&serde_json::to_vec(&unknown).unwrap()).unwrap_err(),
            SettingsError::CorruptDocument
        );
    }

    #[test]
    fn profile_metadata_command_validates_the_adopted_kind_zero_fields() {
        let command = ProfileMetadataCommand::new(
            "grower".to_owned(),
            Some("Local Grower".to_owned()),
            Some("Seasonal produce".to_owned()),
            None,
            None,
            Some("grower@farm.example".to_owned()),
            Some(false),
        )
        .unwrap();
        assert_eq!(command.authored().name(), "grower");
        assert_eq!(
            command.authored().nip05().map(Nip05Identifier::as_str),
            Some("grower@farm.example")
        );
        assert_eq!(
            ProfileMetadataCommand::new(
                "grower".to_owned(),
                None,
                None,
                None,
                None,
                Some("GROWER@farm.example".to_owned()),
                None,
            )
            .unwrap_err()
            .code(),
            "invalid_profile_nip05"
        );
    }

    #[tokio::test]
    async fn settings_are_revision_checked_persisted_and_report_exact_effects() {
        let runtime = RadrootsRuntime::test_memory().unwrap();
        let settings = runtime.phase1_settings().await.unwrap();
        let next = settings
            .clone()
            .with_media_network(MediaNetworkPolicy::new(false, true, false));
        let transition = runtime
            .phase1_replace_settings(ReplaceMobileSettings::new(settings.revision(), next).unwrap())
            .await
            .unwrap();
        assert_eq!(transition.settings.revision(), 2);
        assert!(!transition.runtime_restart_required);
        assert!(!transition.outbox_requeue_required);
        assert!(transition.media_cache_invalidation_required);
        assert_eq!(
            runtime.phase1_settings().await.unwrap(),
            transition.settings
        );

        let conflict = runtime
            .phase1_replace_settings(
                ReplaceMobileSettings::new(settings.revision(), settings).unwrap(),
            )
            .await
            .unwrap_err();
        assert_eq!(conflict, SettingsError::RevisionConflict);
    }

    #[test]
    fn settings_reject_mixed_network_environments() {
        let settings = MobileSettings::default().with_blossom(BlossomPreferences::simulator_default());
        assert_eq!(
            ReplaceMobileSettings::new(settings.revision(), settings).unwrap_err(),
            SettingsError::NetworkEnvironmentMismatch
        );
    }

    #[tokio::test]
    async fn atomic_identity_commands_persist_public_state_but_keep_unlock_process_local() {
        let runtime = RadrootsRuntime::test_memory().unwrap();
        let begun = runtime
            .phase1_apply_identity_command(
                1,
                IdentityCommand::BeginImport {
                    operation_id: "import-1".to_owned(),
                },
            )
            .await
            .unwrap();
        assert_eq!(begun.settings.revision(), 2);
        assert_eq!(
            begun.settings.identity().pending_import_operation_id(),
            Some("import-1")
        );

        let completed = runtime
            .phase1_apply_identity_command(
                2,
                IdentityCommand::CompleteImport {
                    operation_id: "import-1".to_owned(),
                    identity: IdentityRecord::new(
                        "primary",
                        "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
                    )
                    .unwrap(),
                },
            )
            .await
            .unwrap();
        assert_eq!(completed.settings.revision(), 3);
        assert_eq!(
            completed.settings.identity().active_identity_id(),
            Some("primary")
        );

        let unlocked = runtime
            .phase1_apply_identity_command(3, IdentityCommand::Unlock)
            .await
            .unwrap();
        assert_eq!(unlocked.settings.revision(), 3);
        assert_eq!(
            unlocked.settings.identity().lock_state(),
            IdentityLockState::Unlocked
        );
        assert_eq!(
            runtime
                .phase1_settings()
                .await
                .unwrap()
                .identity()
                .lock_state(),
            IdentityLockState::Unlocked
        );

        let locked = runtime
            .phase1_apply_identity_command(3, IdentityCommand::Lock)
            .await
            .unwrap();
        assert_eq!(locked.settings.revision(), 4);
        assert_eq!(
            locked.settings.identity().lock_state(),
            IdentityLockState::Locked
        );
    }

    #[tokio::test]
    async fn concurrent_replacements_cannot_both_commit_the_same_revision() {
        let runtime = RadrootsRuntime::test_memory().unwrap();
        let settings = runtime.phase1_settings().await.unwrap();
        let first = ReplaceMobileSettings::new(
            settings.revision(),
            settings
                .clone()
                .with_media_network(MediaNetworkPolicy::new(false, true, true)),
        )
        .unwrap();
        let second = ReplaceMobileSettings::new(
            settings.revision(),
            settings.with_media_network(MediaNetworkPolicy::new(true, false, true)),
        )
        .unwrap();

        let (first, second) = tokio::join!(
            runtime.phase1_replace_settings(first),
            runtime.phase1_replace_settings(second)
        );
        assert_eq!(usize::from(first.is_ok()) + usize::from(second.is_ok()), 1);
        let failure = first.err().or_else(|| second.err()).unwrap();
        assert_eq!(failure, SettingsError::RevisionConflict);
    }

    #[tokio::test]
    async fn sqlite_settings_survive_a_runtime_restart() {
        let root = tempfile::tempdir().unwrap();
        let store = MobileUserStoreConfig::from_encoded(
            root.path(),
            PUBLIC_KEY,
            "0303030303030303030303030303030303030303030303030303030303030303",
            1_800_000_000_000,
            ProtectedDataAvailability::Available,
        )
        .unwrap();
        std::fs::create_dir_all(store.owner_directory()).unwrap();
        let runtime = RuntimeBuilder::new(store.clone()).build().await.unwrap();
        let settings = runtime.phase1_settings().await.unwrap();
        let transition = runtime
            .phase1_replace_settings(
                ReplaceMobileSettings::new(
                    settings.revision(),
                    settings.with_media_network(MediaNetworkPolicy::new(false, false, false)),
                )
                .unwrap(),
            )
            .await
            .unwrap();
        runtime.shutdown().await.unwrap();
        drop(runtime);

        let reopened = RuntimeBuilder::new(store).build().await.unwrap();
        assert_eq!(
            reopened.phase1_settings().await.unwrap(),
            transition.settings
        );
        reopened.shutdown().await.unwrap();
    }
}
