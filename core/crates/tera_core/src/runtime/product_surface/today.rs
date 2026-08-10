use std::collections::BTreeMap;

use radroots_event_codec::{
    admission::{RadrootsAdmittedEvent, admit_verified_event},
    verify::verify_nip01_event,
};
use radroots_storage::{
    EventStore, ProjectionStore,
    event::{
        AdmissionReceipt, EventAdmission, EventPosition, EventQuery, EventQueryBounds,
        EventSequence,
    },
    projection::{
        ProjectionCheckpoint, ProjectionDocument, ProjectionGeneration, ProjectionId,
        ProjectionSnapshot,
    },
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

#[cfg(feature = "mobile-social")]
use radroots_blossom::{BlobUrl, MediaType};
#[cfg(feature = "mobile-social")]
use radroots_event::admission::ContractValidatedEvent;
#[cfg(feature = "mobile-social")]
use radroots_sdk::transport::{
    BlossomCancellation, BlossomError, BlossomImageDimensions, BlossomInboundRequest,
};
#[cfg(feature = "mobile-social")]
use radroots_sync::{
    PullRequest,
    ingest::{AdmissionDecision, AdmissionPolicy},
    pull::PullTermination,
};
#[cfg(feature = "mobile-social")]
use radroots_transport::{
    Target, outcome::FetchTargetState, source::FetchSelector, target::TargetSet,
};

#[cfg(feature = "mobile-social")]
use super::Phase1LocalMediaArtifact;
use super::{
    CardId, CardLifecycleState, ClassifiedCard, CursorError, CursorScope, LocalAuthorOverlay,
    LocalNetwork, LocalityEvidence, MeSnapshot, MediaReference, Phase1InboundMediaError,
    Phase1InboundMediaFailure, Phase1InboundMediaPending, Phase1InboundMediaState,
    Phase1MediaArtifactId, Phase1MediaCacheIndex, Phase1MediaCacheStatus,
    Phase1MediaConfigurationFingerprint, Phase1StructuralMediaReference,
    ProductEventClassification, ProfileSummary, SearchResult, SearchResultType, SupportingProfile,
    ThreadEntry, ThreadReference, TimeRelevance, TodayCard, TodayCardType, TodayCursor,
    TodayCursorPosition, TodayPage, TodayRank, TodayRankInput, classify_admitted_event,
};
#[cfg(any(feature = "mobile-social", test))]
use super::{Phase1MediaCachePolicy, Phase1VerifiedMediaReceipt};
use crate::runtime::RadrootsRuntime;

const TODAY_PROJECTION_ID: &str = "radroots.today.v1";
const TODAY_PROJECTION_DOCUMENT_SCHEMA_VERSION: u16 = 1;
const TODAY_SNAPSHOT_SCHEMA_VERSION: u16 = 1;
const TODAY_PAGE_LIMIT_MAX: u16 = 100;
const TODAY_SEARCH_LIMIT_MAX: u16 = 100;
#[cfg(feature = "mobile-social")]
const TODAY_SYNC_PAGE_LIMIT: u16 = 500;
#[cfg(feature = "mobile-social")]
const TODAY_SYNC_MAX_PAGES: u16 = 8;
#[cfg(feature = "mobile-social")]
const TODAY_SYNC_KINDS: [u32; 7] = [0, 1, 5, 1111, 30_402, 31_922, 31_923];
const PROJECTION_GENERATION_DOMAIN: &[u8] = b"radroots.today-projection.v1\0";
const PROJECTION_CONTENT_DOMAIN: &[u8] = b"radroots.today-content-generation.v1\0";
const PROJECTION_DOCUMENT_KEY_DOMAIN: &[u8] = b"radroots.today-document-key.v1\0";
const SNAPSHOT_ID_DOMAIN: &[u8] = b"radroots.today-snapshot-id.v1\0";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum TodayProjectionUpdate {
    Incremental,
    Rebuild,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayRefreshReceipt {
    pub update: TodayProjectionUpdate,
    pub source_events: u64,
    pub visible_cards: u64,
    pub profiles: u64,
    pub thread_entries: u64,
    pub content_generation: u64,
    pub changed: bool,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayIngestReceipt {
    pub event_id: String,
    pub disposition: String,
    pub source_sequence: u64,
    pub projection: TodayRefreshReceipt,
}

#[cfg(feature = "mobile-social")]
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum TodayRelaySyncState {
    Complete,
    Partial,
    Offline,
}

#[cfg(feature = "mobile-social")]
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodaySyncReceipt {
    pub relay_state: TodayRelaySyncState,
    pub pages_fetched: u16,
    pub events_observed: u64,
    pub events_admitted: u64,
    pub events_rejected: u64,
    pub projection: TodayRefreshReceipt,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TodayPageRequest {
    pub limit: u16,
    pub as_of: Option<u64>,
    pub cursor: Option<String>,
}

impl TodayPageRequest {
    pub const fn first(limit: u16, as_of: u64) -> Self {
        Self {
            limit,
            as_of: Some(as_of),
            cursor: None,
        }
    }

    pub fn after(limit: u16, cursor: String) -> Self {
        Self {
            limit,
            as_of: None,
            cursor: Some(cursor),
        }
    }
}

#[derive(Debug, Error)]
pub enum TodayError {
    #[error("today runtime is unavailable")]
    RuntimeUnavailable,
    #[error("today request is invalid")]
    InvalidRequest,
    #[error("today projection has not been refreshed")]
    ProjectionMissing,
    #[error("today frozen snapshot is unavailable")]
    SnapshotMissing,
    #[error("today cursor position is absent from its frozen snapshot")]
    CursorPositionMissing,
    #[error("today event was not admitted as visible")]
    EventNotVisible,
    #[error("today projection state is corrupt")]
    CorruptProjection,
    #[error(transparent)]
    Cursor(#[from] CursorError),
    #[error(transparent)]
    Storage(#[from] radroots_storage::Error),
    #[error("today projection serialization failed")]
    Serialization,
    #[error(transparent)]
    InboundMedia(#[from] Phase1InboundMediaError),
    #[cfg(feature = "mobile-social")]
    #[error(transparent)]
    InboundRetrieval(#[from] BlossomError),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ProjectedCard {
    card: ClassifiedCard,
    locality: Vec<LocalityTag>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Ord, PartialOrd, Serialize)]
#[serde(rename_all = "camelCase")]
struct LocalityTag {
    kind: String,
    value: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct TodayProjectionState {
    schema_version: u16,
    context_id: String,
    context_generation: u64,
    store_generation: [u8; 32],
    source_events: u64,
    content_generation: u64,
    cards: Vec<ProjectedCard>,
    profiles: BTreeMap<String, ProfileSummary>,
    thread: Vec<ThreadEntry>,
    overlays: BTreeMap<String, LocalAuthorOverlay>,
    #[serde(default)]
    media_cache: Phase1MediaCacheIndex,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct FrozenTodaySnapshot {
    schema_version: u16,
    context_id: String,
    context_generation: u64,
    as_of: u64,
    store_generation: [u8; 32],
    projection_generation: u64,
    items: Vec<TodayCard>,
}

impl RadrootsRuntime {
    /// Pulls bounded Today-relevant relay pages, canonically admits valid
    /// observations, and materializes the selected LocalNetwork projection.
    #[cfg(feature = "mobile-social")]
    pub async fn phase1_sync_today(
        &self,
        context: &LocalNetwork,
        now_unix_seconds: u64,
        update: TodayProjectionUpdate,
    ) -> Result<TodaySyncReceipt, TodayError> {
        if now_unix_seconds == 0 {
            return Err(TodayError::InvalidRequest);
        }
        let targets = context
            .relay_urls
            .iter()
            .map(Target::nostr_relay)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| TodayError::InvalidRequest)?;
        let targets = TargetSet::new(targets).map_err(|_| TodayError::InvalidRequest)?;
        let selector = FetchSelector::all()
            .with_kinds(TODAY_SYNC_KINDS.to_vec())
            .map_err(|_| TodayError::InvalidRequest)?;
        let request = PullRequest::new(targets, TODAY_SYNC_PAGE_LIMIT, TODAY_SYNC_MAX_PAGES)
            .map_err(|_| TodayError::RuntimeUnavailable)?
            .with_selector(selector);
        let sync = self
            .client
            .sync()
            .map_err(|_| TodayError::RuntimeUnavailable)?
            .ok_or(TodayError::RuntimeUnavailable)?;
        let pull = sync
            .pull(request, &TodayAdmissionPolicy)
            .await
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let projection = self
            .phase1_refresh_today(context, now_unix_seconds, update)
            .await?;
        let events_admitted = pull
            .ingest_outcomes()
            .iter()
            .filter(|outcome| outcome.is_ok())
            .count() as u64;
        let events_observed = u64::try_from(pull.events_observed()).unwrap_or(u64::MAX);
        let events_rejected = events_observed.saturating_sub(events_admitted);
        let target_complete = !pull.target_outcomes().is_empty()
            && pull
                .target_outcomes()
                .iter()
                .all(|outcome| outcome.state() == FetchTargetState::Complete);
        let relay_state = match pull.termination() {
            PullTermination::Complete if target_complete => TodayRelaySyncState::Complete,
            PullTermination::SourceFailed if pull.pages_fetched() == 0 => {
                TodayRelaySyncState::Offline
            }
            _ => TodayRelaySyncState::Partial,
        };
        Ok(TodaySyncReceipt {
            relay_state,
            pages_fetched: pull.pages_fetched(),
            events_observed,
            events_admitted,
            events_rejected,
            projection,
        })
    }

    /// Durably admits one already verified and visibility-authorized relay observation,
    /// then advances the selected LocalNetwork projection.
    pub async fn phase1_ingest_visible(
        &self,
        admission: EventAdmission,
        context: &LocalNetwork,
        now_unix_seconds: u64,
    ) -> Result<TodayIngestReceipt, TodayError> {
        if admission.visible_event().is_none() {
            return Err(TodayError::EventNotVisible);
        }
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let receipt = EventStore::admit(storage, admission).await?;
        let projection = self
            .phase1_refresh_today(
                context,
                now_unix_seconds,
                TodayProjectionUpdate::Incremental,
            )
            .await?;
        Ok(ingest_receipt(receipt, projection))
    }

    /// Materializes current visible event truth for one LocalNetwork.
    pub async fn phase1_refresh_today(
        &self,
        context: &LocalNetwork,
        now_unix_seconds: u64,
        update: TodayProjectionUpdate,
    ) -> Result<TodayRefreshReceipt, TodayError> {
        if now_unix_seconds == 0 {
            return Err(TodayError::InvalidRequest);
        }
        let requested_updated_at_unix_ms = now_unix_seconds
            .checked_mul(1_000)
            .ok_or(TodayError::InvalidRequest)?;
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        if update == TodayProjectionUpdate::Rebuild {
            EventStore::rebuild_visibility(storage).await?;
        }
        let event_status = EventStore::status(storage).await?;
        let generation = projection_generation()?;
        let projection_id = projection_id()?;
        let key = projection_document_key(context);
        let prior = load_state(storage, context, generation).await?;

        if update == TodayProjectionUpdate::Incremental
            && prior
                .as_ref()
                .is_some_and(|state| state.source_events == event_status.raw_events())
        {
            let state = prior.expect("checked present");
            return Ok(refresh_receipt(update, &state, false));
        }

        let visible = query_all_visible(storage).await?;
        let local_media = prior.as_ref().map(local_media_evidence).unwrap_or_default();
        let overlays = prior
            .as_ref()
            .map_or_else(BTreeMap::new, |state| state.overlays.clone());
        let media_cache =
            prior.map_or_else(Phase1MediaCacheIndex::default, |state| state.media_cache);
        let mut state = project_state(
            context,
            event_status.generation().as_bytes(),
            event_status.raw_events(),
            visible,
            overlays,
        )?;
        state.media_cache = media_cache;
        apply_local_media_evidence(&mut state, &local_media);
        state.content_generation = content_generation(&state)?;
        let encoded = encode(&state)?;
        let changed = ProjectionStore::projection_document(
            storage,
            projection_id.clone(),
            generation,
            key.clone(),
        )
        .await?
        .is_none_or(|document| document.value() != encoded);
        ProjectionStore::put_projection_document(
            storage,
            projection_id.clone(),
            generation,
            ProjectionDocument::new(key, encoded)?,
        )
        .await?;

        let source_position = if event_status.raw_events() == 0 {
            None
        } else {
            Some(EventPosition::new(
                event_status.generation(),
                EventSequence::new(event_status.raw_events())?,
            ))
        };
        let prior_updated_at = ProjectionStore::status(storage, projection_id.clone())
            .await?
            .and_then(|status| {
                status
                    .checkpoint()
                    .map(ProjectionCheckpoint::updated_at_unix_ms)
            })
            .unwrap_or(0);
        let updated_at_unix_ms = requested_updated_at_unix_ms.max(prior_updated_at);
        ProjectionStore::checkpoint(
            storage,
            ProjectionCheckpoint::new(
                projection_id,
                generation,
                source_position,
                event_status.raw_events(),
                updated_at_unix_ms,
            )?,
        )
        .await?;
        Ok(refresh_receipt(update, &state, changed))
    }

    /// Returns one page from a durable frozen Today snapshot.
    pub async fn phase1_today_page(
        &self,
        context: &LocalNetwork,
        request: TodayPageRequest,
    ) -> Result<TodayPage, TodayError> {
        if request.limit == 0 || request.limit > TODAY_PAGE_LIMIT_MAX {
            return Err(TodayError::InvalidRequest);
        }
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let event_status = EventStore::status(storage).await?;
        let algorithm_generation = projection_generation()?;
        let projection_id = projection_id()?;

        let (scope, snapshot, after) = if let Some(cursor) = request.cursor.as_deref() {
            let scope = TodayCursor::scope(cursor)?;
            if scope.context_id != context.id || scope.context_generation != context.generation {
                return Err(CursorError::ContextMismatch.into());
            }
            if request.as_of.is_some_and(|as_of| as_of != scope.as_of) {
                return Err(CursorError::SnapshotMismatch.into());
            }
            if scope.store_generation != *event_status.generation().as_bytes() {
                return Err(CursorError::Stale.into());
            }
            let position = TodayCursor::decode(cursor, &scope)?;
            let mut snapshot = load_snapshot(storage, projection_id, algorithm_generation, &scope)
                .await?
                .ok_or(TodayError::SnapshotMissing)?;
            let current = load_state(storage, context, algorithm_generation)
                .await?
                .ok_or(TodayError::ProjectionMissing)?;
            sanitize_snapshot_media(&mut snapshot, &current.media_cache);
            (scope, snapshot, Some(position.rank))
        } else {
            let as_of = request
                .as_of
                .filter(|value| *value != 0)
                .ok_or(TodayError::InvalidRequest)?;
            let state = load_state(storage, context, algorithm_generation)
                .await?
                .ok_or(TodayError::ProjectionMissing)?;
            if state.store_generation != *event_status.generation().as_bytes() {
                return Err(CursorError::Stale.into());
            }
            let scope = CursorScope::new(
                context.id.clone(),
                context.generation,
                as_of,
                state.store_generation,
                state.content_generation,
            )?;
            let snapshot = frozen_snapshot(&state, context, as_of)?;
            persist_snapshot(storage, algorithm_generation, &scope, &snapshot).await?;
            (scope, snapshot, None)
        };

        page_from_snapshot(snapshot, scope, after, request.limit)
    }

    /// Searches the current local projection using Today visibility and context rules.
    pub async fn phase1_search(
        &self,
        context: &LocalNetwork,
        query: &str,
        limit: u16,
        as_of: u64,
    ) -> Result<Vec<SearchResult>, TodayError> {
        if limit == 0 || limit > TODAY_SEARCH_LIMIT_MAX || as_of == 0 {
            return Err(TodayError::InvalidRequest);
        }
        let needle = query.trim().to_lowercase();
        if needle.is_empty() || needle.len() > 256 || query.chars().any(char::is_control) {
            return Err(TodayError::InvalidRequest);
        }
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let state = load_state(storage, context, projection_generation()?)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        let cards = ranked_cards(&state, context, as_of)?;
        let mut results = Vec::new();
        for card in cards {
            let searchable = format!(
                "{} {} {} {}",
                card.card.title.as_deref().unwrap_or(""),
                card.card.content,
                card.card.author_pubkey,
                card.author_profile
                    .as_ref()
                    .and_then(|profile| profile.display_name.as_deref().or(profile.name.as_deref()))
                    .unwrap_or("")
            )
            .to_lowercase();
            if searchable.contains(&needle) {
                results.push(SearchResult {
                    result_type: SearchResultType::Card,
                    stable_id: card.card.card_id.to_hex(),
                    card: Some(card),
                    profile: None,
                });
                if results.len() == usize::from(limit) {
                    return Ok(results);
                }
            }
        }
        for profile in state.profiles.values() {
            let searchable = format!(
                "{} {} {} {} {}",
                profile.name.as_deref().unwrap_or(""),
                profile.display_name.as_deref().unwrap_or(""),
                profile.about.as_deref().unwrap_or(""),
                profile.website.as_deref().unwrap_or(""),
                profile.lightning_address.as_deref().unwrap_or("")
            )
            .to_lowercase();
            if searchable.contains(&needle) {
                results.push(SearchResult {
                    result_type: SearchResultType::Profile,
                    stable_id: profile.author_pubkey.clone(),
                    card: None,
                    profile: Some(profile.clone()),
                });
                if results.len() == usize::from(limit) {
                    break;
                }
            }
        }
        Ok(results)
    }

    /// Returns current active-identity attribution and visible Phase 1 content.
    pub async fn phase1_me(
        &self,
        context: &LocalNetwork,
        public_key: &str,
        as_of: u64,
    ) -> Result<MeSnapshot, TodayError> {
        if !valid_public_key(public_key) || as_of == 0 {
            return Err(TodayError::InvalidRequest);
        }
        if self
            .authenticated_store_public_key_hex()
            .is_some_and(|store_key| store_key != public_key)
        {
            return Err(TodayError::InvalidRequest);
        }
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let state = load_state(storage, context, projection_generation()?)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        let cards = ranked_cards(&state, context, as_of)?
            .into_iter()
            .filter(|card| card.card.author_pubkey == public_key)
            .collect();
        Ok(MeSnapshot {
            public_key: public_key.to_owned(),
            profile: state.profiles.get(public_key).cloned(),
            cards,
        })
    }

    /// Starts one typed retrieval for every occurrence of the exact structural
    /// reference. URL equality alone is deliberately insufficient.
    pub async fn phase1_begin_media_retrieval(
        &self,
        context: &LocalNetwork,
        reference_fingerprint: [u8; 32],
        pending: Phase1InboundMediaPending,
    ) -> Result<bool, TodayError> {
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let generation = projection_generation()?;
        let mut state = load_state(storage, context, generation)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        let mut trial = state.clone();
        let prior_configuration = trial.media_cache.status()?.configuration;
        if prior_configuration.is_some_and(|value| value != pending.configuration()) {
            return Err(Phase1InboundMediaError::ConfigurationMismatch.into());
        }
        trial
            .media_cache
            .invalidate_configuration(pending.configuration());
        let changed = mutate_matching_media(&mut trial, reference_fingerprint, |media| {
            media.begin(pending.clone())
        })?;
        if changed {
            state = trial;
            persist_media_state(storage, context, generation, &mut state).await?;
        }
        Ok(changed)
    }

    /// Records a bounded, safe retrieval failure for the active operation.
    pub async fn phase1_fail_media_retrieval(
        &self,
        context: &LocalNetwork,
        reference_fingerprint: [u8; 32],
        failure: Phase1InboundMediaFailure,
    ) -> Result<bool, TodayError> {
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let generation = projection_generation()?;
        let mut state = load_state(storage, context, generation)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        let changed = mutate_matching_media(&mut state, reference_fingerprint, |media| {
            media.fail(failure.clone())
        })?;
        if changed {
            persist_media_state(storage, context, generation, &mut state).await?;
        }
        Ok(changed)
    }

    /// Atomically binds exact-byte evidence to the matching reference and
    /// admits its content-addressed cache entry under the active LRU quota.
    #[cfg(any(feature = "mobile-social", test))]
    pub(crate) async fn phase1_commit_media_receipt(
        &self,
        context: &LocalNetwork,
        reference_fingerprint: [u8; 32],
        operation_id: [u8; 16],
        receipt: Phase1VerifiedMediaReceipt,
        policy: Phase1MediaCachePolicy,
        cached_at_unix_ms: u64,
    ) -> Result<Vec<Phase1MediaArtifactId>, TodayError> {
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let generation = projection_generation()?;
        let mut state = load_state(storage, context, generation)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        let mut trial = state.clone();
        let changed = mutate_matching_media(&mut trial, reference_fingerprint, |media| {
            media.verify(operation_id, receipt.clone())
        })?;
        if !changed {
            return Err(TodayError::InvalidRequest);
        }
        let evicted = trial
            .media_cache
            .admit(&receipt, policy, cached_at_unix_ms)?;
        for artifact_id in &evicted {
            invalidate_artifact_references(&mut trial, *artifact_id);
        }
        state = trial;
        persist_media_state(storage, context, generation, &mut state).await?;
        Ok(evicted)
    }

    /// Records one successful local artifact access for deterministic LRU.
    pub async fn phase1_touch_media_artifact(
        &self,
        context: &LocalNetwork,
        artifact_id: Phase1MediaArtifactId,
        observed_at_unix_ms: u64,
    ) -> Result<bool, TodayError> {
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let generation = projection_generation()?;
        let mut state = load_state(storage, context, generation)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        let changed = state.media_cache.touch(artifact_id, observed_at_unix_ms)?;
        if changed {
            persist_media_state(storage, context, generation, &mut state).await?;
        }
        Ok(changed)
    }

    /// Invalidates a missing, corrupt, or explicitly evicted local artifact.
    pub async fn phase1_invalidate_media_artifact(
        &self,
        context: &LocalNetwork,
        artifact_id: Phase1MediaArtifactId,
    ) -> Result<bool, TodayError> {
        #[cfg(feature = "mobile-social")]
        let _guard = self.inbound_media_lock.lock().await;
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let generation = projection_generation()?;
        let mut state = load_state(storage, context, generation)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        let cache_changed = state.media_cache.invalidate_artifact(artifact_id);
        let references_changed = invalidate_artifact_references(&mut state, artifact_id);
        if cache_changed || references_changed {
            persist_media_state(storage, context, generation, &mut state).await?;
        }
        #[cfg(feature = "mobile-social")]
        if let Some(directory) = self.inbound_media_directory.as_deref() {
            super::media::remove_artifact_files(directory, artifact_id).await?;
        }
        Ok(cache_changed || references_changed)
    }

    /// Clears all trust derived under an obsolete endpoint/network/cache
    /// configuration and records the new configuration generation.
    pub async fn phase1_invalidate_media_configuration(
        &self,
        context: &LocalNetwork,
        configuration: Phase1MediaConfigurationFingerprint,
    ) -> Result<Vec<Phase1MediaArtifactId>, TodayError> {
        #[cfg(feature = "mobile-social")]
        let _guard = self.inbound_media_lock.lock().await;
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let generation = projection_generation()?;
        let mut state = load_state(storage, context, generation)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        let prior_configuration = state.media_cache.status()?.configuration;
        let removed = state.media_cache.invalidate_configuration(configuration);
        if prior_configuration != Some(configuration) {
            for_each_media_mut(&mut state, |media| {
                media.invalidate();
            });
            persist_media_state(storage, context, generation, &mut state).await?;
        }
        #[cfg(feature = "mobile-social")]
        if let Some(directory) = self.inbound_media_directory.as_deref() {
            for artifact_id in &removed {
                super::media::remove_artifact_files(directory, *artifact_id).await?;
            }
        }
        Ok(removed)
    }

    pub async fn phase1_media_cache_status(
        &self,
        context: &LocalNetwork,
    ) -> Result<Phase1MediaCacheStatus, TodayError> {
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let state = load_state(storage, context, projection_generation()?)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        state.media_cache.status().map_err(TodayError::from)
    }

    /// Resolves one renderable artifact only after rechecking the exact local
    /// file. Missing or corrupt bytes atomically revoke all matching receipts.
    #[cfg(feature = "mobile-social")]
    pub async fn phase1_verified_media_artifact(
        &self,
        context: &LocalNetwork,
        artifact_id: Phase1MediaArtifactId,
        observed_at_unix_ms: u64,
    ) -> Result<Option<Phase1LocalMediaArtifact>, TodayError> {
        let _guard = self.inbound_media_lock.lock().await;
        let directory = self
            .inbound_media_directory
            .as_deref()
            .ok_or(Phase1InboundMediaError::CacheUnavailable)?;
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let generation = projection_generation()?;
        let mut state = load_state(storage, context, generation)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        let Some(receipt) = verified_receipt(&state, artifact_id) else {
            return Ok(None);
        };
        match super::media::verified_artifact(directory, &receipt).await {
            Ok(artifact) => {
                if state.media_cache.touch(artifact_id, observed_at_unix_ms)? {
                    persist_media_state(storage, context, generation, &mut state).await?;
                }
                Ok(Some(artifact))
            }
            Err(error) => {
                state.media_cache.invalidate_artifact(artifact_id);
                invalidate_artifact_references(&mut state, artifact_id);
                persist_media_state(storage, context, generation, &mut state).await?;
                let _ = super::media::remove_artifact_files(directory, artifact_id).await;
                Err(error.into())
            }
        }
    }

    /// Completes one bounded BUD-01 retrieval, exact-byte verification, and
    /// atomic content-addressed cache commit under the configured Blossom slot.
    #[cfg(feature = "mobile-social")]
    pub async fn phase1_retrieve_media(
        &self,
        context: &LocalNetwork,
        reference_fingerprint: [u8; 32],
        operation_id: [u8; 16],
        policy: Phase1MediaCachePolicy,
        cancellation: BlossomCancellation,
    ) -> Result<Phase1LocalMediaArtifact, TodayError> {
        let _guard = self.inbound_media_lock.lock().await;
        let directory = self
            .inbound_media_directory
            .as_deref()
            .ok_or(Phase1InboundMediaError::CacheUnavailable)?;
        let blossom = self
            .client
            .blossom()
            .map_err(|_| TodayError::RuntimeUnavailable)?
            .cloned()
            .ok_or(TodayError::RuntimeUnavailable)?;
        let sdk_configuration = blossom
            .config_fingerprint()
            .ok_or(TodayError::RuntimeUnavailable)?;
        let configuration =
            Phase1MediaConfigurationFingerprint::new(*sdk_configuration.as_bytes())?;
        let structural = load_structural_reference(self, context, reference_fingerprint).await?;
        let started_at_unix_ms = inbound_now_unix_ms()?;
        self.phase1_begin_media_retrieval(
            context,
            reference_fingerprint,
            Phase1InboundMediaPending::new(operation_id, configuration, started_at_unix_ms)?,
        )
        .await?;
        let request = match inbound_request(&structural) {
            Ok(request) => request,
            Err(error) => {
                record_inbound_failure(
                    self,
                    context,
                    reference_fingerprint,
                    operation_id,
                    "invalid_reference",
                    false,
                )
                .await;
                return Err(error);
            }
        };
        let sdk_receipt = match blossom.retrieve(request, cancellation).await {
            Ok(receipt) => receipt,
            Err(error) => {
                record_inbound_failure(
                    self,
                    context,
                    reference_fingerprint,
                    operation_id,
                    error.code().trim_start_matches("blossom_"),
                    error.retryable(),
                )
                .await;
                return Err(error.into());
            }
        };
        if sdk_receipt.config_fingerprint() != sdk_configuration {
            record_inbound_failure(
                self,
                context,
                reference_fingerprint,
                operation_id,
                "configuration_changed",
                false,
            )
            .await;
            return Err(Phase1InboundMediaError::ConfigurationMismatch.into());
        }
        let dimensions = sdk_receipt.dimensions();
        let receipt = match Phase1VerifiedMediaReceipt::from_commitment(
            &structural,
            sdk_receipt.final_url().clone(),
            sdk_receipt.commitment(),
            dimensions.width(),
            dimensions.height(),
            configuration,
            sdk_receipt.verified_at_unix_ms(),
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                record_inbound_failure(
                    self,
                    context,
                    reference_fingerprint,
                    operation_id,
                    "verification_failed",
                    false,
                )
                .await;
                return Err(error.into());
            }
        };
        let artifact =
            match super::media::write_verified_artifact(directory, &receipt, sdk_receipt.bytes())
                .await
            {
                Ok(artifact) => artifact,
                Err(error) => {
                    record_inbound_failure(
                        self,
                        context,
                        reference_fingerprint,
                        operation_id,
                        "cache_write_failed",
                        true,
                    )
                    .await;
                    return Err(error.into());
                }
            };
        let evicted = match self
            .phase1_commit_media_receipt(
                context,
                reference_fingerprint,
                operation_id,
                receipt,
                policy,
                sdk_receipt.verified_at_unix_ms(),
            )
            .await
        {
            Ok(evicted) => evicted,
            Err(error) => {
                record_inbound_failure(
                    self,
                    context,
                    reference_fingerprint,
                    operation_id,
                    "cache_commit_failed",
                    true,
                )
                .await;
                return Err(error);
            }
        };
        for artifact_id in evicted {
            super::media::remove_artifact_files(directory, artifact_id).await?;
        }
        Ok(artifact)
    }

    /// Persists active-author delivery state as a local-only Today overlay.
    pub async fn phase1_set_local_author_overlay(
        &self,
        context: &LocalNetwork,
        card_id: CardId,
        overlay: Option<LocalAuthorOverlay>,
    ) -> Result<(), TodayError> {
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let generation = projection_generation()?;
        let mut state = load_state(storage, context, generation)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        if overlay.as_ref().is_some_and(|overlay| {
            overlay.operation_id.is_empty()
                || overlay.operation_id.len() > 256
                || overlay.state.is_empty()
                || overlay.state.len() > 96
                || overlay.state.chars().any(char::is_control)
        }) {
            return Err(TodayError::InvalidRequest);
        }
        let key = card_id.to_hex();
        let card = state
            .cards
            .iter()
            .find(|projected| projected.card.card_id == card_id)
            .ok_or(TodayError::InvalidRequest)?;
        if self
            .authenticated_store_public_key_hex()
            .is_some_and(|store_key| store_key != card.card.author_pubkey)
        {
            return Err(TodayError::InvalidRequest);
        }
        if let Some(overlay) = overlay {
            state.overlays.insert(key, overlay);
        } else {
            state.overlays.remove(&key);
        }
        state.content_generation = content_generation(&state)?;
        store_state(storage, context, generation, &state).await
    }
}

#[cfg(feature = "mobile-social")]
async fn load_structural_reference(
    runtime: &RadrootsRuntime,
    context: &LocalNetwork,
    reference_fingerprint: [u8; 32],
) -> Result<Phase1StructuralMediaReference, TodayError> {
    let storage = runtime
        .client
        .storage()
        .map_err(|_| TodayError::RuntimeUnavailable)?;
    let state = load_state(storage, context, projection_generation()?)
        .await?
        .ok_or(TodayError::ProjectionMissing)?;
    state
        .cards
        .iter()
        .flat_map(|projected| projected.card.media.iter())
        .chain(
            state
                .profiles
                .values()
                .flat_map(|profile| [&profile.picture, &profile.banner].into_iter().flatten()),
        )
        .find(|media| media.structural().fingerprint() == &reference_fingerprint)
        .map(|media| media.structural().clone())
        .ok_or(TodayError::InvalidRequest)
}

#[cfg(feature = "mobile-social")]
fn inbound_request(
    structural: &Phase1StructuralMediaReference,
) -> Result<BlossomInboundRequest, TodayError> {
    let url = BlobUrl::parse(structural.source_url())
        .map_err(|_| Phase1InboundMediaError::InvalidReference)?;
    let media_type = structural
        .expected_media_type()
        .map(MediaType::parse)
        .transpose()
        .map_err(|_| Phase1InboundMediaError::InvalidMediaType)?;
    let dimensions = match (structural.expected_width(), structural.expected_height()) {
        (Some(width), Some(height)) => Some(BlossomImageDimensions::new(width, height)?),
        (None, None) => None,
        _ => return Err(Phase1InboundMediaError::InvalidDimensions.into()),
    };
    BlossomInboundRequest::new(url, media_type, structural.expected_byte_size(), dimensions)
        .map_err(TodayError::from)
}

#[cfg(feature = "mobile-social")]
fn inbound_now_unix_ms() -> Result<u64, TodayError> {
    use std::time::{SystemTime, UNIX_EPOCH};

    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| u64::try_from(duration.as_millis()).ok())
        .filter(|value| *value != 0)
        .ok_or(TodayError::RuntimeUnavailable)
}

#[cfg(feature = "mobile-social")]
async fn record_inbound_failure(
    runtime: &RadrootsRuntime,
    context: &LocalNetwork,
    reference_fingerprint: [u8; 32],
    operation_id: [u8; 16],
    safe_code: &str,
    retryable: bool,
) {
    let Ok(failed_at_unix_ms) = inbound_now_unix_ms() else {
        return;
    };
    let Ok(failure) =
        Phase1InboundMediaFailure::new(operation_id, safe_code, retryable, failed_at_unix_ms)
    else {
        return;
    };
    let _ = runtime
        .phase1_fail_media_retrieval(context, reference_fingerprint, failure)
        .await;
}

#[cfg(feature = "mobile-social")]
struct TodayAdmissionPolicy;

#[cfg(feature = "mobile-social")]
impl AdmissionPolicy for TodayAdmissionPolicy {
    fn policy_id(&self) -> &'static str {
        "radroots.mobile.today.v1"
    }

    fn select_contract(
        &self,
        event: &radroots_event::admission::SignatureVerifiedEvent,
    ) -> Option<&'static str> {
        verify_nip01_event(event.event().clone())
            .ok()
            .and_then(|event| admit_verified_event(event).ok())
            .map(|event| event.contract().id)
    }

    fn decide(&self, event: &ContractValidatedEvent) -> AdmissionDecision {
        let admitted = verify_nip01_event(event.event().clone())
            .ok()
            .and_then(|event| admit_verified_event(event).ok());
        if admitted.is_some() {
            AdmissionDecision::Visible
        } else {
            AdmissionDecision::Reject
        }
    }
}

fn ingest_receipt(
    receipt: AdmissionReceipt,
    projection: TodayRefreshReceipt,
) -> TodayIngestReceipt {
    TodayIngestReceipt {
        event_id: receipt.event_id().to_hex(),
        disposition: format!("{:?}", receipt.disposition()).to_lowercase(),
        source_sequence: receipt.position().sequence().get(),
        projection,
    }
}

fn refresh_receipt(
    update: TodayProjectionUpdate,
    state: &TodayProjectionState,
    changed: bool,
) -> TodayRefreshReceipt {
    TodayRefreshReceipt {
        update,
        source_events: state.source_events,
        visible_cards: state.cards.len().try_into().unwrap_or(u64::MAX),
        profiles: state.profiles.len().try_into().unwrap_or(u64::MAX),
        thread_entries: state.thread.len().try_into().unwrap_or(u64::MAX),
        content_generation: state.content_generation,
        changed,
    }
}

fn local_media_evidence(
    state: &TodayProjectionState,
) -> BTreeMap<[u8; 32], Phase1InboundMediaState> {
    let mut evidence = state
        .cards
        .iter()
        .flat_map(|projected| projected.card.media.iter())
        .filter(|media| !matches!(media.retrieval(), Phase1InboundMediaState::Unavailable))
        .map(|media| (*media.structural().fingerprint(), media.retrieval().clone()))
        .collect::<BTreeMap<_, _>>();
    for profile in state.profiles.values() {
        for media in [&profile.picture, &profile.banner].into_iter().flatten() {
            if !matches!(media.retrieval(), Phase1InboundMediaState::Unavailable) {
                evidence.insert(*media.structural().fingerprint(), media.retrieval().clone());
            }
        }
    }
    evidence
}

fn apply_local_media_evidence(
    state: &mut TodayProjectionState,
    evidence: &BTreeMap<[u8; 32], Phase1InboundMediaState>,
) {
    let cache = state.media_cache.clone();
    for_each_media_mut(state, |media| {
        if let Some(retrieval) = evidence.get(media.structural().fingerprint())
            && media.restore(retrieval.clone(), &cache).is_err()
        {
            media.invalidate();
        }
    });
    refresh_thread_profiles(state);
}

fn for_each_media_mut(
    state: &mut TodayProjectionState,
    mut action: impl FnMut(&mut MediaReference),
) {
    for projected in &mut state.cards {
        for media in &mut projected.card.media {
            action(media);
        }
    }
    for profile in state.profiles.values_mut() {
        for media in [&mut profile.picture, &mut profile.banner]
            .into_iter()
            .flatten()
        {
            action(media);
        }
    }
}

fn mutate_matching_media(
    state: &mut TodayProjectionState,
    reference_fingerprint: [u8; 32],
    mut action: impl FnMut(&mut MediaReference) -> Result<(), Phase1InboundMediaError>,
) -> Result<bool, TodayError> {
    let mut found = false;
    let mut failure = None;
    for_each_media_mut(state, |media| {
        if failure.is_none() && media.structural().fingerprint() == &reference_fingerprint {
            found = true;
            if let Err(error) = action(media) {
                failure = Some(error);
            }
        }
    });
    if let Some(error) = failure {
        return Err(error.into());
    }
    if found {
        refresh_thread_profiles(state);
    }
    Ok(found)
}

fn invalidate_artifact_references(
    state: &mut TodayProjectionState,
    artifact_id: Phase1MediaArtifactId,
) -> bool {
    let mut changed = false;
    for_each_media_mut(state, |media| {
        if matches!(
            media.retrieval(),
            Phase1InboundMediaState::Verified(receipt) if receipt.artifact_id() == artifact_id
        ) {
            media.invalidate();
            changed = true;
        }
    });
    if changed {
        refresh_thread_profiles(state);
    }
    changed
}

#[cfg(feature = "mobile-social")]
fn verified_receipt(
    state: &TodayProjectionState,
    artifact_id: Phase1MediaArtifactId,
) -> Option<Phase1VerifiedMediaReceipt> {
    state
        .cards
        .iter()
        .flat_map(|projected| projected.card.media.iter())
        .chain(
            state
                .profiles
                .values()
                .flat_map(|profile| [&profile.picture, &profile.banner].into_iter().flatten()),
        )
        .find_map(|media| match media.retrieval() {
            Phase1InboundMediaState::Verified(receipt) if receipt.artifact_id() == artifact_id => {
                Some((**receipt).clone())
            }
            _ => None,
        })
}

async fn persist_media_state(
    storage: &dyn radroots_storage::Storage,
    context: &LocalNetwork,
    generation: ProjectionGeneration,
    state: &mut TodayProjectionState,
) -> Result<(), TodayError> {
    refresh_thread_profiles(state);
    validate_media_state(state)?;
    state.content_generation = content_generation(state)?;
    store_state(storage, context, generation, state).await
}

fn refresh_thread_profiles(state: &mut TodayProjectionState) {
    for entry in &mut state.thread {
        entry.author_profile = state.profiles.get(&entry.author_pubkey).cloned();
    }
}

async fn query_all_visible(
    storage: &dyn radroots_storage::Storage,
) -> Result<Vec<radroots_storage::event::StoredVisibleEvent>, TodayError> {
    let mut items = Vec::new();
    let mut after = None;
    loop {
        let mut bounds = EventQueryBounds::first(radroots_storage::event::EVENT_QUERY_LIMIT_MAX)?;
        if let Some(cursor) = after {
            bounds = bounds.after(cursor);
        }
        let page = EventStore::query_visible(storage, EventQuery::all(bounds)).await?;
        items.extend_from_slice(page.items());
        let Some(next) = page.next_cursor() else {
            break;
        };
        after = Some(next);
    }
    Ok(items)
}

fn project_state(
    context: &LocalNetwork,
    store_generation: &[u8; 32],
    source_events: u64,
    visible: Vec<radroots_storage::event::StoredVisibleEvent>,
    overlays: BTreeMap<String, LocalAuthorOverlay>,
) -> Result<TodayProjectionState, TodayError> {
    let mut cards = Vec::new();
    let mut profiles = BTreeMap::new();
    let mut thread = Vec::new();
    for stored in visible {
        let verified = verify_nip01_event(stored.event().envelope().clone())
            .map_err(|_| TodayError::CorruptProjection)?;
        let admitted = admit_verified_event(verified).map_err(|_| TodayError::CorruptProjection)?;
        match &admitted {
            RadrootsAdmittedEvent::Profile(profile) => {
                profiles.insert(
                    profile.event().author().to_hex(),
                    profile_summary(&admitted)?,
                );
            }
            RadrootsAdmittedEvent::Reply(_) => {
                thread.push(reply_entry(&admitted)?);
            }
            RadrootsAdmittedEvent::Comment(_) => {
                if let Some(entry) = comment_entry(&admitted) {
                    thread.push(entry);
                }
            }
            _ => {
                let locality = locality_tags(admitted.event().tags_as_vec());
                let evidence = locality_evidence(context.locality.as_deref(), &locality);
                if let ProductEventClassification::Card(card) =
                    classify_admitted_event(&admitted, context.admit(evidence))
                {
                    cards.push(ProjectedCard {
                        card: *card,
                        locality,
                    });
                }
            }
        }
    }
    cards.sort_by_key(|projected| projected.card.card_id);
    thread.sort_by(|left, right| left.event_id.cmp(&right.event_id));
    for entry in &mut thread {
        entry.author_profile = profiles.get(&entry.author_pubkey).cloned();
    }
    Ok(TodayProjectionState {
        schema_version: TODAY_PROJECTION_DOCUMENT_SCHEMA_VERSION,
        context_id: context.id.clone(),
        context_generation: context.generation,
        store_generation: *store_generation,
        source_events,
        content_generation: 0,
        cards,
        profiles,
        thread,
        overlays,
        media_cache: Phase1MediaCacheIndex::default(),
    })
}

fn profile_summary(admitted: &RadrootsAdmittedEvent) -> Result<ProfileSummary, TodayError> {
    let RadrootsAdmittedEvent::Profile(profile) = admitted else {
        return Err(TodayError::CorruptProjection);
    };
    let metadata = profile.metadata();
    Ok(ProfileSummary {
        author_pubkey: profile.event().author().to_hex(),
        name: metadata.name().map(str::to_owned),
        display_name: metadata.display_name().map(str::to_owned),
        about: metadata.about().map(str::to_owned),
        picture: metadata
            .picture()
            .map(|value| unverified_media(value.as_str()))
            .transpose()?,
        banner: metadata
            .banner()
            .map(|value| unverified_media(value.as_str()))
            .transpose()?,
        nip05: metadata.nip05().map(|value| value.as_str().to_owned()),
        website: typed_profile_extra(metadata.raw_fields(), "website"),
        lightning_address: typed_profile_extra(metadata.raw_fields(), "lud16"),
    })
}

fn typed_profile_extra(fields: &BTreeMap<String, serde_json::Value>, key: &str) -> Option<String> {
    fields
        .get(key)
        .and_then(serde_json::Value::as_str)
        .filter(|value| {
            !value.is_empty() && value.len() <= 2_048 && !value.chars().any(char::is_control)
        })
        .map(str::to_owned)
}

fn unverified_media(url: &str) -> Result<MediaReference, TodayError> {
    MediaReference::new(Phase1StructuralMediaReference::new(
        url,
        blossom_digest(url),
        None,
        None,
        None,
        None,
        None,
    )?)
    .map_err(TodayError::from)
}

fn reply_entry(admitted: &RadrootsAdmittedEvent) -> Result<ThreadEntry, TodayError> {
    let RadrootsAdmittedEvent::Reply(reply) = admitted else {
        return Err(TodayError::CorruptProjection);
    };
    Ok(ThreadEntry {
        event_id: reply.event().id_hex(),
        author_pubkey: reply.event().author().to_hex(),
        content: reply.event().content().to_owned(),
        authored_at: reply.event().created_at_u64(),
        reference: ThreadReference {
            profile: SupportingProfile::Reply,
            root: reply.projection().root().event_id().to_hex(),
            parent_event_id: reply.projection().parent().event_id().to_hex(),
        },
        author_profile: None,
    })
}

fn comment_entry(admitted: &RadrootsAdmittedEvent) -> Option<ThreadEntry> {
    let RadrootsAdmittedEvent::Comment(comment) = admitted else {
        return None;
    };
    let tags = comment.event().tags_as_vec();
    let root = tag_value(&tags, &["E", "A"])?;
    let parent = tag_value(&tags, &["e", "a"]).unwrap_or_else(|| root.clone());
    Some(ThreadEntry {
        event_id: comment.event().id_hex(),
        author_pubkey: comment.event().author().to_hex(),
        content: comment.event().content().to_owned(),
        authored_at: comment.event().created_at_u64(),
        reference: ThreadReference {
            profile: SupportingProfile::Comment,
            root,
            parent_event_id: parent,
        },
        author_profile: None,
    })
}

fn locality_tags(tags: Vec<Vec<String>>) -> Vec<LocalityTag> {
    let mut locality = tags
        .into_iter()
        .filter_map(|tag| {
            let kind = tag.first()?.as_str();
            if !matches!(kind, "g" | "location") {
                return None;
            }
            let value = tag.get(1)?.trim().to_lowercase();
            (!value.is_empty()).then(|| LocalityTag {
                kind: kind.to_owned(),
                value,
            })
        })
        .collect::<Vec<_>>();
    locality.sort();
    locality.dedup();
    locality
}

fn locality_evidence(selected: Option<&str>, locality: &[LocalityTag]) -> LocalityEvidence {
    let Some(selected) = selected else {
        return LocalityEvidence::Missing;
    };
    if locality.is_empty() {
        return LocalityEvidence::Missing;
    }
    let selected = selected.trim().to_lowercase();
    if locality.iter().any(|tag| {
        tag.value == selected
            || (tag.kind == "g"
                && (tag.value.starts_with(&selected) || selected.starts_with(&tag.value)))
    }) {
        LocalityEvidence::Match
    } else {
        LocalityEvidence::Nonmatch
    }
}

fn ranked_cards(
    state: &TodayProjectionState,
    context: &LocalNetwork,
    as_of: u64,
) -> Result<Vec<TodayCard>, TodayError> {
    let mut cards = Vec::new();
    for projected in &state.cards {
        let evidence = locality_evidence(context.locality.as_deref(), &projected.locality);
        let admission = match context.admit(evidence) {
            super::LocalNetworkAdmission::Included(admission) => admission,
            super::LocalNetworkAdmission::Excluded { .. } => continue,
        };
        let mut card = projected.card.clone();
        card.context_rank = admission.rank;
        card.inclusion_reason = admission.reason.to_owned();
        let time = match card.card_type {
            TodayCardType::Update | TodayCardType::PhotoUpdate | TodayCardType::Ask => {
                TimeRelevance::Published
            }
            TodayCardType::FoodAvailability => TimeRelevance::FoodAvailability {
                active: card.lifecycle == CardLifecycleState::Active,
            },
            TodayCardType::Event => TimeRelevance::Event {
                start: card.event_start.unwrap_or(card.effective_at),
                end: card.event_end,
            },
        };
        let rank = TodayRank::derive(TodayRankInput {
            card_type: card.card_type,
            context_rank: card.context_rank,
            as_of,
            effective_at: card.effective_at,
            time,
            card_id: card.card_id,
        })
        .map_err(|_| TodayError::CorruptProjection)?;
        if card.card_type == TodayCardType::Event && rank.time_relevance_rank == 0 {
            card.lifecycle = CardLifecycleState::Past;
        }
        card.rank = Some(rank);
        let roots = [
            card.source_event_id.as_str(),
            card.source_address.as_deref().unwrap_or(""),
        ];
        let card_thread = state
            .thread
            .iter()
            .filter(|entry| roots.contains(&entry.reference.root.as_str()))
            .cloned()
            .collect();
        cards.push(TodayCard {
            author_profile: state.profiles.get(&card.author_pubkey).cloned(),
            local_overlay: state.overlays.get(&card.card_id.to_hex()).cloned(),
            card,
            thread: card_thread,
        });
    }
    cards.sort_by_key(|card| card.card.rank.expect("assigned rank"));
    Ok(cards)
}

fn frozen_snapshot(
    state: &TodayProjectionState,
    context: &LocalNetwork,
    as_of: u64,
) -> Result<FrozenTodaySnapshot, TodayError> {
    Ok(FrozenTodaySnapshot {
        schema_version: TODAY_SNAPSHOT_SCHEMA_VERSION,
        context_id: context.id.clone(),
        context_generation: context.generation,
        as_of,
        store_generation: state.store_generation,
        projection_generation: state.content_generation,
        items: ranked_cards(state, context, as_of)?,
    })
}

fn page_from_snapshot(
    snapshot: FrozenTodaySnapshot,
    scope: CursorScope,
    after: Option<TodayRank>,
    limit: u16,
) -> Result<TodayPage, TodayError> {
    validate_snapshot(&snapshot, &scope)?;
    let start = if let Some(after) = after {
        snapshot
            .items
            .iter()
            .position(|card| card.card.rank == Some(after))
            .map(|index| index + 1)
            .ok_or(TodayError::CursorPositionMissing)?
    } else {
        0
    };
    let end = start
        .saturating_add(usize::from(limit))
        .min(snapshot.items.len());
    let items = snapshot.items[start..end].to_vec();
    let next_cursor = if end < snapshot.items.len() {
        items
            .last()
            .and_then(|card| card.card.rank)
            .map(|rank| TodayCursor::encode(&scope, TodayCursorPosition { rank }))
            .transpose()?
            .map(|cursor| cursor.as_str().to_owned())
    } else {
        None
    };
    Ok(TodayPage {
        as_of: snapshot.as_of,
        items,
        next_cursor,
    })
}

fn validate_snapshot(
    snapshot: &FrozenTodaySnapshot,
    scope: &CursorScope,
) -> Result<(), TodayError> {
    if snapshot.schema_version != TODAY_SNAPSHOT_SCHEMA_VERSION
        || snapshot.context_id != scope.context_id
        || snapshot.context_generation != scope.context_generation
        || snapshot.as_of != scope.as_of
        || snapshot.store_generation != scope.store_generation
        || snapshot.projection_generation != scope.projection_generation
    {
        return Err(TodayError::CorruptProjection);
    }
    Ok(())
}

async fn load_state(
    storage: &dyn radroots_storage::Storage,
    context: &LocalNetwork,
    generation: ProjectionGeneration,
) -> Result<Option<TodayProjectionState>, TodayError> {
    let document = ProjectionStore::projection_document(
        storage,
        projection_id()?,
        generation,
        projection_document_key(context),
    )
    .await?;
    let Some(document) = document else {
        return Ok(None);
    };
    let (state, migrated) = decode_state_document(document.value())?;
    if migrated {
        store_state(storage, context, generation, &state).await?;
    }
    Ok(Some(state))
}

async fn store_state(
    storage: &dyn radroots_storage::Storage,
    context: &LocalNetwork,
    generation: ProjectionGeneration,
    state: &TodayProjectionState,
) -> Result<(), TodayError> {
    ProjectionStore::put_projection_document(
        storage,
        projection_id()?,
        generation,
        ProjectionDocument::new(projection_document_key(context), encode(state)?)?,
    )
    .await?;
    Ok(())
}

async fn persist_snapshot(
    storage: &dyn radroots_storage::Storage,
    generation: ProjectionGeneration,
    scope: &CursorScope,
    snapshot: &FrozenTodaySnapshot,
) -> Result<(), TodayError> {
    ProjectionStore::put_projection_snapshot(
        storage,
        ProjectionSnapshot::new(
            projection_id()?,
            snapshot_id(scope),
            generation,
            scope
                .as_of
                .checked_mul(1_000)
                .ok_or(TodayError::InvalidRequest)?,
            encode(snapshot)?,
        )?,
    )
    .await?;
    Ok(())
}

async fn load_snapshot(
    storage: &dyn radroots_storage::Storage,
    projection_id: ProjectionId,
    generation: ProjectionGeneration,
    scope: &CursorScope,
) -> Result<Option<FrozenTodaySnapshot>, TodayError> {
    ProjectionStore::projection_snapshot(storage, projection_id, snapshot_id(scope))
        .await?
        .map(|snapshot| {
            if snapshot.generation() != generation {
                return Err(TodayError::CorruptProjection);
            }
            decode_snapshot(snapshot.value())
        })
        .transpose()
}

#[cfg(test)]
fn decode_state(value: &[u8]) -> Result<TodayProjectionState, TodayError> {
    decode_state_document(value).map(|(state, _)| state)
}

fn decode_state_document(value: &[u8]) -> Result<(TodayProjectionState, bool), TodayError> {
    if let Ok(state) = serde_json::from_slice::<TodayProjectionState>(value)
        && state.schema_version == TODAY_PROJECTION_DOCUMENT_SCHEMA_VERSION
        && state.content_generation != 0
        && content_generation(&state)? == state.content_generation
        && validate_media_state(&state).is_ok()
    {
        return Ok((state, false));
    }
    let state = migrate_legacy_state(value)?;
    if state.schema_version != TODAY_PROJECTION_DOCUMENT_SCHEMA_VERSION
        || state.content_generation == 0
        || content_generation(&state)? != state.content_generation
        || validate_media_state(&state).is_err()
    {
        return Err(TodayError::CorruptProjection);
    }
    Ok((state, true))
}

fn validate_media_state(state: &TodayProjectionState) -> Result<(), Phase1InboundMediaError> {
    state.media_cache.status()?;
    for projected in &state.cards {
        for media in &projected.card.media {
            media.validate()?;
            if let Phase1InboundMediaState::Verified(receipt) = media.retrieval()
                && !state.media_cache.contains(receipt)
            {
                return Err(Phase1InboundMediaError::CorruptState);
            }
        }
    }
    for profile in state.profiles.values() {
        for media in [&profile.picture, &profile.banner].into_iter().flatten() {
            media.validate()?;
            if let Phase1InboundMediaState::Verified(receipt) = media.retrieval()
                && !state.media_cache.contains(receipt)
            {
                return Err(Phase1InboundMediaError::CorruptState);
            }
        }
    }
    if state
        .thread
        .iter()
        .any(|entry| entry.author_profile.as_ref() != state.profiles.get(&entry.author_pubkey))
    {
        return Err(Phase1InboundMediaError::CorruptState);
    }
    Ok(())
}

fn sanitize_snapshot_media(snapshot: &mut FrozenTodaySnapshot, cache: &Phase1MediaCacheIndex) {
    for item in &mut snapshot.items {
        for media in &mut item.card.media {
            if let Phase1InboundMediaState::Verified(receipt) = media.retrieval()
                && !cache.contains(receipt)
            {
                media.invalidate();
            }
        }
        if let Some(profile) = &mut item.author_profile {
            for media in [&mut profile.picture, &mut profile.banner]
                .into_iter()
                .flatten()
            {
                if let Phase1InboundMediaState::Verified(receipt) = media.retrieval()
                    && !cache.contains(receipt)
                {
                    media.invalidate();
                }
            }
        }
        for entry in &mut item.thread {
            if let Some(profile) = &mut entry.author_profile {
                for media in [&mut profile.picture, &mut profile.banner]
                    .into_iter()
                    .flatten()
                {
                    if let Phase1InboundMediaState::Verified(receipt) = media.retrieval()
                        && !cache.contains(receipt)
                    {
                        media.invalidate();
                    }
                }
            }
        }
    }
}

fn decode_snapshot(value: &[u8]) -> Result<FrozenTodaySnapshot, TodayError> {
    let snapshot: FrozenTodaySnapshot = match serde_json::from_slice(value) {
        Ok(snapshot) => snapshot,
        Err(_) => {
            let mut legacy: serde_json::Value = decode(value)?;
            migrate_legacy_media_values(&mut legacy)?;
            serde_json::from_value(legacy).map_err(|_| TodayError::CorruptProjection)?
        }
    };
    if snapshot.schema_version != TODAY_SNAPSHOT_SCHEMA_VERSION {
        return Err(TodayError::CorruptProjection);
    }
    Ok(snapshot)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
struct LegacyMediaReference {
    url: String,
    sha256: Option<String>,
    media_type: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    byte_size: Option<u64>,
    alt: Option<String>,
    verification: LegacyMediaVerificationState,
}

#[derive(Deserialize)]
#[serde(rename_all = "PascalCase")]
enum LegacyMediaVerificationState {
    Pending,
    Verified,
    Failed,
    Unavailable,
}

fn migrate_legacy_state(value: &[u8]) -> Result<TodayProjectionState, TodayError> {
    verify_legacy_content_generation(value)?;
    let mut legacy: serde_json::Value = decode(value)?;
    migrate_legacy_media_values(&mut legacy)?;
    let object = legacy
        .as_object_mut()
        .ok_or(TodayError::CorruptProjection)?;
    if object.contains_key("mediaCache") {
        return Err(TodayError::CorruptProjection);
    }
    object.insert(
        "mediaCache".to_owned(),
        serde_json::to_value(Phase1MediaCacheIndex::default())
            .map_err(|_| TodayError::Serialization)?,
    );
    object.insert("contentGeneration".to_owned(), serde_json::json!(0));
    let mut state: TodayProjectionState =
        serde_json::from_value(legacy).map_err(|_| TodayError::CorruptProjection)?;
    state.content_generation = content_generation(&state)?;
    Ok(state)
}

fn verify_legacy_content_generation(value: &[u8]) -> Result<(), TodayError> {
    let parsed: serde_json::Value = decode(value)?;
    let expected = parsed
        .get("contentGeneration")
        .and_then(serde_json::Value::as_u64)
        .filter(|value| *value != 0)
        .ok_or(TodayError::CorruptProjection)?;
    if parsed
        .get("schemaVersion")
        .and_then(serde_json::Value::as_u64)
        != Some(u64::from(TODAY_PROJECTION_DOCUMENT_SCHEMA_VERSION))
    {
        return Err(TodayError::CorruptProjection);
    }
    let marker = b"\"contentGeneration\":";
    let starts = value
        .windows(marker.len())
        .enumerate()
        .filter_map(|(index, bytes)| (bytes == marker).then_some(index))
        .collect::<Vec<_>>();
    let [start] = starts.as_slice() else {
        return Err(TodayError::CorruptProjection);
    };
    let number_start = start + marker.len();
    let number_end = value[number_start..]
        .iter()
        .position(|byte| !byte.is_ascii_digit())
        .map(|offset| number_start + offset)
        .ok_or(TodayError::CorruptProjection)?;
    if number_end == number_start {
        return Err(TodayError::CorruptProjection);
    }
    let mut canonical = Vec::with_capacity(value.len());
    canonical.extend_from_slice(&value[..number_start]);
    canonical.push(b'0');
    canonical.extend_from_slice(&value[number_end..]);
    let digest = Sha256::digest([PROJECTION_CONTENT_DOMAIN, canonical.as_slice()].concat());
    let observed = u64::from_be_bytes(digest[..8].try_into().expect("digest prefix")).max(1);
    (observed == expected)
        .then_some(())
        .ok_or(TodayError::CorruptProjection)
}

fn migrate_legacy_media_values(value: &mut serde_json::Value) -> Result<(), TodayError> {
    match value {
        serde_json::Value::Array(values) => {
            for value in values {
                migrate_legacy_media_values(value)?;
            }
        }
        serde_json::Value::Object(object)
            if object.contains_key("verification") && object.contains_key("url") =>
        {
            let legacy: LegacyMediaReference =
                serde_json::from_value(value.clone()).map_err(|_| TodayError::CorruptProjection)?;
            let _legacy_verification = legacy.verification;
            let migrated = MediaReference::legacy_unavailable(
                legacy.url,
                legacy.sha256,
                legacy.media_type,
                legacy.width,
                legacy.height,
                legacy.byte_size,
                legacy.alt,
            )?;
            *value = serde_json::to_value(migrated).map_err(|_| TodayError::Serialization)?;
        }
        serde_json::Value::Object(object) => {
            for value in object.values_mut() {
                migrate_legacy_media_values(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}

fn content_generation(state: &TodayProjectionState) -> Result<u64, TodayError> {
    let mut canonical = state.clone();
    canonical.content_generation = 0;
    let digest =
        Sha256::digest([PROJECTION_CONTENT_DOMAIN, encode(&canonical)?.as_slice()].concat());
    let generation = u64::from_be_bytes(digest[..8].try_into().expect("digest prefix"));
    Ok(generation.max(1))
}

fn projection_generation() -> Result<ProjectionGeneration, TodayError> {
    ProjectionGeneration::new(Sha256::digest(PROJECTION_GENERATION_DOMAIN).into())
        .map_err(TodayError::from)
}

fn projection_id() -> Result<ProjectionId, TodayError> {
    ProjectionId::parse(TODAY_PROJECTION_ID).map_err(TodayError::from)
}

fn projection_document_key(context: &LocalNetwork) -> String {
    let mut digest = Sha256::new();
    digest.update(PROJECTION_DOCUMENT_KEY_DOMAIN);
    digest.update(context.id.as_bytes());
    digest.update(context.generation.to_be_bytes());
    format!("context.{}", hex::encode(digest.finalize()))
}

fn snapshot_id(scope: &CursorScope) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(SNAPSHOT_ID_DOMAIN);
    digest.update(scope.context_id.as_bytes());
    digest.update(scope.context_generation.to_be_bytes());
    digest.update(scope.as_of.to_be_bytes());
    digest.update(scope.store_generation);
    digest.update(scope.projection_generation.to_be_bytes());
    digest.finalize().into()
}

fn tag_value(tags: &[Vec<String>], names: &[&str]) -> Option<String> {
    tags.iter().find_map(|tag| {
        names
            .contains(&tag.first()?.as_str())
            .then(|| tag.get(1).cloned())
            .flatten()
    })
}

fn blossom_digest(url: &str) -> Option<String> {
    let path = url.split_once("://")?.1.split_once('/')?.1;
    let candidate = path.split(['.', '/', '?', '#']).next()?;
    (candidate.len() == 64
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then(|| candidate.to_owned())
}

fn valid_public_key(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

fn encode(value: &impl Serialize) -> Result<Vec<u8>, TodayError> {
    serde_json::to_vec(value).map_err(|_| TodayError::Serialization)
}

fn decode<T: for<'de> Deserialize<'de>>(value: &[u8]) -> Result<T, TodayError> {
    serde_json::from_slice(value).map_err(|_| TodayError::CorruptProjection)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(feature = "mobile-social")]
    use std::sync::{Arc, Mutex, RwLock, atomic::AtomicBool};
    #[cfg(feature = "mobile-social")]
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    #[cfg(feature = "mobile-social")]
    use tokio::net::TcpListener;

    use nostr::secp256k1::Message;
    use nostr::{Keys, SECP256K1};
    use radroots_blossom::{
        BlobUrl, MediaType, Sha256 as BlossomSha256, descriptor::ByteCommitment,
    };
    use radroots_event::{
        SignedEvent,
        admission::{AdmissionPolicy, RawEvent, VisibilityPolicy},
        wire::{Nip01EventWire, compute_canonical_nip01_event_id},
    };
    use radroots_event_codec::verify::Nip01SignatureVerifier;
    #[cfg(feature = "mobile-social")]
    use radroots_transport::{
        Error as TransportError, EventSource, FetchPage, FetchRequest, SourceStatus,
        outcome::{FetchTargetOutcome, FetchTargetState},
        source::NextPage,
    };
    use radroots_transport::{
        Target, TransportId,
        source::{EventProvenance, ObservedEvent},
    };

    const SECRET: &str = "10c5304d6c9ae3a1a16f7860f1cc8f5e3a76225a2663b3a989a0d775919b7df5";

    struct Allow;

    #[cfg(feature = "mobile-social")]
    struct TodaySource {
        event: SignedEvent,
        requested_kinds: Mutex<Vec<Vec<u32>>>,
    }

    #[cfg(feature = "mobile-social")]
    impl EventSource for TodaySource {
        fn status(
            &self,
        ) -> radroots_transport::BoxFuture<'_, Result<SourceStatus, TransportError>> {
            Box::pin(async { unreachable!("Today sync does not inspect source status") })
        }

        fn fetch(
            &self,
            request: FetchRequest,
        ) -> radroots_transport::BoxFuture<'_, Result<FetchPage, TransportError>> {
            Box::pin(async move {
                self.requested_kinds
                    .lock()
                    .expect("requested kinds")
                    .push(request.selector().kinds().to_vec());
                let target = request.target_set().targets()[0].clone();
                let provenance = EventProvenance::new(
                    TransportId::NOSTR,
                    target.fingerprint().clone(),
                    2_000_000_100_000,
                )?;
                FetchPage::for_request(
                    &request,
                    vec![ObservedEvent::new(self.event.clone(), provenance)],
                    vec![FetchTargetOutcome::new(
                        target.fingerprint().clone(),
                        FetchTargetState::Complete,
                    )],
                    NextPage::Complete,
                )
            })
        }
    }

    impl AdmissionPolicy for Allow {
        type Error = core::convert::Infallible;

        fn policy_id(&self) -> &'static str {
            "test.today.admission.v1"
        }

        fn admit(
            &self,
            _event: &radroots_event::admission::ContractValidatedEvent,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    impl VisibilityPolicy for Allow {
        type Error = core::convert::Infallible;

        fn policy_id(&self) -> &'static str {
            "test.today.visibility.v1"
        }

        fn make_visible(
            &self,
            _event: &radroots_event::admission::AdmittedEvent,
        ) -> Result<(), Self::Error> {
            Ok(())
        }
    }

    fn context(locality: Option<&str>, generation: u64) -> LocalNetwork {
        LocalNetwork::new(
            "victoria".into(),
            "Victoria".into(),
            vec!["wss://relay.example".into()],
            locality.map(str::to_owned),
            Vec::new(),
            generation,
        )
        .expect("context")
    }

    fn keys() -> Keys {
        Keys::parse(SECRET).expect("keys")
    }

    fn signed(kind: u32, tags: Vec<Vec<&str>>, content: &str, created_at: u64) -> SignedEvent {
        signed_owned(
            kind,
            tags.into_iter()
                .map(|tag| tag.into_iter().map(str::to_owned).collect())
                .collect(),
            content,
            created_at,
        )
    }

    fn signed_owned(
        kind: u32,
        tags: Vec<Vec<String>>,
        content: &str,
        created_at: u64,
    ) -> SignedEvent {
        let keys = keys();
        let author = keys.public_key().to_string();
        let id = compute_canonical_nip01_event_id(&author, created_at, kind, &tags, content)
            .expect("id");
        let message = Message::from_digest(*id.as_bytes());
        let signature = SECP256K1.sign_schnorr_no_aux_rand(
            &message,
            &nostr::secp256k1::Keypair::from_secret_key(SECP256K1, keys.secret_key()),
        );
        let wire = Nip01EventWire {
            id: id.to_hex(),
            pubkey: author,
            created_at,
            kind,
            tags,
            content: content.to_owned(),
            sig: signature.to_string(),
            extra: Default::default(),
        };
        let raw = serde_json::json!({
            "id": &wire.id,
            "pubkey": &wire.pubkey,
            "created_at": wire.created_at,
            "kind": wire.kind,
            "tags": &wire.tags,
            "content": &wire.content,
            "sig": &wire.sig,
        })
        .to_string();
        SignedEvent::from_wire_verified_id(wire, raw).expect("signed event")
    }

    fn visible_admission(event: SignedEvent, observed_at: u64) -> EventAdmission {
        let selected = admit_verified_event(
            verify_nip01_event(event.envelope().clone()).expect("codec verification"),
        )
        .expect("codec admission")
        .contract_id();
        let verified = RawEvent::new(event.envelope().clone())
            .verify_id()
            .expect("id")
            .verify_signature(&Nip01SignatureVerifier)
            .expect("signature");
        let visible = verified
            .validate_contract_for_admission(selected)
            .expect("selected contract")
            .admit_with(&Allow)
            .expect("admission")
            .make_visible_with(&Allow)
            .expect("visibility");
        let target = Target::new(TransportId::NOSTR, "wss://relay.example").expect("target");
        let provenance = EventProvenance::new(
            TransportId::NOSTR,
            target.fingerprint().clone(),
            observed_at,
        )
        .expect("provenance");
        EventAdmission::visible(ObservedEvent::new(event, provenance), visible)
            .expect("visible admission")
    }

    fn raw_admission(event: SignedEvent, observed_at: u64) -> EventAdmission {
        let target = Target::new(TransportId::NOSTR, "wss://relay.example").expect("target");
        let provenance = EventProvenance::new(
            TransportId::NOSTR,
            target.fingerprint().clone(),
            observed_at,
        )
        .expect("provenance");
        EventAdmission::raw(ObservedEvent::new(event, provenance))
    }

    fn admitted(event: &SignedEvent) -> RadrootsAdmittedEvent {
        admit_verified_event(
            verify_nip01_event(event.envelope().clone()).expect("codec verification"),
        )
        .expect("codec admission")
    }

    async fn ingest(
        runtime: &RadrootsRuntime,
        context: &LocalNetwork,
        event: SignedEvent,
        at: u64,
    ) -> TodayIngestReceipt {
        runtime
            .phase1_ingest_visible(visible_admission(event, at * 1_000), context, at)
            .await
            .expect("ingest")
    }

    #[allow(clippy::too_many_arguments)]
    async fn verify_inbound_media(
        runtime: &RadrootsRuntime,
        context: &LocalNetwork,
        source_url: &str,
        bytes: &[u8],
        media_type: &str,
        width: u32,
        height: u32,
        operation_id: [u8; 16],
    ) {
        let storage = runtime.client.storage().expect("storage");
        let state = load_state(storage, context, projection_generation().unwrap())
            .await
            .unwrap()
            .expect("projection");
        let reference = state
            .cards
            .iter()
            .flat_map(|value| value.card.media.iter())
            .chain(
                state
                    .profiles
                    .values()
                    .flat_map(|profile| [&profile.picture, &profile.banner].into_iter().flatten()),
            )
            .find(|media| media.structural().source_url() == source_url)
            .expect("structural media")
            .structural()
            .clone();
        let configuration = Phase1MediaConfigurationFingerprint::new([9; 32]).unwrap();
        runtime
            .phase1_begin_media_retrieval(
                context,
                *reference.fingerprint(),
                Phase1InboundMediaPending::new(operation_id, configuration, 10).unwrap(),
            )
            .await
            .expect("begin retrieval");
        let commitment = ByteCommitment::from_bytes(bytes, MediaType::parse(media_type).unwrap());
        let receipt = Phase1VerifiedMediaReceipt::from_commitment(
            &reference,
            BlobUrl::parse(source_url).unwrap(),
            &commitment,
            width,
            height,
            configuration,
            11,
        )
        .expect("byte receipt");
        runtime
            .phase1_commit_media_receipt(
                context,
                *reference.fingerprint(),
                operation_id,
                receipt,
                Phase1MediaCachePolicy::new(1_024, 8).unwrap(),
                12,
            )
            .await
            .expect("commit receipt");
    }

    fn downgrade_media_to_legacy(value: &mut serde_json::Value) {
        match value {
            serde_json::Value::Array(values) => {
                for value in values {
                    downgrade_media_to_legacy(value);
                }
            }
            serde_json::Value::Object(object)
                if object.contains_key("structural") && object.contains_key("retrieval") =>
            {
                let structural = object
                    .get("structural")
                    .and_then(serde_json::Value::as_object)
                    .expect("structural object");
                *value = serde_json::json!({
                    "url": structural.get("sourceUrl").cloned().unwrap(),
                    "sha256": structural.get("expectedSha256").cloned().unwrap(),
                    "mediaType": structural.get("expectedMediaType").cloned().unwrap(),
                    "width": structural.get("expectedWidth").cloned().unwrap(),
                    "height": structural.get("expectedHeight").cloned().unwrap(),
                    "byteSize": structural.get("expectedByteSize").cloned().unwrap(),
                    "alt": structural.get("alt").cloned().unwrap(),
                    "verification": "Verified",
                });
            }
            serde_json::Value::Object(object) => {
                for value in object.values_mut() {
                    downgrade_media_to_legacy(value);
                }
            }
            _ => {}
        }
    }

    fn legacy_state_bytes(state: &TodayProjectionState) -> Vec<u8> {
        let mut value = serde_json::to_value(state).expect("state value");
        let object = value.as_object_mut().expect("state object");
        object.remove("mediaCache").expect("new cache field");
        object.insert("contentGeneration".to_owned(), serde_json::json!(0));
        downgrade_media_to_legacy(&mut value);
        let canonical = serde_json::to_vec(&value).expect("legacy canonical");
        let digest =
            sha2::Sha256::digest([PROJECTION_CONTENT_DOMAIN, canonical.as_slice()].concat());
        let generation = u64::from_be_bytes(digest[..8].try_into().expect("digest prefix")).max(1);
        value["contentGeneration"] = serde_json::json!(generation);
        serde_json::to_vec(&value).expect("legacy state")
    }

    #[cfg(feature = "mobile-social")]
    fn png(width: u32, height: u32) -> Vec<u8> {
        let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        bytes.extend_from_slice(&width.to_be_bytes());
        bytes.extend_from_slice(&height.to_be_bytes());
        bytes
    }

    #[cfg(feature = "mobile-social")]
    async fn serve_one_blob(listener: TcpListener, bytes: Vec<u8>) {
        let (mut stream, _) = listener.accept().await.expect("accept");
        let mut request = Vec::new();
        while !request.windows(4).any(|value| value == b"\r\n\r\n") {
            let mut chunk = [0_u8; 1024];
            let read = stream.read(&mut chunk).await.expect("request read");
            assert_ne!(read, 0);
            request.extend_from_slice(&chunk[..read]);
            assert!(request.len() <= 64 * 1024);
        }
        let headers = String::from_utf8_lossy(&request);
        assert!(headers.starts_with("GET /"));
        assert!(
            headers
                .to_ascii_lowercase()
                .contains("accept-encoding: identity")
        );
        assert!(!headers.to_ascii_lowercase().contains("authorization:"));
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            bytes.len()
        );
        stream.write_all(response.as_bytes()).await.expect("head");
        stream.write_all(&bytes).await.expect("body");
        stream.shutdown().await.expect("close");
    }

    #[cfg(feature = "mobile-social")]
    #[tokio::test]
    async fn relay_sync_fetches_the_exact_today_selector_and_projects_real_events() {
        let source = Arc::new(TodaySource {
            event: signed(1, Vec::new(), "Fresh from the field", 2_000_000_000),
            requested_kinds: Mutex::new(Vec::new()),
        });
        let client = radroots_sdk::ClientBuilder::memory_default()
            .source(source.clone())
            .host_sync(radroots_sdk::sync::HostPolicy::standard())
            .build()
            .expect("client");
        let runtime = RadrootsRuntime {
            client,
            started_unix_ms: 1,
            shutting_down: AtomicBool::new(false),
            platform_app: RwLock::new(None),
            store_public_key: None,
            settings_lock: tokio::sync::Mutex::new(()),
            identity_session: tokio::sync::RwLock::new(None),
            inbound_media_directory: None,
            inbound_media_lock: tokio::sync::Mutex::new(()),
        };
        let context = context(None, 1);

        let receipt = runtime
            .phase1_sync_today(&context, 2_000_000_200, TodayProjectionUpdate::Incremental)
            .await
            .expect("Today sync");
        assert_eq!(receipt.relay_state, TodayRelaySyncState::Complete);
        assert_eq!(receipt.pages_fetched, 1);
        assert_eq!(receipt.events_observed, 1);
        assert_eq!(receipt.events_admitted, 1);
        assert_eq!(receipt.events_rejected, 0);
        assert_eq!(receipt.projection.visible_cards, 1);
        assert_eq!(
            source
                .requested_kinds
                .lock()
                .expect("requested kinds")
                .as_slice(),
            &[TODAY_SYNC_KINDS.to_vec()]
        );
        let page = runtime
            .phase1_today_page(&context, TodayPageRequest::first(20, 2_000_000_200))
            .await
            .expect("Today page");
        assert_eq!(page.items.len(), 1);
        assert_eq!(page.items[0].card.card_type, TodayCardType::Update);
        assert_eq!(page.items[0].card.content, "Fresh from the field");
    }

    #[tokio::test]
    async fn equal_timestamp_pages_are_complete_and_remain_frozen_across_ingest() {
        let runtime = RadrootsRuntime::test_memory().expect("runtime");
        let context = context(None, 1);
        for content in ["alpha", "bravo", "charlie"] {
            ingest(
                &runtime,
                &context,
                signed(1, Vec::new(), content, 2_000_000_000),
                2_000_000_100,
            )
            .await;
        }
        let first = runtime
            .phase1_today_page(&context, TodayPageRequest::first(1, 2_000_000_200))
            .await
            .expect("first page");
        assert_eq!(first.items.len(), 1);
        let frozen_cursor = first.next_cursor.clone().expect("cursor");

        ingest(
            &runtime,
            &context,
            signed(1, Vec::new(), "delta", 2_000_000_001),
            2_000_000_101,
        )
        .await;

        let mut ids = first
            .items
            .iter()
            .map(|card| card.card.card_id.to_hex())
            .collect::<Vec<_>>();
        let mut cursor = Some(frozen_cursor);
        while let Some(value) = cursor {
            let page = runtime
                .phase1_today_page(&context, TodayPageRequest::after(1, value))
                .await
                .expect("continued frozen page");
            ids.extend(page.items.iter().map(|card| card.card.card_id.to_hex()));
            cursor = page.next_cursor;
        }
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), 3, "frozen snapshot has no loss or duplicates");

        let current = runtime
            .phase1_today_page(&context, TodayPageRequest::first(100, 2_000_000_201))
            .await
            .expect("current page");
        assert_eq!(current.items.len(), 4);
    }

    #[tokio::test]
    async fn profile_thread_media_search_me_context_and_rebuild_share_one_projection() {
        let runtime = RadrootsRuntime::test_memory().expect("runtime");
        let context = context(Some("victoria"), 7);
        let author = keys().public_key().to_string();
        let profile_bytes = b"profile picture";
        let profile_digest = BlossomSha256::digest(profile_bytes).to_hex();
        let profile_url = format!("https://blob.example/{profile_digest}.jpg");
        let profile = signed(
            0,
            Vec::new(),
            &format!(
                r#"{{"name":"moss","display_name":"Moss Farm","about":"Local roots","picture":"{profile_url}","website":"https://moss.example","lud16":"moss@example.com"}}"#
            ),
            2_000_000_000,
        );
        ingest(&runtime, &context, profile, 2_000_000_100).await;

        let photo_bytes = b"photo";
        let digest = BlossomSha256::digest(photo_bytes).to_hex();
        let photo_url = format!("https://blob.example/{digest}.jpg");
        let photo_content = format!("Fresh field photo {photo_url}");
        let photo = signed_owned(
            1,
            vec![
                vec!["location".into(), "Victoria".into()],
                vec![
                    "imeta".into(),
                    format!("url {photo_url}"),
                    format!("x {digest}"),
                    "m image/jpeg".into(),
                    "dim 120x80".into(),
                    format!("size {}", photo_bytes.len()),
                    "alt Fresh field photo".into(),
                ],
            ],
            &photo_content,
            2_000_000_001,
        );
        let root_id = photo.id().to_hex();
        ingest(&runtime, &context, photo, 2_000_000_101).await;

        let nonmatch = signed(
            1,
            vec![vec!["location", "Elsewhere"]],
            "far away",
            2_000_000_002,
        );
        ingest(&runtime, &context, nonmatch, 2_000_000_102).await;

        let reply = signed_owned(
            1,
            vec![
                vec![
                    "e".into(),
                    root_id.clone(),
                    "wss://relay.example".into(),
                    "root".into(),
                ],
                vec!["p".into(), author.clone()],
            ],
            "Looks good",
            2_000_000_003,
        );
        ingest(&runtime, &context, reply, 2_000_000_103).await;

        let food = signed(
            30_402,
            vec![
                vec!["d", "today-carrots"],
                vec!["title", "Today carrots"],
                vec!["summary", "Fresh"],
                vec!["published_at", "2000000004"],
                vec!["location", "Victoria"],
                vec!["price", "3", "CAD"],
                vec!["radroots:price_unit", "lb"],
                vec!["status", "active"],
            ],
            "Fresh carrots",
            2_000_000_004,
        );
        let food_id = food.id().to_hex();
        ingest(&runtime, &context, food, 2_000_000_104).await;

        let comment = signed_owned(
            1_111,
            vec![
                vec![
                    "E".into(),
                    food_id.clone(),
                    "wss://relay.example".into(),
                    author.clone(),
                ],
                vec!["K".into(), "30402".into()],
                vec!["P".into(), author.clone(), "wss://relay.example".into()],
                vec![
                    "e".into(),
                    food_id,
                    "wss://relay.example".into(),
                    author.clone(),
                ],
                vec!["k".into(), "30402".into()],
                vec!["p".into(), author.clone(), "wss://relay.example".into()],
            ],
            "A NIP-22 comment",
            2_000_000_005,
        );
        ingest(&runtime, &context, comment, 2_000_000_105).await;

        verify_inbound_media(
            &runtime,
            &context,
            &photo_url,
            photo_bytes,
            "image/jpeg",
            120,
            80,
            [1; 16],
        )
        .await;
        verify_inbound_media(
            &runtime,
            &context,
            &profile_url,
            profile_bytes,
            "image/jpeg",
            24,
            24,
            [2; 16],
        )
        .await;
        let live = runtime
            .phase1_today_page(&context, TodayPageRequest::first(100, 2_000_000_200))
            .await
            .expect("page");
        assert_eq!(live.items.len(), 2, "known locality nonmatch is excluded");
        let card = live
            .items
            .iter()
            .find(|card| card.card.card_type == TodayCardType::PhotoUpdate)
            .unwrap_or_else(|| panic!("photo card missing from {:#?}", live.items));
        assert_eq!(card.card.card_type, TodayCardType::PhotoUpdate);
        assert!(matches!(
            card.card.media[0].retrieval(),
            Phase1InboundMediaState::Verified(_)
        ));
        assert_eq!(card.thread.len(), 1);
        assert_eq!(
            live.items
                .iter()
                .find(|card| card.card.card_type == TodayCardType::FoodAvailability)
                .expect("food card")
                .thread
                .len(),
            1
        );
        let profile = card.author_profile.as_ref().expect("profile enrichment");
        assert_eq!(profile.website.as_deref(), Some("https://moss.example"));
        assert_eq!(
            profile.lightning_address.as_deref(),
            Some("moss@example.com")
        );
        assert!(matches!(
            profile
                .picture
                .as_ref()
                .expect("profile picture")
                .retrieval(),
            Phase1InboundMediaState::Verified(_)
        ));

        runtime
            .phase1_set_local_author_overlay(
                &context,
                card.card.card_id,
                Some(LocalAuthorOverlay {
                    operation_id: "publish-photo-1".into(),
                    state: "delivered".into(),
                }),
            )
            .await
            .expect("active author overlay");
        assert!(matches!(
            runtime
                .phase1_set_local_author_overlay(
                    &context,
                    CardId::parse(&"f".repeat(64)).expect("unknown card id"),
                    None,
                )
                .await,
            Err(TodayError::InvalidRequest)
        ));

        let search = runtime
            .phase1_search(&context, "moss farm", 10, 2_000_000_200)
            .await
            .expect("search");
        assert!(search.iter().any(|result| result.profile.is_some()));
        let me = runtime
            .phase1_me(&context, &author, 2_000_000_200)
            .await
            .expect("me");
        assert_eq!(me.cards.len(), 2);
        assert_eq!(
            me.profile.expect("me profile").name.as_deref(),
            Some("moss")
        );

        let rebuilt = runtime
            .phase1_refresh_today(&context, 2_000_000_201, TodayProjectionUpdate::Rebuild)
            .await
            .expect("rebuild");
        assert!(!rebuilt.changed, "rebuild is byte-equivalent to live state");
        let after = runtime
            .phase1_today_page(&context, TodayPageRequest::first(100, 2_000_000_202))
            .await
            .expect("rebuilt page");
        let rebuilt_photo = after
            .items
            .iter()
            .find(|card| card.card.card_type == TodayCardType::PhotoUpdate)
            .expect("rebuilt photo");
        assert!(matches!(
            rebuilt_photo.card.media[0].retrieval(),
            Phase1InboundMediaState::Verified(_)
        ));
        assert!(matches!(
            rebuilt_photo
                .author_profile
                .as_ref()
                .and_then(|profile| profile.picture.as_ref())
                .expect("rebuilt profile picture")
                .retrieval(),
            Phase1InboundMediaState::Verified(_)
        ));
        assert_eq!(
            rebuilt_photo
                .local_overlay
                .as_ref()
                .map(|overlay| overlay.state.as_str()),
            Some("delivered")
        );
        assert_eq!(rebuilt_photo.thread.len(), 1);

        let photo_reference = rebuilt_photo.card.media[0].structural().clone();
        let photo_receipt = match rebuilt_photo.card.media[0].retrieval() {
            Phase1InboundMediaState::Verified(receipt) => (**receipt).clone(),
            state => panic!("unexpected photo state: {state:?}"),
        };
        let profile_artifact = match rebuilt_photo
            .author_profile
            .as_ref()
            .and_then(|profile| profile.picture.as_ref())
            .expect("profile picture before eviction")
            .retrieval()
        {
            Phase1InboundMediaState::Verified(receipt) => receipt.artifact_id(),
            state => panic!("unexpected profile state: {state:?}"),
        };
        runtime
            .phase1_begin_media_retrieval(
                &context,
                *photo_reference.fingerprint(),
                Phase1InboundMediaPending::new(
                    [3; 16],
                    Phase1MediaConfigurationFingerprint::new([9; 32]).unwrap(),
                    30,
                )
                .unwrap(),
            )
            .await
            .expect("repeat retrieval");
        let evicted = runtime
            .phase1_commit_media_receipt(
                &context,
                *photo_reference.fingerprint(),
                [3; 16],
                photo_receipt,
                Phase1MediaCachePolicy::new(1_024, 1).unwrap(),
                31,
            )
            .await
            .expect("quota commit");
        assert_eq!(evicted, vec![profile_artifact]);
        let quota_page = runtime
            .phase1_today_page(&context, TodayPageRequest::first(100, 2_000_000_203))
            .await
            .expect("quota page");
        let quota_photo = quota_page
            .items
            .iter()
            .find(|card| card.card.card_type == TodayCardType::PhotoUpdate)
            .expect("quota photo");
        assert!(matches!(
            quota_photo.card.media[0].retrieval(),
            Phase1InboundMediaState::Verified(_)
        ));
        assert!(matches!(
            quota_photo
                .author_profile
                .as_ref()
                .and_then(|profile| profile.picture.as_ref())
                .expect("evicted profile picture")
                .retrieval(),
            Phase1InboundMediaState::Unavailable
        ));

        let removed = runtime
            .phase1_invalidate_media_configuration(
                &context,
                Phase1MediaConfigurationFingerprint::new([8; 32]).unwrap(),
            )
            .await
            .expect("configuration invalidation");
        assert_eq!(removed.len(), 1);
        let invalidated = runtime
            .phase1_today_page(&context, TodayPageRequest::first(100, 2_000_000_203))
            .await
            .expect("page after configuration change");
        let invalidated_photo = invalidated
            .items
            .iter()
            .find(|card| card.card.card_type == TodayCardType::PhotoUpdate)
            .expect("invalidated photo");
        assert!(matches!(
            invalidated_photo.card.media[0].retrieval(),
            Phase1InboundMediaState::Unavailable
        ));
        assert!(matches!(
            invalidated_photo
                .author_profile
                .as_ref()
                .and_then(|profile| profile.picture.as_ref())
                .expect("invalidated profile picture")
                .retrieval(),
            Phase1InboundMediaState::Unavailable
        ));
    }

    #[tokio::test]
    async fn legacy_enum_only_media_migrates_to_unavailable_and_repersists() {
        let runtime = RadrootsRuntime::test_memory().expect("runtime");
        let context = context(None, 1);
        let bytes = b"legacy profile";
        let hash = BlossomSha256::digest(bytes).to_hex();
        let url = format!("https://blob.example/{hash}.jpg");
        ingest(
            &runtime,
            &context,
            signed(
                0,
                Vec::new(),
                &format!(r#"{{"name":"legacy","picture":"{url}"}}"#),
                2_000_000_000,
            ),
            2_000_000_100,
        )
        .await;
        let storage = runtime.client.storage().expect("storage");
        let generation = projection_generation().unwrap();
        let state = load_state(storage, &context, generation)
            .await
            .unwrap()
            .expect("state");
        let legacy = legacy_state_bytes(&state);
        ProjectionStore::put_projection_document(
            storage,
            projection_id().unwrap(),
            generation,
            ProjectionDocument::new(projection_document_key(&context), legacy.clone()).unwrap(),
        )
        .await
        .unwrap();

        let migrated = load_state(storage, &context, generation)
            .await
            .unwrap()
            .expect("migrated state");
        let picture = migrated
            .profiles
            .values()
            .next()
            .and_then(|profile| profile.picture.as_ref())
            .expect("picture");
        assert!(matches!(
            picture.retrieval(),
            Phase1InboundMediaState::Unavailable
        ));
        assert_eq!(migrated.media_cache.status().unwrap().artifacts, 0);
        let stored = ProjectionStore::projection_document(
            storage,
            projection_id().unwrap(),
            generation,
            projection_document_key(&context),
        )
        .await
        .unwrap()
        .expect("repersisted state");
        let text = std::str::from_utf8(stored.value()).unwrap();
        assert!(!text.contains("\"verification\""));
        assert!(text.contains("\"mediaCache\""));

        let mut tampered: serde_json::Value = serde_json::from_slice(&legacy).unwrap();
        tampered["profiles"]
            .as_object_mut()
            .unwrap()
            .values_mut()
            .next()
            .unwrap()["name"] = serde_json::json!("tampered");
        assert!(matches!(
            decode_state(&serde_json::to_vec(&tampered).unwrap()),
            Err(TodayError::CorruptProjection)
        ));
    }

    #[tokio::test]
    async fn replacement_and_deletion_change_only_current_today_truth() {
        let runtime = RadrootsRuntime::test_memory().expect("runtime");
        let context = context(None, 1);
        let active = signed(
            30_402,
            vec![
                vec!["d", "carrots"],
                vec!["title", "Carrots"],
                vec!["summary", "Fresh"],
                vec!["published_at", "2000000000"],
                vec!["location", "Victoria"],
                vec!["price", "3", "CAD"],
                vec!["radroots:price_unit", "lb"],
                vec!["status", "active"],
            ],
            "available",
            2_000_000_000,
        );
        ingest(&runtime, &context, active, 2_000_000_100).await;
        let sold = signed(
            30_402,
            vec![
                vec!["d", "carrots"],
                vec!["title", "Carrots"],
                vec!["summary", "Gone"],
                vec!["published_at", "2000000001"],
                vec!["location", "Victoria"],
                vec!["price", "3", "CAD"],
                vec!["radroots:price_unit", "lb"],
                vec!["status", "sold"],
            ],
            "sold out",
            2_000_000_001,
        );
        let sold_id = sold.id().to_hex();
        ingest(&runtime, &context, sold, 2_000_000_101).await;
        let current = runtime
            .phase1_today_page(&context, TodayPageRequest::first(10, 2_000_000_200))
            .await
            .expect("current");
        assert_eq!(current.items.len(), 1);
        assert_eq!(current.items[0].card.source_event_id, sold_id);
        assert_eq!(current.items[0].card.lifecycle, CardLifecycleState::Sold);

        let deletion = signed_owned(5, vec![vec!["e".into(), sold_id]], "", 2_000_000_002);
        ingest(&runtime, &context, deletion, 2_000_000_102).await;
        let deleted = runtime
            .phase1_today_page(&context, TodayPageRequest::first(10, 2_000_000_201))
            .await
            .expect("deleted");
        assert!(deleted.items.is_empty());
    }

    #[tokio::test]
    async fn sqlite_reopen_preserves_materialized_state_and_frozen_cursor_pages() {
        use crate::runtime::{
            builder::RuntimeBuilder,
            store::{MobileUserStoreConfig, ProtectedDataAvailability},
        };

        let root = tempfile::tempdir().expect("root");
        let store = MobileUserStoreConfig::from_encoded(
            root.path(),
            &keys().public_key().to_string(),
            "3131313131313131313131313131313131313131313131313131313131313131",
            2_000_000_000_000,
            ProtectedDataAvailability::Available,
        )
        .expect("store");
        std::fs::create_dir_all(store.owner_directory()).expect("owner directory");
        let context = context(None, 1);
        let runtime = RuntimeBuilder::new(store.clone())
            .build()
            .await
            .expect("runtime");
        ingest(
            &runtime,
            &context,
            signed(1, Vec::new(), "persisted alpha", 2_000_000_000),
            2_000_000_100,
        )
        .await;
        ingest(
            &runtime,
            &context,
            signed(1, Vec::new(), "persisted bravo", 2_000_000_000),
            2_000_000_101,
        )
        .await;
        let first = runtime
            .phase1_today_page(&context, TodayPageRequest::first(1, 2_000_000_200))
            .await
            .expect("first page");
        let cursor = first.next_cursor.expect("frozen cursor");
        runtime.shutdown().await.expect("shutdown");

        let reopened = RuntimeBuilder::new(store).build().await.expect("reopen");
        let second = reopened
            .phase1_today_page(&context, TodayPageRequest::after(1, cursor))
            .await
            .expect("continued page after reopen");
        assert_eq!(second.items.len(), 1);
        assert_ne!(first.items[0].card.card_id, second.items[0].card.card_id);
        assert!(second.next_cursor.is_none());
        let search = reopened
            .phase1_search(&context, "persisted", 10, 2_000_000_200)
            .await
            .expect("search after reopen");
        assert_eq!(search.len(), 2);
        reopened.shutdown().await.expect("shutdown reopened");
    }

    #[tokio::test]
    async fn sqlite_reopen_preserves_receipts_and_missing_artifacts_fail_closed() {
        use crate::runtime::{
            builder::RuntimeBuilder,
            store::{MobileUserStoreConfig, ProtectedDataAvailability},
        };

        let root = tempfile::tempdir().expect("root");
        let public_key = keys().public_key().to_string();
        let store = MobileUserStoreConfig::from_encoded(
            root.path(),
            &public_key,
            "4141414141414141414141414141414141414141414141414141414141414141",
            2_000_000_000_000,
            ProtectedDataAvailability::Available,
        )
        .expect("store");
        std::fs::create_dir_all(store.owner_directory()).expect("owner directory");
        let context = context(None, 1);
        let runtime = RuntimeBuilder::new(store.clone())
            .build()
            .await
            .expect("runtime");
        let bytes = b"sqlite profile";
        let hash = BlossomSha256::digest(bytes).to_hex();
        let url = format!("https://blob.example/{hash}.jpg");
        ingest(
            &runtime,
            &context,
            signed(
                0,
                Vec::new(),
                &format!(r#"{{"name":"sqlite","picture":"{url}"}}"#),
                2_000_000_000,
            ),
            2_000_000_100,
        )
        .await;
        verify_inbound_media(
            &runtime,
            &context,
            &url,
            bytes,
            "image/jpeg",
            20,
            20,
            [6; 16],
        )
        .await;
        assert_eq!(
            runtime
                .phase1_media_cache_status(&context)
                .await
                .unwrap()
                .artifacts,
            1
        );
        runtime.shutdown().await.expect("shutdown");

        let reopened = RuntimeBuilder::new(store).build().await.expect("reopen");
        let profile = reopened
            .phase1_me(&context, &public_key, 2_000_000_200)
            .await
            .unwrap()
            .profile
            .expect("profile");
        let picture = profile.picture.expect("picture");
        let artifact_id = match picture.retrieval() {
            Phase1InboundMediaState::Verified(receipt) => receipt.artifact_id(),
            state => panic!("unexpected state: {state:?}"),
        };
        assert!(
            reopened
                .phase1_invalidate_media_artifact(&context, artifact_id)
                .await
                .unwrap()
        );
        assert_eq!(
            reopened
                .phase1_media_cache_status(&context)
                .await
                .unwrap()
                .artifacts,
            0
        );
        let profile = reopened
            .phase1_me(&context, &public_key, 2_000_000_201)
            .await
            .unwrap()
            .profile
            .expect("profile after invalidation");
        assert!(matches!(
            profile.picture.unwrap().retrieval(),
            Phase1InboundMediaState::Unavailable
        ));
        reopened.shutdown().await.expect("shutdown reopened");
    }

    #[cfg(feature = "mobile-social")]
    #[tokio::test]
    async fn hardened_retrieval_atomically_writes_and_invalidates_the_local_artifact() {
        use crate::runtime::{
            builder::RuntimeBuilder,
            store::{MobileUserStoreConfig, ProtectedDataAvailability},
        };
        use radroots_sdk::transport::{
            BlossomCancellation, BlossomConfig, BlossomEndpointAuthority, BlossomHostKind,
            BlossomProfile,
        };

        let bytes = png(2, 3);
        let hash = BlossomSha256::digest(bytes.as_slice()).to_hex();
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let origin = format!("http://{}", listener.local_addr().expect("address"));
        let url = format!("{origin}/{hash}.png");
        let server = tokio::spawn(serve_one_blob(listener, bytes.clone()));
        let root = tempfile::tempdir().expect("root");
        let public_key = keys().public_key().to_string();
        let store = MobileUserStoreConfig::from_encoded(
            root.path(),
            &public_key,
            "6161616161616161616161616161616161616161616161616161616161616161",
            2_000_000_000_000,
            ProtectedDataAvailability::Available,
        )
        .expect("store");
        std::fs::create_dir_all(store.owner_directory()).expect("owner directory");
        let owner_directory = store.owner_directory().to_path_buf();
        let blossom = BlossomConfig::from_profile(
            BlossomProfile::new(
                BlossomHostKind::Simulator,
                BlossomEndpointAuthority::LoopbackDevelopment,
                origin,
                std::iter::empty::<String>(),
            )
            .expect("profile"),
        )
        .with_network_policy(
            std::time::Duration::from_millis(100),
            std::time::Duration::from_millis(100),
            1,
            std::time::Duration::from_millis(1),
        )
        .expect("network policy");
        let runtime = RuntimeBuilder::new(store)
            .blossom_config(blossom)
            .build()
            .await
            .expect("runtime");
        let context = context(None, 1);
        ingest(
            &runtime,
            &context,
            signed(
                0,
                Vec::new(),
                &format!(r#"{{"name":"retrieved","picture":"{url}"}}"#),
                2_000_000_000,
            ),
            2_000_000_100,
        )
        .await;
        let picture = runtime
            .phase1_me(&context, &public_key, 2_000_000_200)
            .await
            .expect("me")
            .profile
            .expect("profile")
            .picture
            .expect("picture");
        let artifact = runtime
            .phase1_retrieve_media(
                &context,
                *picture.structural().fingerprint(),
                [9; 16],
                Phase1MediaCachePolicy::new(1_024, 8).unwrap(),
                BlossomCancellation::default(),
            )
            .await
            .expect("verified retrieval");
        server.await.expect("server");
        assert!(
            artifact
                .local_path()
                .starts_with(owner_directory.join("inbound_media.v1"))
        );
        assert_eq!(tokio::fs::read(artifact.local_path()).await.unwrap(), bytes);
        assert_eq!(artifact.bytes(), bytes);
        assert!(
            !tokio::fs::symlink_metadata(artifact.local_path())
                .await
                .unwrap()
                .file_type()
                .is_symlink()
        );
        let verified = runtime
            .phase1_me(&context, &public_key, 2_000_000_201)
            .await
            .expect("verified me")
            .profile
            .expect("verified profile")
            .picture
            .expect("verified picture");
        assert!(matches!(
            verified.retrieval(),
            Phase1InboundMediaState::Verified(_)
        ));
        let mut corrupt = bytes.clone();
        let last = corrupt.len() - 1;
        corrupt[last] ^= 1;
        tokio::fs::write(artifact.local_path(), corrupt)
            .await
            .expect("corrupt cached bytes");
        assert!(matches!(
            runtime
                .phase1_verified_media_artifact(
                    &context,
                    artifact.artifact_id(),
                    2_000_000_203_000,
                )
                .await,
            Err(TodayError::InboundMedia(
                Phase1InboundMediaError::CorruptArtifact
            ))
        ));
        assert!(!artifact.local_path().exists());
        let current_configuration = runtime
            .phase1_media_cache_status(&context)
            .await
            .unwrap()
            .configuration
            .expect("configuration");
        let replacement_bytes = if current_configuration.as_bytes() == &[1; 32] {
            [2; 32]
        } else {
            [1; 32]
        };
        let replacement_configuration =
            Phase1MediaConfigurationFingerprint::new(replacement_bytes).unwrap();
        runtime
            .phase1_invalidate_media_configuration(&context, replacement_configuration)
            .await
            .expect("invalidate configuration");
        let unavailable = runtime
            .phase1_me(&context, &public_key, 2_000_000_202)
            .await
            .expect("unavailable me")
            .profile
            .expect("unavailable profile")
            .picture
            .expect("unavailable picture");
        assert!(matches!(
            unavailable.retrieval(),
            Phase1InboundMediaState::Unavailable
        ));
        runtime.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn fail_closed_requests_cursors_overlays_and_projection_guards_are_executable() {
        let mut runtime = RadrootsRuntime::test_memory().expect("runtime");
        let context = context(None, 1);
        let note = signed(1, Vec::new(), "guarded alpha", 2_000_000_000);
        assert!(matches!(
            runtime
                .phase1_ingest_visible(raw_admission(note.clone(), 2_000_000_000_000), &context, 1)
                .await,
            Err(TodayError::EventNotVisible)
        ));
        assert!(matches!(
            runtime
                .phase1_refresh_today(&context, 0, TodayProjectionUpdate::Incremental)
                .await,
            Err(TodayError::InvalidRequest)
        ));
        assert!(matches!(
            runtime
                .phase1_refresh_today(&context, u64::MAX, TodayProjectionUpdate::Incremental)
                .await,
            Err(TodayError::InvalidRequest)
        ));
        let empty = runtime
            .phase1_refresh_today(&context, 1, TodayProjectionUpdate::Incremental)
            .await
            .expect("empty projection");
        assert_eq!(empty.source_events, 0);
        assert!(empty.changed);
        assert!(
            !runtime
                .phase1_refresh_today(&context, 2, TodayProjectionUpdate::Incremental)
                .await
                .expect("unchanged projection")
                .changed
        );

        for request in [
            TodayPageRequest::first(0, 1),
            TodayPageRequest::first(TODAY_PAGE_LIMIT_MAX + 1, 1),
            TodayPageRequest {
                limit: 1,
                as_of: None,
                cursor: None,
            },
            TodayPageRequest::first(1, 0),
        ] {
            assert!(matches!(
                runtime.phase1_today_page(&context, request).await,
                Err(TodayError::InvalidRequest)
            ));
        }
        for (query, limit, as_of) in [
            ("valid", 0, 1),
            ("valid", TODAY_SEARCH_LIMIT_MAX + 1, 1),
            ("valid", 1, 0),
            (" ", 1, 1),
            (&"x".repeat(257), 1, 1),
            ("bad\nquery", 1, 1),
        ] {
            assert!(matches!(
                runtime.phase1_search(&context, query, limit, as_of).await,
                Err(TodayError::InvalidRequest)
            ));
        }
        assert!(matches!(
            runtime.phase1_me(&context, "bad", 1).await,
            Err(TodayError::InvalidRequest)
        ));
        assert!(matches!(
            runtime
                .phase1_me(&context, &keys().public_key().to_string(), 0)
                .await,
            Err(TodayError::InvalidRequest)
        ));
        assert!(
            !runtime
                .phase1_begin_media_retrieval(
                    &context,
                    [3; 32],
                    Phase1InboundMediaPending::new(
                        [4; 16],
                        Phase1MediaConfigurationFingerprint::new([5; 32]).unwrap(),
                        1,
                    )
                    .unwrap(),
                )
                .await
                .expect("unmatched media")
        );

        ingest(&runtime, &context, note.clone(), 2_000_000_100).await;
        let page = runtime
            .phase1_today_page(&context, TodayPageRequest::first(1, 2_000_000_200))
            .await
            .expect("page");
        let card_id = page.items[0].card.card_id;
        let rank = page.items[0].card.rank.expect("rank");
        let generation = projection_generation().expect("generation");
        let storage = runtime.client.storage().expect("storage");
        let state = load_state(storage, &context, generation)
            .await
            .expect("load state")
            .expect("state");
        let scope = CursorScope::new(
            context.id.clone(),
            context.generation,
            2_000_000_200,
            state.store_generation,
            state.content_generation,
        )
        .expect("scope");

        let cursor_for = |scope: CursorScope| {
            TodayCursor::encode(&scope, TodayCursorPosition { rank })
                .expect("cursor")
                .as_str()
                .to_owned()
        };
        let mut wrong_context = scope.clone();
        wrong_context.context_id = "elsewhere".into();
        assert!(matches!(
            runtime
                .phase1_today_page(
                    &context,
                    TodayPageRequest::after(1, cursor_for(wrong_context))
                )
                .await,
            Err(TodayError::Cursor(CursorError::ContextMismatch))
        ));
        let mut wrong_context_generation = scope.clone();
        wrong_context_generation.context_generation += 1;
        assert!(matches!(
            runtime
                .phase1_today_page(
                    &context,
                    TodayPageRequest::after(1, cursor_for(wrong_context_generation)),
                )
                .await,
            Err(TodayError::Cursor(CursorError::ContextMismatch))
        ));
        assert!(matches!(
            runtime
                .phase1_today_page(
                    &context,
                    TodayPageRequest {
                        limit: 1,
                        as_of: Some(scope.as_of + 1),
                        cursor: Some(cursor_for(scope.clone())),
                    },
                )
                .await,
            Err(TodayError::Cursor(CursorError::SnapshotMismatch))
        ));
        let mut stale = scope.clone();
        stale.store_generation = [9; 32];
        assert!(matches!(
            runtime
                .phase1_today_page(&context, TodayPageRequest::after(1, cursor_for(stale)))
                .await,
            Err(TodayError::Cursor(CursorError::Stale))
        ));
        let mut missing = scope.clone();
        missing.projection_generation = missing.projection_generation.wrapping_add(1).max(1);
        assert!(matches!(
            runtime
                .phase1_today_page(&context, TodayPageRequest::after(1, cursor_for(missing)))
                .await,
            Err(TodayError::SnapshotMissing)
        ));

        let snapshot = frozen_snapshot(&state, &context, scope.as_of).expect("snapshot");
        assert!(validate_snapshot(&snapshot, &scope).is_ok());
        for invalid in [
            {
                let mut value = snapshot.clone();
                value.schema_version += 1;
                value
            },
            {
                let mut value = snapshot.clone();
                value.context_id = "other".into();
                value
            },
            {
                let mut value = snapshot.clone();
                value.context_generation += 1;
                value
            },
            {
                let mut value = snapshot.clone();
                value.as_of += 1;
                value
            },
            {
                let mut value = snapshot.clone();
                value.store_generation = [8; 32];
                value
            },
            {
                let mut value = snapshot.clone();
                value.projection_generation = value.projection_generation.wrapping_add(1);
                value
            },
        ] {
            assert!(matches!(
                validate_snapshot(&invalid, &scope),
                Err(TodayError::CorruptProjection)
            ));
        }
        let mut absent_rank = rank;
        absent_rank.card_id = CardId::parse(&"f".repeat(64)).expect("absent card id");
        assert!(matches!(
            page_from_snapshot(snapshot.clone(), scope.clone(), Some(absent_rank), 1),
            Err(TodayError::CursorPositionMissing)
        ));

        for invalid in [
            {
                let mut value = state.clone();
                value.schema_version += 1;
                value
            },
            {
                let mut value = state.clone();
                value.content_generation = 0;
                value
            },
            {
                let mut value = state.clone();
                value.source_events += 1;
                value
            },
        ] {
            assert!(matches!(
                decode_state(&encode(&invalid).expect("encode invalid state")),
                Err(TodayError::CorruptProjection)
            ));
        }
        let mut invalid_snapshot = snapshot.clone();
        invalid_snapshot.schema_version += 1;
        assert!(matches!(
            decode_snapshot(&encode(&invalid_snapshot).expect("encode invalid snapshot")),
            Err(TodayError::CorruptProjection)
        ));
        assert!(matches!(
            decode_state(b"not-json"),
            Err(TodayError::CorruptProjection)
        ));

        for overlay in [
            LocalAuthorOverlay {
                operation_id: String::new(),
                state: "queued".into(),
            },
            LocalAuthorOverlay {
                operation_id: "x".repeat(257),
                state: "queued".into(),
            },
            LocalAuthorOverlay {
                operation_id: "operation".into(),
                state: String::new(),
            },
            LocalAuthorOverlay {
                operation_id: "operation".into(),
                state: "x".repeat(97),
            },
            LocalAuthorOverlay {
                operation_id: "operation".into(),
                state: "bad\nstate".into(),
            },
        ] {
            assert!(matches!(
                runtime
                    .phase1_set_local_author_overlay(&context, card_id, Some(overlay))
                    .await,
                Err(TodayError::InvalidRequest)
            ));
        }
        let other_keys = Keys::generate();
        runtime.store_public_key = Some(
            radroots_identity::PublicKey::from_hex(&other_keys.public_key().to_string())
                .expect("other public key"),
        );
        assert!(matches!(
            runtime
                .phase1_me(&context, &keys().public_key().to_string(), 1)
                .await,
            Err(TodayError::InvalidRequest)
        ));
        assert!(matches!(
            runtime
                .phase1_set_local_author_overlay(
                    &context,
                    card_id,
                    Some(LocalAuthorOverlay {
                        operation_id: "operation".into(),
                        state: "queued".into(),
                    }),
                )
                .await,
            Err(TodayError::InvalidRequest)
        ));
        runtime.store_public_key = None;
        runtime
            .phase1_set_local_author_overlay(
                &context,
                card_id,
                Some(LocalAuthorOverlay {
                    operation_id: "operation".into(),
                    state: "queued".into(),
                }),
            )
            .await
            .expect("set overlay");
        runtime
            .phase1_set_local_author_overlay(&context, card_id, None)
            .await
            .expect("remove overlay");
        assert_eq!(
            runtime
                .phase1_search(&context, "guarded", 1, 2_000_000_200)
                .await
                .expect("card-limited search")
                .len(),
            1
        );

        let mut event_state = state.clone();
        event_state.cards[0].card.card_type = TodayCardType::Event;
        event_state.cards[0].card.event_start = Some(100);
        event_state.cards[0].card.event_end = Some(200);
        event_state.cards[0].card.effective_at = 100;
        assert_eq!(
            ranked_cards(&event_state, &context, 150).expect("live event")[0]
                .card
                .lifecycle,
            CardLifecycleState::Active
        );
        assert_eq!(
            ranked_cards(&event_state, &context, 200).expect("past event")[0]
                .card
                .lifecycle,
            CardLifecycleState::Past
        );

        let root = admitted(&note);
        assert!(matches!(
            profile_summary(&root),
            Err(TodayError::CorruptProjection)
        ));
        assert!(matches!(
            reply_entry(&root),
            Err(TodayError::CorruptProjection)
        ));
        assert!(comment_entry(&root).is_none());
        assert_eq!(tag_value(&[Vec::new()], &["x"]), None);
        assert_eq!(tag_value(&[vec!["x".into()]], &["x"]), None);
        assert_eq!(blossom_digest("not-a-url"), None);
        assert_eq!(blossom_digest("https://blob.example/short"), None);
        assert_eq!(
            blossom_digest(&format!("https://blob.example/{}", "A".repeat(64))),
            None
        );

        let tags = locality_tags(vec![
            Vec::new(),
            vec!["x".into(), "ignored".into()],
            vec!["g".into()],
            vec!["location".into(), " ".into()],
            vec!["g".into(), "u10hr".into()],
        ]);
        assert_eq!(tags.len(), 1);
        assert_eq!(
            locality_evidence(Some("u10"), &tags),
            LocalityEvidence::Match
        );
        assert_eq!(
            locality_evidence(Some("u10hr7"), &tags),
            LocalityEvidence::Match
        );
        assert_eq!(
            locality_evidence(Some("other"), &tags),
            LocalityEvidence::Nonmatch
        );
        assert_eq!(
            locality_evidence(Some("selected"), &[]),
            LocalityEvidence::Missing
        );

        let mut fields = BTreeMap::new();
        fields.insert("value".into(), serde_json::json!(1));
        assert_eq!(typed_profile_extra(&fields, "missing"), None);
        assert_eq!(typed_profile_extra(&fields, "value"), None);
        for value in [String::new(), "x".repeat(2_049), "bad\nvalue".into()] {
            fields.insert("value".into(), serde_json::Value::String(value));
            assert_eq!(typed_profile_extra(&fields, "value"), None);
        }
    }

    #[test]
    fn malformed_cursor_and_request_bounds_fail_closed() {
        assert!(matches!(
            TodayCursor::scope("bad"),
            Err(CursorError::Malformed)
        ));
        assert!(!valid_public_key("A".repeat(64).as_str()));
        assert_eq!(TODAY_PAGE_LIMIT_MAX, 100);
        assert_eq!(TODAY_SEARCH_LIMIT_MAX, 100);
    }
}
