//! Focused secret-safe operation records for settings, profile, revision, and inbound media.

use radroots_mobile_core::runtime::product_surface::{
    AddCommandType, BlossomEndpointAuthorityPreference, BlossomPreferences, IdentityCommand,
    IdentityLockState, IdentityRecord, IdentityState, LocalStoragePolicy, MediaNetworkPolicy,
    MobileNetworkEnvironment, MobileSettings, Phase1LocalMediaArtifact, Phase1MediaCacheStatus,
    Phase1ProfileStatus, Phase1RevisionPhase, Phase1RevisionPolicy, Phase1RevisionStatus,
    Phase1RevisionTarget, ProfileMetadataCommand, RelayAccessPreference, RelayEndpointPreference,
    RelayPreferences, SettingsTransition, phase1_new_operation_id,
};

use crate::dto::PreparedMedia;
use crate::{
    FfiAddDraftInput, FfiDraftStatusRecord, FfiOperationSettlementRecord, FfiOutboxState,
    FfiPreparedMediaInput, MOBILE_FFI_SCHEMA_VERSION, RadrootsAppError,
};

impl From<crate::FfiAddCommandType> for AddCommandType {
    fn from(value: crate::FfiAddCommandType) -> Self {
        match value {
            crate::FfiAddCommandType::CreateUpdate => Self::CreateUpdate,
            crate::FfiAddCommandType::CreatePhotoUpdate => Self::CreatePhotoUpdate,
            crate::FfiAddCommandType::CreateAsk => Self::CreateAsk,
            crate::FfiAddCommandType::CreateEvent => Self::CreateEvent,
            crate::FfiAddCommandType::CreateFoodAvailability => Self::CreateFoodAvailability,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiIdentityLockState {
    Locked,
    Unlocked,
}

impl From<IdentityLockState> for FfiIdentityLockState {
    fn from(value: IdentityLockState) -> Self {
        match value {
            IdentityLockState::Locked => Self::Locked,
            IdentityLockState::Unlocked => Self::Unlocked,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiSettingsIdentityRecord {
    pub schema_version: u16,
    pub id: String,
    pub public_key: String,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiIdentityStateRecord {
    pub schema_version: u16,
    pub identities: Vec<FfiSettingsIdentityRecord>,
    pub active_identity_id: Option<String>,
    pub lock_state: FfiIdentityLockState,
    pub pending_import_operation_id: Option<String>,
}

impl From<&IdentityState> for FfiIdentityStateRecord {
    fn from(value: &IdentityState) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            identities: value
                .identities()
                .iter()
                .map(|identity| FfiSettingsIdentityRecord {
                    schema_version: MOBILE_FFI_SCHEMA_VERSION,
                    id: identity.id().to_owned(),
                    public_key: identity.public_key_hex().to_owned(),
                })
                .collect(),
            active_identity_id: value.active_identity_id().map(str::to_owned),
            lock_state: value.lock_state().into(),
            pending_import_operation_id: value.pending_import_operation_id().map(str::to_owned),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiIdentityCommandKind {
    BeginImport,
    CompleteImport,
    CancelImport,
    Select,
    Lock,
    Unlock,
    Recover,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiIdentityCommandRecord {
    pub schema_version: u16,
    pub kind: FfiIdentityCommandKind,
    pub operation_id: Option<String>,
    pub identity_id: Option<String>,
    pub public_key: Option<String>,
}

impl TryFrom<FfiIdentityCommandRecord> for IdentityCommand {
    type Error = RadrootsAppError;

    fn try_from(value: FfiIdentityCommandRecord) -> Result<Self, Self::Error> {
        require_schema(value.schema_version)?;
        let no_operation = value.operation_id.is_none();
        let no_identity = value.identity_id.is_none() && value.public_key.is_none();
        match value.kind {
            FfiIdentityCommandKind::BeginImport if no_identity => Ok(Self::BeginImport {
                operation_id: required(value.operation_id, "identity_operation_id_required")?,
            }),
            FfiIdentityCommandKind::CompleteImport => {
                let operation_id = required(value.operation_id, "identity_operation_id_required")?;
                let identity_id = required(value.identity_id, "identity_id_required")?;
                let public_key = required(value.public_key, "identity_public_key_required")?;
                Ok(Self::CompleteImport {
                    operation_id,
                    identity: IdentityRecord::new(identity_id, &public_key)
                        .map_err(|error| RadrootsAppError::invalid_argument(error.code()))?,
                })
            }
            FfiIdentityCommandKind::CancelImport if no_identity => Ok(Self::CancelImport {
                operation_id: required(value.operation_id, "identity_operation_id_required")?,
            }),
            FfiIdentityCommandKind::Select if no_operation && value.public_key.is_none() => {
                Ok(Self::Select {
                    identity_id: required(value.identity_id, "identity_id_required")?,
                })
            }
            FfiIdentityCommandKind::Lock if no_operation && no_identity => Ok(Self::Lock),
            FfiIdentityCommandKind::Unlock if no_operation && no_identity => Ok(Self::Unlock),
            FfiIdentityCommandKind::Recover if no_operation && no_identity => Ok(Self::Recover),
            _ => Err(RadrootsAppError::invalid_argument(
                "invalid_identity_command",
            )),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiMobileNetworkEnvironment {
    Public,
    Simulator,
    PhysicalDevice,
}

impl From<FfiMobileNetworkEnvironment> for MobileNetworkEnvironment {
    fn from(value: FfiMobileNetworkEnvironment) -> Self {
        match value {
            FfiMobileNetworkEnvironment::Public => Self::Public,
            FfiMobileNetworkEnvironment::Simulator => Self::Simulator,
            FfiMobileNetworkEnvironment::PhysicalDevice => Self::PhysicalDevice,
        }
    }
}

impl From<MobileNetworkEnvironment> for FfiMobileNetworkEnvironment {
    fn from(value: MobileNetworkEnvironment) -> Self {
        match value {
            MobileNetworkEnvironment::Public => Self::Public,
            MobileNetworkEnvironment::Simulator => Self::Simulator,
            MobileNetworkEnvironment::PhysicalDevice => Self::PhysicalDevice,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRelayAccessPreference {
    ReadOnly,
    ReadWrite,
}

impl From<FfiRelayAccessPreference> for RelayAccessPreference {
    fn from(value: FfiRelayAccessPreference) -> Self {
        match value {
            FfiRelayAccessPreference::ReadOnly => Self::ReadOnly,
            FfiRelayAccessPreference::ReadWrite => Self::ReadWrite,
        }
    }
}

impl From<RelayAccessPreference> for FfiRelayAccessPreference {
    fn from(value: RelayAccessPreference) -> Self {
        match value {
            RelayAccessPreference::ReadOnly => Self::ReadOnly,
            RelayAccessPreference::ReadWrite => Self::ReadWrite,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRelayPreferenceRecord {
    pub schema_version: u16,
    pub url: String,
    pub access: FfiRelayAccessPreference,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRelayPreferencesRecord {
    pub schema_version: u16,
    pub environment: FfiMobileNetworkEnvironment,
    pub endpoints: Vec<FfiRelayPreferenceRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiBlossomAuthorityPreference {
    PublicWebPki,
    LoopbackDevelopment,
    PrivateNetworkDevelopment,
}

impl From<FfiBlossomAuthorityPreference> for BlossomEndpointAuthorityPreference {
    fn from(value: FfiBlossomAuthorityPreference) -> Self {
        match value {
            FfiBlossomAuthorityPreference::PublicWebPki => Self::PublicWebPki,
            FfiBlossomAuthorityPreference::LoopbackDevelopment => Self::LoopbackDevelopment,
            FfiBlossomAuthorityPreference::PrivateNetworkDevelopment => {
                Self::PrivateNetworkDevelopment
            }
        }
    }
}

impl From<BlossomEndpointAuthorityPreference> for FfiBlossomAuthorityPreference {
    fn from(value: BlossomEndpointAuthorityPreference) -> Self {
        match value {
            BlossomEndpointAuthorityPreference::PublicWebPki => Self::PublicWebPki,
            BlossomEndpointAuthorityPreference::LoopbackDevelopment => Self::LoopbackDevelopment,
            BlossomEndpointAuthorityPreference::PrivateNetworkDevelopment => {
                Self::PrivateNetworkDevelopment
            }
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiBlossomPreferencesRecord {
    pub schema_version: u16,
    pub environment: FfiMobileNetworkEnvironment,
    pub authority: FfiBlossomAuthorityPreference,
    pub primary_origin: String,
    pub fallback_origins: Vec<String>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiMediaNetworkPolicyRecord {
    pub schema_version: u16,
    pub allow_cellular_downloads: bool,
    pub allow_cellular_uploads: bool,
    pub allow_background_transfers: bool,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiLocalStoragePolicyRecord {
    pub schema_version: u16,
    pub media_cache_bytes: u64,
    pub media_cache_artifacts: u32,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiMobileSettingsRecord {
    pub schema_version: u16,
    pub revision: u64,
    pub identity: FfiIdentityStateRecord,
    pub relays: FfiRelayPreferencesRecord,
    pub blossom: FfiBlossomPreferencesRecord,
    pub media_network: FfiMediaNetworkPolicyRecord,
    pub local_storage: FfiLocalStoragePolicyRecord,
}

impl From<&MobileSettings> for FfiMobileSettingsRecord {
    fn from(value: &MobileSettings) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            revision: value.revision(),
            identity: value.identity().into(),
            relays: FfiRelayPreferencesRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                environment: value.relays().environment().into(),
                endpoints: value
                    .relays()
                    .endpoints()
                    .iter()
                    .map(|endpoint| FfiRelayPreferenceRecord {
                        schema_version: MOBILE_FFI_SCHEMA_VERSION,
                        url: endpoint.url().to_owned(),
                        access: endpoint.access().into(),
                    })
                    .collect(),
            },
            blossom: FfiBlossomPreferencesRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                environment: value.blossom().environment().into(),
                authority: value.blossom().authority().into(),
                primary_origin: value.blossom().primary_origin().to_owned(),
                fallback_origins: value.blossom().fallback_origins().to_vec(),
            },
            media_network: FfiMediaNetworkPolicyRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                allow_cellular_downloads: value.media_network().allow_cellular_downloads(),
                allow_cellular_uploads: value.media_network().allow_cellular_uploads(),
                allow_background_transfers: value.media_network().allow_background_transfers(),
            },
            local_storage: FfiLocalStoragePolicyRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                media_cache_bytes: value.local_storage().media_cache_bytes(),
                media_cache_artifacts: value.local_storage().media_cache_artifacts(),
            },
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiReplaceSettingsRecord {
    pub schema_version: u16,
    pub expected_revision: u64,
    pub relays: FfiRelayPreferencesRecord,
    pub blossom: FfiBlossomPreferencesRecord,
    pub media_network: FfiMediaNetworkPolicyRecord,
    pub local_storage: FfiLocalStoragePolicyRecord,
}

impl FfiReplaceSettingsRecord {
    pub(crate) fn apply(self, current: MobileSettings) -> Result<MobileSettings, RadrootsAppError> {
        require_schema(self.schema_version)?;
        if current.revision() != self.expected_revision {
            return Err(RadrootsAppError::invalid_argument(
                "settings_revision_conflict",
            ));
        }
        require_schema(self.relays.schema_version)?;
        let relay_environment: MobileNetworkEnvironment = self.relays.environment.into();
        let relay_endpoints = self
            .relays
            .endpoints
            .into_iter()
            .map(|endpoint| {
                require_schema(endpoint.schema_version)?;
                RelayEndpointPreference::new(
                    relay_environment,
                    endpoint.url,
                    endpoint.access.into(),
                )
                .map_err(Into::into)
            })
            .collect::<Result<Vec<_>, RadrootsAppError>>()?;
        let relays = RelayPreferences::new(relay_environment, relay_endpoints)?;

        require_schema(self.blossom.schema_version)?;
        let blossom = BlossomPreferences::new(
            self.blossom.environment.into(),
            self.blossom.authority.into(),
            self.blossom.primary_origin,
            self.blossom.fallback_origins,
        )?;
        require_schema(self.media_network.schema_version)?;
        let media_network = MediaNetworkPolicy::new(
            self.media_network.allow_cellular_downloads,
            self.media_network.allow_cellular_uploads,
            self.media_network.allow_background_transfers,
        );
        require_schema(self.local_storage.schema_version)?;
        let local_storage = LocalStoragePolicy::new(
            self.local_storage.media_cache_bytes,
            self.local_storage.media_cache_artifacts,
        )?;
        Ok(current
            .with_relays(relays)
            .with_blossom(blossom)
            .with_media_network(media_network)
            .with_local_storage(local_storage))
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiSettingsTransitionRecord {
    pub schema_version: u16,
    pub settings: FfiMobileSettingsRecord,
    pub runtime_restart_required: bool,
    pub outbox_requeue_required: bool,
    pub media_cache_invalidation_required: bool,
}

impl From<SettingsTransition> for FfiSettingsTransitionRecord {
    fn from(value: SettingsTransition) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            settings: (&value.settings).into(),
            runtime_restart_required: value.runtime_restart_required,
            outbox_requeue_required: value.outbox_requeue_required,
            media_cache_invalidation_required: value.media_cache_invalidation_required,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiProfileMetadataInputRecord {
    pub schema_version: u16,
    pub name: String,
    pub display_name: Option<String>,
    pub about: Option<String>,
    pub picture: Option<FfiPreparedMediaInput>,
    pub banner: Option<FfiPreparedMediaInput>,
    pub nip05: Option<String>,
    pub bot: Option<bool>,
}

impl FfiProfileMetadataInputRecord {
    pub(crate) fn command(
        self,
        blossom: Option<&radroots_sdk::transport::BlossomSlot>,
    ) -> Result<ProfileMetadataCommand, RadrootsAppError> {
        require_schema(self.schema_version)?;
        let picture = self
            .picture
            .map(PreparedMedia::try_from)
            .transpose()?
            .map(|media| {
                let blossom = blossom.ok_or_else(|| {
                    RadrootsAppError::failure(
                        "blossom_unconfigured",
                        "profile",
                        true,
                        &["configure_blossom"],
                        "Profile media configuration is unavailable.",
                    )
                })?;
                media.into_authored_image(blossom)
            })
            .transpose()?;
        let banner = self
            .banner
            .map(PreparedMedia::try_from)
            .transpose()?
            .map(|media| {
                let blossom = blossom.ok_or_else(|| {
                    RadrootsAppError::failure(
                        "blossom_unconfigured",
                        "profile",
                        true,
                        &["configure_blossom"],
                        "Profile media configuration is unavailable.",
                    )
                })?;
                media.into_authored_image(blossom)
            })
            .transpose()?;
        ProfileMetadataCommand::new(
            self.name,
            self.display_name,
            self.about,
            picture,
            banner,
            self.nip05,
            self.bot,
        )
        .map_err(Into::into)
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiProfileStatusRecord {
    pub schema_version: u16,
    pub operation_id: String,
    pub revision: u64,
    pub author_public_key: String,
    pub state: FfiOutboxState,
    pub delivery_id: Option<String>,
    pub created_at_unix_ms: u64,
    pub updated_at_unix_ms: u64,
    pub settlement: Option<FfiOperationSettlementRecord>,
}

impl From<Phase1ProfileStatus> for FfiProfileStatusRecord {
    fn from(value: Phase1ProfileStatus) -> Self {
        let draft = value.draft();
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            operation_id: hex::encode(draft.draft_id().as_bytes()),
            revision: draft.revision().get(),
            author_public_key: hex::encode(draft.author()),
            state: value.state().into(),
            delivery_id: draft.operation_id().map(|id| hex::encode(id.as_bytes())),
            created_at_unix_ms: draft.created_at_unix_ms(),
            updated_at_unix_ms: draft.updated_at_unix_ms(),
            settlement: value.push().map(|push| push.settlement().into()),
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct FfiRevisionInputRecord {
    pub schema_version: u16,
    pub card_id: String,
    pub source_event_id: String,
    pub source_address: Option<String>,
    pub author_public_key: String,
    pub replacement: FfiAddDraftInput,
}

impl FfiRevisionInputRecord {
    pub(crate) fn target(&self) -> Result<Phase1RevisionTarget, RadrootsAppError> {
        require_schema(self.schema_version)?;
        Phase1RevisionTarget::from_source(
            self.replacement.command_type.into(),
            radroots_mobile_core::runtime::product_surface::CardId::parse(&self.card_id)
                .map_err(|_| RadrootsAppError::invalid_argument("invalid_card_id"))?,
            self.source_event_id.clone(),
            self.source_address.clone(),
            self.author_public_key.clone(),
        )
        .map_err(Into::into)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRevisionPolicy {
    ReplaceThenRetract,
    AddressableReplacement,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRevisionPhase {
    ReplacementPending,
    ReplacementFailed,
    RetractionPending,
    Complete,
    PartialEffect,
    Cancelled,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRevisionStatusRecord {
    pub schema_version: u16,
    pub operation_id: String,
    pub replacement: FfiDraftStatusRecord,
    pub retraction: Option<FfiDraftStatusRecord>,
    pub policy: FfiRevisionPolicy,
    pub phase: FfiRevisionPhase,
}

impl From<Phase1RevisionStatus> for FfiRevisionStatusRecord {
    fn from(value: Phase1RevisionStatus) -> Self {
        let operation_id = hex::encode(value.replacement().draft().draft_id().as_bytes());
        let retraction = value.retraction().cloned().map(Into::into);
        let replacement = value.replacement().clone().into();
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            operation_id,
            replacement,
            retraction,
            policy: match value.policy() {
                Phase1RevisionPolicy::ReplaceThenRetract => FfiRevisionPolicy::ReplaceThenRetract,
                Phase1RevisionPolicy::AddressableReplacement => {
                    FfiRevisionPolicy::AddressableReplacement
                }
            },
            phase: match value.phase() {
                Phase1RevisionPhase::ReplacementPending => FfiRevisionPhase::ReplacementPending,
                Phase1RevisionPhase::ReplacementFailed => FfiRevisionPhase::ReplacementFailed,
                Phase1RevisionPhase::RetractionPending => FfiRevisionPhase::RetractionPending,
                Phase1RevisionPhase::Complete => FfiRevisionPhase::Complete,
                Phase1RevisionPhase::PartialEffect => FfiRevisionPhase::PartialEffect,
                Phase1RevisionPhase::Cancelled => FfiRevisionPhase::Cancelled,
            },
        }
    }
}

#[derive(uniffi::Object)]
pub struct FfiMediaOperation {
    operation_id: [u8; 16],
    cancellation: radroots_sdk::transport::BlossomCancellation,
    claimed: std::sync::atomic::AtomicBool,
}

#[uniffi::export]
impl FfiMediaOperation {
    #[uniffi::constructor]
    pub fn new() -> Result<Self, RadrootsAppError> {
        Ok(Self {
            operation_id: phase1_new_operation_id().map_err(RadrootsAppError::from)?,
            cancellation: radroots_sdk::transport::BlossomCancellation::default(),
            claimed: std::sync::atomic::AtomicBool::new(false),
        })
    }

    pub fn operation_id(&self) -> String {
        hex::encode(self.operation_id)
    }

    pub fn cancel(&self) {
        self.cancellation.cancel();
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancellation.is_cancelled()
    }
}

impl FfiMediaOperation {
    pub(crate) fn claim(&self) -> Result<(), RadrootsAppError> {
        self.claimed
            .compare_exchange(
                false,
                true,
                std::sync::atomic::Ordering::AcqRel,
                std::sync::atomic::Ordering::Acquire,
            )
            .map(|_| ())
            .map_err(|_| RadrootsAppError::invalid_argument("media_operation_already_used"))
    }

    pub(crate) const fn id(&self) -> [u8; 16] {
        self.operation_id
    }

    pub(crate) fn cancellation(&self) -> radroots_sdk::transport::BlossomCancellation {
        self.cancellation.clone()
    }
}

#[derive(Clone, Eq, PartialEq, uniffi::Record)]
pub struct FfiVerifiedMediaArtifactRecord {
    pub schema_version: u16,
    pub operation_id: Option<String>,
    pub artifact_id: String,
    pub bytes: Vec<u8>,
    pub byte_size: u64,
    pub media_type: String,
    pub width: u32,
    pub height: u32,
}

impl std::fmt::Debug for FfiVerifiedMediaArtifactRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FfiVerifiedMediaArtifactRecord")
            .field("schema_version", &self.schema_version)
            .field("operation_id", &self.operation_id)
            .field("artifact_id", &self.artifact_id)
            .field("bytes", &"<redacted>")
            .field("byte_size", &self.byte_size)
            .field("media_type", &self.media_type)
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

impl FfiVerifiedMediaArtifactRecord {
    pub(crate) fn from_artifact(
        value: Phase1LocalMediaArtifact,
        operation_id: Option<String>,
    ) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            operation_id,
            artifact_id: value.artifact_id().to_hex(),
            bytes: value.bytes().to_vec(),
            byte_size: value.byte_size(),
            media_type: value.media_type().to_owned(),
            width: value.width(),
            height: value.height(),
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiMediaCacheStatusRecord {
    pub schema_version: u16,
    pub artifact_count: u32,
    pub total_bytes: u64,
    pub configuration_fingerprint: Option<String>,
}

impl From<Phase1MediaCacheStatus> for FfiMediaCacheStatusRecord {
    fn from(value: Phase1MediaCacheStatus) -> Self {
        Self {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            artifact_count: value.artifacts,
            total_bytes: value.bytes,
            configuration_fingerprint: value.configuration.map(|value| value.to_hex()),
        }
    }
}

pub(crate) fn require_schema(schema_version: u16) -> Result<(), RadrootsAppError> {
    if schema_version == MOBILE_FFI_SCHEMA_VERSION {
        Ok(())
    } else {
        Err(RadrootsAppError::invalid_argument(
            "unsupported_schema_version",
        ))
    }
}

fn required(value: Option<String>, code: &'static str) -> Result<String, RadrootsAppError> {
    value.ok_or_else(|| RadrootsAppError::invalid_argument(code))
}

pub(crate) fn decode_artifact_id(
    value: &str,
) -> Result<radroots_mobile_core::runtime::product_surface::Phase1MediaArtifactId, RadrootsAppError>
{
    radroots_mobile_core::runtime::product_surface::Phase1MediaArtifactId::parse(value)
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_media_artifact_id"))
}

pub(crate) fn decode_configuration(
    value: &str,
) -> Result<
    radroots_mobile_core::runtime::product_surface::Phase1MediaConfigurationFingerprint,
    RadrootsAppError,
> {
    radroots_mobile_core::runtime::product_surface::Phase1MediaConfigurationFingerprint::parse(
        value,
    )
    .map_err(|_| RadrootsAppError::invalid_argument("invalid_media_configuration"))
}

pub(crate) fn decode_reference_fingerprint(value: &str) -> Result<[u8; 32], RadrootsAppError> {
    let bytes = hex::decode(value)
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_media_reference_fingerprint"))?;
    bytes
        .try_into()
        .map_err(|_| RadrootsAppError::invalid_argument("invalid_media_reference_fingerprint"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use radroots_mobile_core::runtime::product_surface::Phase1MediaConfigurationFingerprint;

    const PUBLIC_KEY: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

    #[test]
    fn media_operation_can_be_claimed_exactly_once() {
        let operation = FfiMediaOperation::new().unwrap();
        operation.claim().unwrap();
        let error = operation.claim().unwrap_err();
        assert_eq!(error.report().code, "media_operation_already_used");
        assert_eq!(operation.operation_id().len(), 32);
    }

    #[test]
    fn verified_media_artifact_debug_never_exposes_renderable_bytes() {
        let artifact = FfiVerifiedMediaArtifactRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            operation_id: None,
            artifact_id: "11".repeat(32),
            bytes: b"private farm image".to_vec(),
            byte_size: 18,
            media_type: "image/png".to_owned(),
            width: 1,
            height: 1,
        };
        let debug = format!("{artifact:?}");
        assert!(debug.contains("<redacted>"));
        assert!(!debug.contains("private farm image"));
    }

    #[test]
    fn identity_commands_reject_fields_outside_the_selected_variant() {
        let error = IdentityCommand::try_from(FfiIdentityCommandRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            kind: FfiIdentityCommandKind::Lock,
            operation_id: Some("unexpected".to_owned()),
            identity_id: None,
            public_key: None,
        })
        .unwrap_err();
        assert_eq!(error.report().code, "invalid_identity_command");
    }

    #[test]
    fn boundary_enums_and_identity_commands_cover_the_closed_vocabularies() {
        for (ffi, core) in [
            (
                crate::FfiAddCommandType::CreateUpdate,
                AddCommandType::CreateUpdate,
            ),
            (
                crate::FfiAddCommandType::CreatePhotoUpdate,
                AddCommandType::CreatePhotoUpdate,
            ),
            (
                crate::FfiAddCommandType::CreateAsk,
                AddCommandType::CreateAsk,
            ),
            (
                crate::FfiAddCommandType::CreateEvent,
                AddCommandType::CreateEvent,
            ),
            (
                crate::FfiAddCommandType::CreateFoodAvailability,
                AddCommandType::CreateFoodAvailability,
            ),
        ] {
            assert_eq!(AddCommandType::from(ffi), core);
        }
        for environment in [
            FfiMobileNetworkEnvironment::Public,
            FfiMobileNetworkEnvironment::Simulator,
            FfiMobileNetworkEnvironment::PhysicalDevice,
        ] {
            assert_eq!(
                FfiMobileNetworkEnvironment::from(MobileNetworkEnvironment::from(environment)),
                environment
            );
        }
        for access in [
            FfiRelayAccessPreference::ReadOnly,
            FfiRelayAccessPreference::ReadWrite,
        ] {
            assert_eq!(
                FfiRelayAccessPreference::from(RelayAccessPreference::from(access)),
                access
            );
        }
        for authority in [
            FfiBlossomAuthorityPreference::PublicWebPki,
            FfiBlossomAuthorityPreference::LoopbackDevelopment,
            FfiBlossomAuthorityPreference::PrivateNetworkDevelopment,
        ] {
            assert_eq!(
                FfiBlossomAuthorityPreference::from(BlossomEndpointAuthorityPreference::from(
                    authority
                )),
                authority
            );
        }

        let commands = [
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::BeginImport,
                operation_id: Some("operation".to_owned()),
                identity_id: None,
                public_key: None,
            },
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::CompleteImport,
                operation_id: Some("operation".to_owned()),
                identity_id: Some("primary".to_owned()),
                public_key: Some(PUBLIC_KEY.to_owned()),
            },
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::CancelImport,
                operation_id: Some("operation".to_owned()),
                identity_id: None,
                public_key: None,
            },
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::Select,
                operation_id: None,
                identity_id: Some("primary".to_owned()),
                public_key: None,
            },
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::Lock,
                operation_id: None,
                identity_id: None,
                public_key: None,
            },
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::Unlock,
                operation_id: None,
                identity_id: None,
                public_key: None,
            },
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::Recover,
                operation_id: None,
                identity_id: None,
                public_key: None,
            },
        ];
        for command in commands {
            assert!(IdentityCommand::try_from(command).is_ok());
        }
        for command in [
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::BeginImport,
                operation_id: Some("operation".to_owned()),
                identity_id: Some("unexpected".to_owned()),
                public_key: None,
            },
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::CancelImport,
                operation_id: Some("operation".to_owned()),
                identity_id: None,
                public_key: Some(PUBLIC_KEY.to_owned()),
            },
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::Select,
                operation_id: Some("unexpected".to_owned()),
                identity_id: Some("primary".to_owned()),
                public_key: None,
            },
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::Select,
                operation_id: None,
                identity_id: Some("primary".to_owned()),
                public_key: Some(PUBLIC_KEY.to_owned()),
            },
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::Lock,
                operation_id: Some("unexpected".to_owned()),
                identity_id: None,
                public_key: None,
            },
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::Unlock,
                operation_id: None,
                identity_id: Some("unexpected".to_owned()),
                public_key: None,
            },
            FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                kind: FfiIdentityCommandKind::Recover,
                operation_id: None,
                identity_id: None,
                public_key: Some(PUBLIC_KEY.to_owned()),
            },
        ] {
            assert!(IdentityCommand::try_from(command).is_err());
        }
        assert!(
            IdentityCommand::try_from(FfiIdentityCommandRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION + 1,
                kind: FfiIdentityCommandKind::Lock,
                operation_id: None,
                identity_id: None,
                public_key: None,
            })
            .is_err()
        );

        let identity = IdentityRecord::new("primary", PUBLIC_KEY).unwrap();
        for lock_state in [IdentityLockState::Locked, IdentityLockState::Unlocked] {
            let state = IdentityState::new(
                vec![identity.clone()],
                Some("primary".to_owned()),
                lock_state,
                None,
            )
            .unwrap();
            let record = FfiIdentityStateRecord::from(&state);
            assert_eq!(record.identities.len(), 1);
            assert_eq!(record.lock_state, lock_state.into());
        }
    }

    #[test]
    fn media_decoders_status_and_private_operation_accessors_are_exhaustive() {
        assert!(require_schema(MOBILE_FFI_SCHEMA_VERSION).is_ok());
        assert!(require_schema(MOBILE_FFI_SCHEMA_VERSION + 1).is_err());
        for invalid in ["", "not-hex", "00"] {
            assert!(decode_artifact_id(invalid).is_err());
            assert!(decode_configuration(invalid).is_err());
            assert!(decode_reference_fingerprint(invalid).is_err());
        }
        let digest = "11".repeat(32);
        assert!(decode_artifact_id(&digest).is_ok());
        assert!(decode_configuration(&digest).is_ok());
        assert_eq!(decode_reference_fingerprint(&digest).unwrap(), [0x11; 32]);

        let configuration = Phase1MediaConfigurationFingerprint::new([7; 32]).unwrap();
        let status = FfiMediaCacheStatusRecord::from(Phase1MediaCacheStatus {
            artifacts: 2,
            bytes: 9,
            configuration: Some(configuration),
        });
        assert_eq!(status.artifact_count, 2);
        assert_eq!(status.total_bytes, 9);
        assert_eq!(status.configuration_fingerprint, Some("07".repeat(32)));
        assert!(
            FfiMediaCacheStatusRecord::from(Phase1MediaCacheStatus {
                artifacts: 0,
                bytes: 0,
                configuration: None,
            })
            .configuration_fingerprint
            .is_none()
        );

        let operation = FfiMediaOperation::new().unwrap();
        assert_eq!(operation.id(), operation.operation_id);
        assert!(!operation.cancellation().is_cancelled());
        assert!(!operation.is_cancelled());
        operation.cancel();
        assert!(operation.is_cancelled());
        assert!(operation.cancellation().is_cancelled());
    }
}
