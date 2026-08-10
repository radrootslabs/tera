//! Focused Phase 1 social-product domain for native Radroots clients.
//!
//! This module owns presentation-neutral product semantics. Protocol parsing
//! and admission remain in lower event crates; persistence and live query
//! composition remain in the runtime slices that consume these types.

mod authoring;
mod context;
mod cursor;
mod identity;
mod model;
#[cfg(feature = "mobile-social")]
mod outbox;
mod projection;
mod ranking;
#[cfg(feature = "mobile-social")]
mod settings;
mod today;

pub use authoring::{
    CreateAsk, CreateEvent, CreateFoodAvailability, CreatePhotoUpdate, CreateUpdate,
    Phase1AddCommand, Phase1ReplacementPolicy, phase1_retraction_plan,
};
pub use context::{
    ContextAdmission, ContextRank, LocalNetwork, LocalNetworkAdmission, LocalNetworkError,
    LocalNetworkRelayPolicy, LocalityEvidence,
};
pub use cursor::{CursorError, CursorScope, TodayCursor, TodayCursorPosition};
pub use identity::{CARD_ID_SCHEMA_VERSION, CardId, CardIdError, CardSourceIdentity};
pub use model::{
    AddCommandType, CANONICAL_ADD_COMMAND_TYPES, CANONICAL_CARD_ADD_PARITY,
    CANONICAL_TODAY_CARD_TYPES, CardAddParity, CardLifecycleState, ClassifiedCard,
    LocalAuthorOverlay, MeSnapshot, MediaReference, MediaVerificationState, ProfileSummary,
    SearchResult, SearchResultType, SupportingProfile, ThreadEntry, ThreadReference, TodayCard,
    TodayCardType, TodayPage,
};
#[cfg(feature = "mobile-social")]
pub use outbox::{
    Phase1AddIntent, Phase1CancellationPolicy, Phase1DraftError, Phase1DraftEventTiming,
    Phase1DraftFormSnapshot, Phase1DraftKind, Phase1DraftMediaSnapshot, Phase1DraftStatus,
    Phase1ExistingDraft, Phase1MediaOrphanRecord, Phase1MediaPrerequisite, Phase1MediaStage,
    Phase1OutboxState, Phase1QueueIntent, Phase1QueuePolicy, Phase1RelaySatisfaction,
    Phase1ReviseIntent, Phase1RevisionPhase, Phase1RevisionPolicy, Phase1RevisionStatus,
    Phase1RevisionTarget, Phase1UploadIntent, Phase1UploadPlan, phase1_new_addressable_identifier,
    phase1_operation_now_unix_ms,
};
pub use projection::{ProductEventClassification, ProductEventExclusion, classify_admitted_event};
pub use ranking::{RankError, TODAY_RANK_SCHEMA_VERSION, TimeRelevance, TodayRank, TodayRankInput};
#[cfg(feature = "mobile-social")]
pub use settings::{
    BlossomEndpointAuthorityPreference, BlossomPreferences, DEFAULT_PUBLIC_BLOSSOM_ORIGIN,
    DEFAULT_PUBLIC_RELAY, DEFAULT_SIMULATOR_BLOSSOM_ORIGIN, DEFAULT_SIMULATOR_RELAY,
    IdentityCommand, IdentityError, IdentityLockState, IdentityRecord, IdentityState,
    LocalStoragePolicy, MOBILE_SETTINGS_SCHEMA_VERSION, MediaNetworkPolicy,
    MobileNetworkEnvironment, MobileSettings, ProfileMetadataCommand, ProfileMetadataError,
    RelayAccessPreference, RelayEndpointPreference, RelayPreferences, ReplaceMobileSettings,
    SettingsError, SettingsTransition,
};
pub use today::{
    TodayError, TodayIngestReceipt, TodayPageRequest, TodayProjectionUpdate, TodayRefreshReceipt,
};
#[cfg(feature = "mobile-social")]
pub use today::{TodayRelaySyncState, TodaySyncReceipt};

use super::RadrootsRuntime;

impl RadrootsRuntime {
    /// Returns the exact five Phase 1 Today card types in contract order.
    pub fn phase1_card_types(&self) -> Vec<TodayCardType> {
        CANONICAL_TODAY_CARD_TYPES.to_vec()
    }

    /// Returns the exact five Phase 1 Add commands in card-parity order.
    pub fn phase1_add_command_types(&self) -> Vec<AddCommandType> {
        CANONICAL_ADD_COMMAND_TYPES.to_vec()
    }

    /// Returns the closed one-to-one Today/Add mapping.
    pub fn phase1_card_add_parity(&self) -> Vec<CardAddParity> {
        CANONICAL_CARD_ADD_PARITY.to_vec()
    }

    /// Constructs a validated local query/composer context.
    pub fn phase1_local_network(
        &self,
        id: String,
        label: String,
        relay_urls: Vec<String>,
        locality: Option<String>,
        followed_authors: Vec<String>,
        generation: u64,
    ) -> Result<LocalNetwork, crate::RadrootsAppError> {
        LocalNetwork::new(
            id,
            label,
            relay_urls,
            locality,
            followed_authors,
            generation,
        )
        .map_err(|error| crate::RadrootsAppError::runtime(error.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_exposes_only_the_locked_card_and_add_catalogs() {
        let runtime = RadrootsRuntime::test_memory().expect("runtime");
        assert_eq!(runtime.phase1_card_types(), CANONICAL_TODAY_CARD_TYPES);
        assert_eq!(
            runtime.phase1_add_command_types(),
            CANONICAL_ADD_COMMAND_TYPES
        );
        assert_eq!(runtime.phase1_card_add_parity(), CANONICAL_CARD_ADD_PARITY);
        assert_eq!(
            runtime
                .phase1_local_network(
                    "nearby".into(),
                    "Near me".into(),
                    vec!["wss://relay.example".into()],
                    Some("u10h".into()),
                    vec!["a".repeat(64)],
                    1,
                )
                .expect("network")
                .id,
            "nearby"
        );
        assert!(
            runtime
                .phase1_local_network(
                    "nearby".into(),
                    "Near me".into(),
                    Vec::new(),
                    None,
                    Vec::new(),
                    1,
                )
                .is_err()
        );
    }
}
