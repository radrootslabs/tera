use std::collections::BTreeSet;

use radroots_blossom::{
    BlobUrl, ByteVerifiedDescriptor, MediaType, authorization::AuthoredUploadClaim,
};
use radroots_event::{
    contract::AuthorRole,
    post::deletion::{
        AuthoredNip09DeletionRequest, Nip09DeletionAddressTarget, Nip09DeletionEventTarget,
    },
};
use radroots_event_codec::authoring::PlanWireV1;
use radroots_identity::PublicKey;
use radroots_signing::{
    Actor, AuthoredArtifactId, SigningIntentId, SigningOperationId,
    actor::ActorSource,
    request::{CancellationPolicy, SignPolicy},
};
use radroots_storage::{
    authored::{AdmissionState, SigningState},
    authored_delivery::{AuthoredDeliveryState, DeliveryAttemptOutcome},
    authored_draft::{
        AuthoredDraft, AuthoredDraftId, AuthoredDraftRevision, AuthoredDraftStage,
        AuthoredDraftStore,
    },
    journal::{IdempotencyKey, OperationInstanceId},
};
use radroots_sync::{PushRequest, PushStatus, policy::SyncId};
use radroots_transport::{
    Target, TargetSet,
    outcome::DeliveryOutcomeKind,
    policy::{SatisfactionClass, SatisfactionPolicy, TargetPolicy},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::{
    AddCommandType, CardId, CardSourceIdentity, LocalAuthorOverlay, LocalNetwork, Phase1AddCommand,
    TodayCardType, TodayError, phase1_retraction_plan,
};
use crate::runtime::RadrootsRuntime;

const DRAFT_PAYLOAD_SCHEMA: &str = "radroots.mobile.phase1-draft.v1";
const DRAFT_SCHEMA_VERSION: u16 = 1;
const DRAFT_MEDIA_MAX: usize = 20;
const DRAFT_LOCAL_REFERENCE_MAX_BYTES: usize = 4_096;
const DRAFT_FAILURE_CODE_MAX_BYTES: usize = 96;
const DRAFT_OPERATION_DOMAIN: &[u8] = b"radroots.mobile.phase1-draft-operation.v1\0";

/// Product intent represented by one durable draft/outbox item.
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase1DraftKind {
    #[default]
    Add,
    Retraction,
}

/// Event timing profile retained for a reopenable Add form.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase1DraftEventTiming {
    AllDay,
    Timed,
}

/// Secret-free, restart-safe media fields retained with an Add form.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Phase1DraftMediaSnapshot {
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

/// Immutable, validated presentation input used to reopen a durable draft.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Phase1DraftFormSnapshot {
    pub command_type: AddCommandType,
    pub content: String,
    pub identifier: Option<String>,
    pub title: Option<String>,
    pub summary: Option<String>,
    pub location: Option<String>,
    pub event_timing: Option<Phase1DraftEventTiming>,
    pub event_start_date: Option<String>,
    pub event_end_date: Option<String>,
    pub event_start_unix_s: Option<u64>,
    pub event_end_unix_s: Option<u64>,
    pub event_timezone: Option<String>,
    pub price_amount: Option<String>,
    pub currency: Option<String>,
    pub unit: Option<String>,
    pub quantity: Option<String>,
    pub food_status: Option<String>,
    pub media: Vec<Phase1DraftMediaSnapshot>,
}

impl Phase1DraftFormSnapshot {
    fn validate(
        &self,
        command_type: AddCommandType,
        media: &[Phase1MediaPrerequisite],
    ) -> Result<(), Phase1DraftError> {
        let bounded = |value: &str, maximum: usize| {
            value.len() <= maximum && !value.chars().any(char::is_control)
        };
        if self.command_type != command_type
            || self.content.len() > 65_535
            || self.media.len() != media.len()
            || [
                self.identifier.as_deref(),
                self.title.as_deref(),
                self.summary.as_deref(),
                self.location.as_deref(),
                self.event_start_date.as_deref(),
                self.event_end_date.as_deref(),
                self.event_timezone.as_deref(),
                self.price_amount.as_deref(),
                self.currency.as_deref(),
                self.unit.as_deref(),
                self.quantity.as_deref(),
                self.food_status.as_deref(),
            ]
            .into_iter()
            .flatten()
            .any(|value| !bounded(value, 1_024))
        {
            return Err(Phase1DraftError::InvalidDraft);
        }
        for (snapshot, prerequisite) in self.media.iter().zip(media) {
            if snapshot.opaque_reference.is_empty()
                || snapshot.opaque_reference != prerequisite.local_reference()
                || snapshot.url != prerequisite.url
                || snapshot.sha256 != prerequisite.sha256()
                || snapshot.media_type != prerequisite.media_type()
                || snapshot.byte_size != prerequisite.byte_size()
                || snapshot.width == 0
                || snapshot.height == 0
                || snapshot.alt.trim().is_empty()
                || !bounded(&snapshot.alt, 1_024)
                || snapshot.prepared_at_unix_s == 0
            {
                return Err(Phase1DraftError::InvalidMedia);
            }
        }
        Ok(())
    }
}

/// Durable state of one media prerequisite referenced by an Add command.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase1MediaStage {
    Pending,
    Preparing,
    Uploading,
    Verified,
    Failed,
    Orphaned,
}

/// Exact local and remote identity of one Add media prerequisite.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Phase1MediaPrerequisite {
    local_reference: String,
    url: String,
    sha256: String,
    media_type: String,
    byte_size: u64,
    stage: Phase1MediaStage,
    failure_code: Option<String>,
    upload_attempts: u8,
    verified_at_unix_ms: Option<u64>,
    orphan: Option<Phase1MediaOrphanRecord>,
}

/// Durable, secret-safe evidence that a remote blob may be unreferenced.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Phase1MediaOrphanRecord {
    reason_code: String,
    recorded_at_unix_ms: u64,
}

impl Phase1MediaOrphanRecord {
    pub fn reason_code(&self) -> &str {
        self.reason_code.as_str()
    }

    pub const fn recorded_at_unix_ms(&self) -> u64 {
        self.recorded_at_unix_ms
    }
}

impl Phase1MediaPrerequisite {
    pub fn new(
        local_reference: impl Into<String>,
        descriptor: &ByteVerifiedDescriptor,
    ) -> Result<Self, Phase1DraftError> {
        let value = Self {
            local_reference: local_reference.into(),
            url: descriptor.url().as_blob_url().as_str().to_owned(),
            sha256: descriptor.sha256().to_hex(),
            media_type: descriptor.media_type().as_str().to_owned(),
            byte_size: descriptor.size(),
            stage: Phase1MediaStage::Pending,
            failure_code: None,
            upload_attempts: 0,
            verified_at_unix_ms: None,
            orphan: None,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), Phase1DraftError> {
        let blob = BlobUrl::parse(self.url.as_str()).map_err(|_| Phase1DraftError::InvalidMedia)?;
        let hash = blob.hash_path().hash().to_string();
        if self.local_reference.is_empty()
            || self.local_reference.len() > DRAFT_LOCAL_REFERENCE_MAX_BYTES
            || self.local_reference != self.local_reference.trim()
            || self.local_reference.chars().any(char::is_control)
            || self.sha256 != hash
            || MediaType::parse(self.media_type.as_str()).is_err()
            || self.byte_size == 0
            || self.failure_code.as_ref().is_some_and(|code| {
                code.is_empty()
                    || code.len() > DRAFT_FAILURE_CODE_MAX_BYTES
                    || code != code.trim()
                    || code.chars().any(char::is_control)
            })
            || self.orphan.as_ref().is_some_and(|record| {
                record.reason_code.is_empty()
                    || record.reason_code.len() > DRAFT_FAILURE_CODE_MAX_BYTES
                    || record.reason_code != record.reason_code.trim()
                    || record.reason_code.chars().any(char::is_control)
                    || record.recorded_at_unix_ms == 0
            })
            || match self.stage {
                Phase1MediaStage::Pending | Phase1MediaStage::Preparing => {
                    self.failure_code.is_some()
                        || self.upload_attempts != 0
                        || self.verified_at_unix_ms.is_some()
                        || self.orphan.is_some()
                }
                Phase1MediaStage::Uploading => {
                    self.failure_code.is_some()
                        || self.verified_at_unix_ms.is_some()
                        || self.orphan.is_some()
                }
                Phase1MediaStage::Verified => {
                    self.failure_code.is_some()
                        || self.upload_attempts == 0
                        || self.verified_at_unix_ms.is_none()
                        || self.orphan.is_some()
                }
                Phase1MediaStage::Failed => {
                    self.failure_code.is_none() || self.verified_at_unix_ms.is_some()
                }
                Phase1MediaStage::Orphaned => self.failure_code.is_some() || self.orphan.is_none(),
            }
        {
            return Err(Phase1DraftError::InvalidMedia);
        }
        Ok(())
    }

    pub fn url(&self) -> &str {
        self.url.as_str()
    }
    pub fn local_reference(&self) -> &str {
        self.local_reference.as_str()
    }
    pub fn sha256(&self) -> &str {
        self.sha256.as_str()
    }
    pub fn media_type(&self) -> &str {
        self.media_type.as_str()
    }
    pub const fn byte_size(&self) -> u64 {
        self.byte_size
    }
    pub const fn stage(&self) -> Phase1MediaStage {
        self.stage
    }

    pub const fn upload_attempts(&self) -> u8 {
        self.upload_attempts
    }

    pub const fn verified_at_unix_ms(&self) -> Option<u64> {
        self.verified_at_unix_ms
    }

    pub const fn orphan(&self) -> Option<&Phase1MediaOrphanRecord> {
        self.orphan.as_ref()
    }

    fn matches_receipt(&self, receipt: &radroots_sdk::transport::BlossomUploadReceipt) -> bool {
        let descriptor = receipt.descriptor();
        self.url == descriptor.url().as_blob_url().as_str()
            && self.sha256 == descriptor.sha256().to_hex()
            && self.media_type == descriptor.media_type().as_str()
            && self.byte_size == descriptor.size()
            && receipt.attempts() > 0
            && receipt.verified_at_unix_ms() > 0
    }

    fn is_remote_verified(&self) -> bool {
        self.stage == Phase1MediaStage::Verified
            && self.upload_attempts > 0
            && self.verified_at_unix_ms.is_some()
    }
}

/// Closed delivery-satisfaction profiles available to Phase 1 Add.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase1RelaySatisfaction {
    AnyAccepted,
    AllAccepted,
    AnyDelivered,
    AllDelivered,
}

/// Stable product-level cancellation policy persisted with queue intent.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Phase1CancellationPolicy {
    PreservePublishedRequest,
    LocalCooperative,
}

impl Phase1CancellationPolicy {
    const fn signing(self) -> CancellationPolicy {
        match self {
            Self::PreservePublishedRequest => CancellationPolicy::PreservePublishedRequest,
            Self::LocalCooperative => CancellationPolicy::LocalCooperative,
        }
    }
}

/// Exact relay and deadline intent frozen before an operation is prepared.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Phase1QueuePolicy {
    relay_urls: Vec<String>,
    satisfaction: Phase1RelaySatisfaction,
    delivery_deadline_unix_ms: u64,
    cancellation: Phase1CancellationPolicy,
}

impl Phase1QueuePolicy {
    pub fn new(
        relay_urls: Vec<String>,
        satisfaction: Phase1RelaySatisfaction,
        delivery_deadline_unix_ms: u64,
        cancellation: Phase1CancellationPolicy,
    ) -> Result<Self, Phase1DraftError> {
        let value = Self {
            relay_urls,
            satisfaction,
            delivery_deadline_unix_ms,
            cancellation,
        };
        value.materialize()?;
        Ok(value)
    }

    fn materialize(
        &self,
    ) -> Result<(TargetSet, SatisfactionPolicy, CancellationPolicy), Phase1DraftError> {
        if self.delivery_deadline_unix_ms == 0 || self.relay_urls.is_empty() {
            return Err(Phase1DraftError::InvalidQueuePolicy);
        }
        let mut canonical = BTreeSet::new();
        let targets = self
            .relay_urls
            .iter()
            .map(|url| {
                let target =
                    Target::nostr_relay(url).map_err(|_| Phase1DraftError::InvalidQueuePolicy)?;
                if target.uri().as_str() != url || !canonical.insert(url.as_str()) {
                    return Err(Phase1DraftError::InvalidQueuePolicy);
                }
                Ok(target)
            })
            .collect::<Result<Vec<_>, _>>()?;
        let targets = TargetSet::new(targets).map_err(|_| Phase1DraftError::InvalidQueuePolicy)?;
        let (class, target_policy) = match self.satisfaction {
            Phase1RelaySatisfaction::AnyAccepted => {
                (SatisfactionClass::Accepted, TargetPolicy::any())
            }
            Phase1RelaySatisfaction::AllAccepted => {
                (SatisfactionClass::Accepted, TargetPolicy::all())
            }
            Phase1RelaySatisfaction::AnyDelivered => {
                (SatisfactionClass::Delivered, TargetPolicy::any())
            }
            Phase1RelaySatisfaction::AllDelivered => {
                (SatisfactionClass::Delivered, TargetPolicy::all())
            }
        };
        let cancellation = match self.cancellation {
            Phase1CancellationPolicy::PreservePublishedRequest => {
                CancellationPolicy::PreservePublishedRequest
            }
            Phase1CancellationPolicy::LocalCooperative => CancellationPolicy::LocalCooperative,
        };
        Ok((
            targets,
            SatisfactionPolicy::new(class, target_policy),
            cancellation,
        ))
    }
}

/// Honest aggregate state for a local draft and its durable authored operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase1OutboxState {
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

impl Phase1OutboxState {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Draft => "draft",
            Self::MediaPreparing => "media_preparing",
            Self::MediaUploading => "media_uploading",
            Self::ReadyToSign => "ready_to_sign",
            Self::Signing => "signing",
            Self::Signed => "signed",
            Self::Queued => "queued",
            Self::Delivering => "delivering",
            Self::PartiallyDelivered => "partially_delivered",
            Self::Retryable => "retryable",
            Self::Terminal => "terminal",
            Self::Cancelled => "cancelled",
            Self::Complete => "complete",
        }
    }
}

/// Current reconstructable product view of one Add draft.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Phase1DraftStatus {
    draft: AuthoredDraft,
    kind: Phase1DraftKind,
    command_type: AddCommandType,
    form: Option<Phase1DraftFormSnapshot>,
    media: Vec<Phase1MediaPrerequisite>,
    state: Phase1OutboxState,
    card_id: CardId,
    push: Option<PushStatus>,
}

impl Phase1DraftStatus {
    pub const fn draft(&self) -> &AuthoredDraft {
        &self.draft
    }
    pub const fn command_type(&self) -> AddCommandType {
        self.command_type
    }
    pub const fn kind(&self) -> Phase1DraftKind {
        self.kind
    }
    pub const fn form(&self) -> Option<&Phase1DraftFormSnapshot> {
        self.form.as_ref()
    }
    pub fn media(&self) -> &[Phase1MediaPrerequisite] {
        self.media.as_slice()
    }
    pub const fn state(&self) -> Phase1OutboxState {
        self.state
    }
    pub const fn card_id(&self) -> CardId {
        self.card_id
    }
    pub const fn push(&self) -> Option<&PushStatus> {
        self.push.as_ref()
    }
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum Phase1DraftError {
    #[error("authenticated draft identity is unavailable")]
    IdentityUnavailable,
    #[error("phase 1 draft input is invalid")]
    InvalidDraft,
    #[error("phase 1 media prerequisite is invalid")]
    InvalidMedia,
    #[error("phase 1 queue policy is invalid")]
    InvalidQueuePolicy,
    #[error("phase 1 draft revision conflicts with durable state")]
    RevisionConflict,
    #[error("phase 1 draft is not found")]
    NotFound,
    #[error("phase 1 draft is terminal")]
    Terminal,
    #[error("phase 1 draft media is not ready")]
    MediaNotReady,
    #[error("phase 1 authored operation is unavailable")]
    OperationUnavailable,
    #[error("phase 1 draft persistence failed")]
    Storage,
    #[error("phase 1 draft payload is corrupt")]
    Corrupt,
    #[error("phase 1 authored operation failed")]
    Operation,
    #[error("phase 1 Today overlay failed")]
    Overlay,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
struct Phase1DraftPayload {
    schema_version: u16,
    #[serde(default)]
    kind: Phase1DraftKind,
    command_type: AddCommandType,
    #[serde(default)]
    form: Option<Phase1DraftFormSnapshot>,
    #[serde(default)]
    target_card_id: Option<CardId>,
    plan_wire_json: Vec<u8>,
    media: Vec<Phase1MediaPrerequisite>,
    queue: Option<Phase1QueuePolicy>,
}

impl Phase1DraftPayload {
    fn new(
        command: &Phase1AddCommand,
        plan_wire_json: Vec<u8>,
        media: Vec<Phase1MediaPrerequisite>,
        form: Option<Phase1DraftFormSnapshot>,
    ) -> Result<Self, Phase1DraftError> {
        let value = Self {
            schema_version: DRAFT_SCHEMA_VERSION,
            kind: Phase1DraftKind::Add,
            command_type: command.command_type(),
            form,
            target_card_id: None,
            plan_wire_json,
            media,
            queue: None,
        };
        value.validate()?;
        Ok(value)
    }

    fn retraction(
        command_type: AddCommandType,
        target_card_id: CardId,
        plan_wire_json: Vec<u8>,
    ) -> Result<Self, Phase1DraftError> {
        let value = Self {
            schema_version: DRAFT_SCHEMA_VERSION,
            kind: Phase1DraftKind::Retraction,
            command_type,
            form: None,
            target_card_id: Some(target_card_id),
            plan_wire_json,
            media: Vec::new(),
            queue: None,
        };
        value.validate()?;
        Ok(value)
    }

    fn validate(&self) -> Result<(), Phase1DraftError> {
        if self.schema_version != DRAFT_SCHEMA_VERSION || self.media.len() > DRAFT_MEDIA_MAX {
            return Err(Phase1DraftError::Corrupt);
        }
        match self.kind {
            Phase1DraftKind::Add => {
                if self.target_card_id.is_some() {
                    return Err(Phase1DraftError::Corrupt);
                }
                if let Some(form) = &self.form {
                    form.validate(self.command_type, &self.media)?;
                }
            }
            Phase1DraftKind::Retraction => {
                if self.form.is_some() || self.target_card_id.is_none() || !self.media.is_empty() {
                    return Err(Phase1DraftError::Corrupt);
                }
            }
        }
        let integrity = PlanWireV1::from_json(self.plan_wire_json.as_slice())
            .map_err(|_| Phase1DraftError::Corrupt)?;
        let plan = integrity.plan();
        let expected_media = media_urls(plan.body().tags())?;
        let actual_media = self
            .media
            .iter()
            .map(|media| {
                media.validate()?;
                Ok(media.url.as_str())
            })
            .collect::<Result<BTreeSet<_>, Phase1DraftError>>()?;
        if expected_media != actual_media || actual_media.len() != self.media.len() {
            return Err(Phase1DraftError::InvalidMedia);
        }
        if let Some(queue) = &self.queue {
            queue.materialize()?;
        }
        Ok(())
    }

    fn decode(draft: &AuthoredDraft) -> Result<Self, Phase1DraftError> {
        if draft.payload_schema() != DRAFT_PAYLOAD_SCHEMA {
            return Err(Phase1DraftError::Corrupt);
        }
        let value = serde_json::from_slice::<Self>(draft.payload())
            .map_err(|_| Phase1DraftError::Corrupt)?;
        value.validate()?;
        let plan = PlanWireV1::from_json(value.plan_wire_json.as_slice())
            .map_err(|_| Phase1DraftError::Corrupt)?;
        if plan.plan().author().as_bytes() != draft.author() {
            return Err(Phase1DraftError::Corrupt);
        }
        Ok(value)
    }

    fn encode(&self) -> Result<Vec<u8>, Phase1DraftError> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|_| Phase1DraftError::InvalidDraft)
    }
}

impl RadrootsRuntime {
    /// Creates or replaces the editable content of one immutable-revision draft.
    #[allow(clippy::too_many_arguments)]
    pub async fn phase1_save_draft(
        &self,
        draft_id: [u8; 16],
        command: Phase1AddCommand,
        authored_at_unix_s: u64,
        media: Vec<Phase1MediaPrerequisite>,
        expected_revision: Option<u64>,
        persisted_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        self.phase1_save_draft_inner(
            draft_id,
            command,
            authored_at_unix_s,
            media,
            None,
            expected_revision,
            persisted_at_unix_ms,
        )
        .await
    }

    /// Creates or replaces a draft while retaining its validated, reopenable form.
    #[allow(clippy::too_many_arguments)]
    pub async fn phase1_save_draft_with_form(
        &self,
        draft_id: [u8; 16],
        command: Phase1AddCommand,
        authored_at_unix_s: u64,
        media: Vec<Phase1MediaPrerequisite>,
        form: Phase1DraftFormSnapshot,
        expected_revision: Option<u64>,
        persisted_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        self.phase1_save_draft_inner(
            draft_id,
            command,
            authored_at_unix_s,
            media,
            Some(form),
            expected_revision,
            persisted_at_unix_ms,
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn phase1_save_draft_inner(
        &self,
        draft_id: [u8; 16],
        command: Phase1AddCommand,
        authored_at_unix_s: u64,
        media: Vec<Phase1MediaPrerequisite>,
        form: Option<Phase1DraftFormSnapshot>,
        expected_revision: Option<u64>,
        persisted_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let author = self.draft_author()?;
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let plan = command
            .authored_plan(authored_at_unix_s, hex::encode(author))
            .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let wire = PlanWireV1::from_plan(&plan)
            .to_json()
            .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let payload = Phase1DraftPayload::new(&command, wire, media, form)?;
        let bytes = payload.encode()?;
        let storage = self.storage()?;
        let expected = expected_revision
            .map(AuthoredDraftRevision::new)
            .transpose()
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let draft = if let Some(expected) = expected {
            let head = storage
                .authored_draft_head(draft_id)
                .await
                .map_err(|_| Phase1DraftError::Storage)?
                .ok_or(Phase1DraftError::NotFound)?;
            if head.revision() != expected
                || head.stage().is_terminal()
                || matches!(
                    head.stage(),
                    AuthoredDraftStage::ReadyToSign | AuthoredDraftStage::Queued
                )
            {
                return Err(Phase1DraftError::RevisionConflict);
            }
            head.successor(
                bytes,
                draft_stage_for_media(&payload.media),
                None,
                persisted_at_unix_ms,
            )
            .map_err(|_| Phase1DraftError::RevisionConflict)?
        } else {
            AuthoredDraft::initial(
                draft_id,
                author,
                DRAFT_PAYLOAD_SCHEMA,
                bytes,
                draft_stage_for_media(&payload.media),
                None,
                persisted_at_unix_ms,
            )
            .map_err(|_| Phase1DraftError::InvalidDraft)?
        };
        let receipt = storage
            .append_authored_draft(draft, expected)
            .await
            .map_err(map_draft_storage_error)?;
        self.draft_status_from(receipt.draft().clone()).await
    }

    /// Persists an independent strict NIP-09 retraction as a normal durable outbox item.
    #[allow(clippy::too_many_arguments)]
    pub async fn phase1_save_retraction_draft(
        &self,
        draft_id: [u8; 16],
        command_type: AddCommandType,
        target_card_id: CardId,
        target_event_id: &str,
        target_kind: u32,
        target_address: Option<&str>,
        reason: &str,
        authored_at_unix_s: u64,
        persisted_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let target_shape_valid = match command_type {
            AddCommandType::CreateUpdate
            | AddCommandType::CreatePhotoUpdate
            | AddCommandType::CreateAsk => target_kind == 1 && target_address.is_none(),
            AddCommandType::CreateEvent => {
                matches!(target_kind, 31_922 | 31_923) && target_address.is_some()
            }
            AddCommandType::CreateFoodAvailability => {
                target_kind == 30_402 && target_address.is_some()
            }
        };
        if !target_shape_valid || authored_at_unix_s == 0 || persisted_at_unix_ms == 0 {
            return Err(Phase1DraftError::InvalidDraft);
        }
        let author = self.draft_author()?;
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let event_target = Nip09DeletionEventTarget::parse(target_event_id, target_kind)
            .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let address_targets = target_address
            .map(Nip09DeletionAddressTarget::parse)
            .transpose()
            .map_err(|_| Phase1DraftError::InvalidDraft)?
            .into_iter()
            .collect();
        let request =
            AuthoredNip09DeletionRequest::new(reason, vec![event_target], address_targets)
                .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let plan = phase1_retraction_plan(&request, authored_at_unix_s, hex::encode(author))
            .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let wire = PlanWireV1::from_plan(&plan)
            .to_json()
            .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let payload = Phase1DraftPayload::retraction(command_type, target_card_id, wire)?;
        let bytes = payload.encode()?;
        let draft = AuthoredDraft::initial(
            draft_id,
            author,
            DRAFT_PAYLOAD_SCHEMA,
            bytes,
            AuthoredDraftStage::Draft,
            None,
            persisted_at_unix_ms,
        )
        .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let receipt = self
            .storage()?
            .append_authored_draft(draft, None)
            .await
            .map_err(map_draft_storage_error)?;
        self.draft_status_from(receipt.draft().clone()).await
    }

    /// Advances one media prerequisite without mutating any prior revision.
    pub async fn phase1_update_draft_media(
        &self,
        draft_id: [u8; 16],
        expected_revision: u64,
        url: &str,
        stage: Phase1MediaStage,
        failure_code: Option<String>,
        updated_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let expected = AuthoredDraftRevision::new(expected_revision)
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let storage = self.storage()?;
        let head = storage
            .authored_draft_head(draft_id)
            .await
            .map_err(|_| Phase1DraftError::Storage)?
            .ok_or(Phase1DraftError::NotFound)?;
        if head.revision() != expected
            || head.stage().is_terminal()
            || matches!(
                head.stage(),
                AuthoredDraftStage::ReadyToSign | AuthoredDraftStage::Queued
            )
        {
            return Err(Phase1DraftError::RevisionConflict);
        }
        let mut payload = Phase1DraftPayload::decode(&head)?;
        let media = payload
            .media
            .iter_mut()
            .find(|media| media.url == url)
            .ok_or(Phase1DraftError::InvalidMedia)?;
        if !valid_media_transition(media.stage, stage)
            || matches!(
                stage,
                Phase1MediaStage::Verified | Phase1MediaStage::Orphaned
            )
        {
            return Err(Phase1DraftError::InvalidMedia);
        }
        media.stage = stage;
        media.failure_code = failure_code;
        if stage != Phase1MediaStage::Failed {
            media.failure_code = None;
        }
        media.validate()?;
        let next_stage = match stage {
            Phase1MediaStage::Pending | Phase1MediaStage::Preparing => {
                AuthoredDraftStage::MediaPreparing
            }
            Phase1MediaStage::Uploading
            | Phase1MediaStage::Verified
            | Phase1MediaStage::Failed
            | Phase1MediaStage::Orphaned => AuthoredDraftStage::MediaUploading,
        };
        let next = head
            .successor(payload.encode()?, next_stage, None, updated_at_unix_ms)
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let receipt = storage
            .append_authored_draft(next, Some(expected))
            .await
            .map_err(map_draft_storage_error)?;
        self.draft_status_from(receipt.draft().clone()).await
    }

    /// Records the only proof that can advance media to remote-byte-verified.
    pub async fn phase1_complete_draft_media(
        &self,
        draft_id: [u8; 16],
        expected_revision: u64,
        url: &str,
        receipt: radroots_sdk::transport::BlossomUploadReceipt,
        updated_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let expected = AuthoredDraftRevision::new(expected_revision)
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let storage = self.storage()?;
        let head = storage
            .authored_draft_head(draft_id)
            .await
            .map_err(|_| Phase1DraftError::Storage)?
            .ok_or(Phase1DraftError::NotFound)?;
        if head.revision() != expected
            || head.stage().is_terminal()
            || matches!(
                head.stage(),
                AuthoredDraftStage::ReadyToSign | AuthoredDraftStage::Queued
            )
        {
            return Err(Phase1DraftError::RevisionConflict);
        }
        let mut payload = Phase1DraftPayload::decode(&head)?;
        let media = payload
            .media
            .iter_mut()
            .find(|media| media.url == url)
            .ok_or(Phase1DraftError::InvalidMedia)?;
        if media.stage != Phase1MediaStage::Uploading || !media.matches_receipt(&receipt) {
            return Err(Phase1DraftError::InvalidMedia);
        }
        media.stage = Phase1MediaStage::Verified;
        media.failure_code = None;
        media.upload_attempts = receipt.attempts();
        media.verified_at_unix_ms = Some(receipt.verified_at_unix_ms());
        media.orphan = None;
        media.validate()?;
        let next = head
            .successor(
                payload.encode()?,
                AuthoredDraftStage::MediaUploading,
                None,
                updated_at_unix_ms,
            )
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let receipt = storage
            .append_authored_draft(next, Some(expected))
            .await
            .map_err(map_draft_storage_error)?;
        self.draft_status_from(receipt.draft().clone()).await
    }

    /// Persists a redacted recoverable Blossom failure and possible-orphan evidence.
    pub async fn phase1_fail_draft_media(
        &self,
        draft_id: [u8; 16],
        expected_revision: u64,
        url: &str,
        error: &radroots_sdk::transport::BlossomError,
        updated_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        if updated_at_unix_ms == 0 {
            return Err(Phase1DraftError::InvalidMedia);
        }
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let expected = AuthoredDraftRevision::new(expected_revision)
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let storage = self.storage()?;
        let head = storage
            .authored_draft_head(draft_id)
            .await
            .map_err(|_| Phase1DraftError::Storage)?
            .ok_or(Phase1DraftError::NotFound)?;
        if head.revision() != expected
            || head.stage().is_terminal()
            || matches!(
                head.stage(),
                AuthoredDraftStage::ReadyToSign | AuthoredDraftStage::Queued
            )
        {
            return Err(Phase1DraftError::RevisionConflict);
        }
        let mut payload = Phase1DraftPayload::decode(&head)?;
        let media = payload
            .media
            .iter_mut()
            .find(|media| media.url == url)
            .ok_or(Phase1DraftError::InvalidMedia)?;
        if !matches!(
            media.stage,
            Phase1MediaStage::Pending
                | Phase1MediaStage::Preparing
                | Phase1MediaStage::Uploading
                | Phase1MediaStage::Failed
        ) {
            return Err(Phase1DraftError::InvalidMedia);
        }
        media.stage = Phase1MediaStage::Failed;
        media.failure_code = Some(error.code().to_owned());
        media.upload_attempts = error.attempts();
        media.verified_at_unix_ms = None;
        media.orphan = error.possible_orphan().then(|| Phase1MediaOrphanRecord {
            reason_code: error.code().to_owned(),
            recorded_at_unix_ms: updated_at_unix_ms,
        });
        media.validate()?;
        let next = head
            .successor(
                payload.encode()?,
                AuthoredDraftStage::MediaUploading,
                None,
                updated_at_unix_ms,
            )
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let receipt = storage
            .append_authored_draft(next, Some(expected))
            .await
            .map_err(map_draft_storage_error)?;
        self.draft_status_from(receipt.draft().clone()).await
    }

    /// Freezes queue intent and atomically prepares the canonical outbox before
    /// returning `queued`. Network connectivity is neither read nor required.
    pub async fn phase1_queue_draft(
        &self,
        draft_id: [u8; 16],
        expected_revision: u64,
        policy: Phase1QueuePolicy,
        queued_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let expected = AuthoredDraftRevision::new(expected_revision)
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let storage = self.storage()?;
        let head = storage
            .authored_draft_head(draft_id)
            .await
            .map_err(|_| Phase1DraftError::Storage)?
            .ok_or(Phase1DraftError::NotFound)?;
        if head.revision() != expected {
            return Err(Phase1DraftError::RevisionConflict);
        }
        let mut payload = Phase1DraftPayload::decode(&head)?;
        let ready = match head.stage() {
            AuthoredDraftStage::ReadyToSign => {
                if payload.queue.as_ref() != Some(&policy) {
                    return Err(Phase1DraftError::RevisionConflict);
                }
                head
            }
            AuthoredDraftStage::Queued => {
                if payload.queue.as_ref() != Some(&policy) {
                    return Err(Phase1DraftError::RevisionConflict);
                }
                return self.draft_status_from(head).await;
            }
            AuthoredDraftStage::Cancelled => return Err(Phase1DraftError::Terminal),
            AuthoredDraftStage::Draft
            | AuthoredDraftStage::MediaPreparing
            | AuthoredDraftStage::MediaUploading => {
                if payload
                    .media
                    .iter()
                    .any(|media| !media.is_remote_verified())
                {
                    return Err(Phase1DraftError::MediaNotReady);
                }
                policy.materialize()?;
                payload.queue = Some(policy);
                let bytes = payload.encode()?;
                let operation_id = operation_id(draft_id, bytes.as_slice())?;
                let operation_id = OperationInstanceId::new(*operation_id.as_bytes())
                    .map_err(|_| Phase1DraftError::InvalidDraft)?;
                let ready = head
                    .successor(
                        bytes,
                        AuthoredDraftStage::ReadyToSign,
                        Some(operation_id),
                        queued_at_unix_ms,
                    )
                    .map_err(|_| Phase1DraftError::RevisionConflict)?;
                storage
                    .append_authored_draft(ready.clone(), Some(expected))
                    .await
                    .map_err(map_draft_storage_error)?;
                ready
            }
        };
        self.finish_queue(ready, queued_at_unix_ms).await
    }

    /// Resumes a queue transition interrupted after its durable ready-to-sign
    /// checkpoint, including the crash window after outbox preparation.
    pub async fn phase1_recover_draft_queue(
        &self,
        draft_id: [u8; 16],
        recovered_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let head = self
            .storage()?
            .authored_draft_head(draft_id)
            .await
            .map_err(|_| Phase1DraftError::Storage)?
            .ok_or(Phase1DraftError::NotFound)?;
        match head.stage() {
            AuthoredDraftStage::ReadyToSign => self.finish_queue(head, recovered_at_unix_ms).await,
            AuthoredDraftStage::Queued | AuthoredDraftStage::Cancelled => {
                self.draft_status_from(head).await
            }
            AuthoredDraftStage::Draft
            | AuthoredDraftStage::MediaPreparing
            | AuthoredDraftStage::MediaUploading => Err(Phase1DraftError::InvalidDraft),
        }
    }

    /// Invokes the configured opaque host signer for one durably queued draft.
    ///
    /// The canonical sync engine verifies author, event ID, exact fields,
    /// signature, deadline, cancellation, and operation binding before the
    /// signed artifact can be persisted. Delivery remains a separate phase.
    pub async fn phase1_sign_queued_draft(
        &self,
        draft_id: [u8; 16],
        expected_revision: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let expected = AuthoredDraftRevision::new(expected_revision)
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let head = self
            .storage()?
            .authored_draft_head(draft_id)
            .await
            .map_err(|_| Phase1DraftError::Storage)?
            .ok_or(Phase1DraftError::NotFound)?;
        if head.revision() != expected || head.stage() != AuthoredDraftStage::Queued {
            return Err(Phase1DraftError::RevisionConflict);
        }
        self.sync()?
            .sign_prepared(push_request(&head)?)
            .await
            .map_err(|_| Phase1DraftError::Operation)?;
        self.draft_status_from(head).await
    }

    /// Advances one durably queued draft through signing, local admission, and
    /// at most one bounded relay-delivery attempt.
    pub async fn phase1_advance_draft(
        &self,
        draft_id: [u8; 16],
        expected_revision: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let expected = AuthoredDraftRevision::new(expected_revision)
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let head = self
            .storage()?
            .authored_draft_head(draft_id)
            .await
            .map_err(|_| Phase1DraftError::Storage)?
            .ok_or(Phase1DraftError::NotFound)?;
        if head.revision() != expected || head.stage() != AuthoredDraftStage::Queued {
            return Err(Phase1DraftError::RevisionConflict);
        }
        let request = push_request(&head)?;
        let operation_id = request.operation_id();
        let sync = self.sync()?;
        let mut status = sync
            .push_status(operation_id)
            .await
            .map_err(|_| Phase1DraftError::Operation)?
            .ok_or(Phase1DraftError::Corrupt)?;

        if matches!(
            status.artifact().signing_state(),
            SigningState::Planned | SigningState::Retryable
        ) {
            sync.sign_prepared(request)
                .await
                .map_err(|_| Phase1DraftError::Operation)?;
            status = sync
                .push_status(operation_id)
                .await
                .map_err(|_| Phase1DraftError::Operation)?
                .ok_or(Phase1DraftError::Corrupt)?;
        }
        if status.artifact().signing_state() == SigningState::Signed
            && matches!(
                status.artifact().admission_state(),
                AdmissionState::Pending | AdmissionState::Retryable
            )
        {
            sync.admit_signed(operation_id)
                .await
                .map_err(|_| Phase1DraftError::Operation)?;
            status = sync
                .push_status(operation_id)
                .await
                .map_err(|_| Phase1DraftError::Operation)?
                .ok_or(Phase1DraftError::Corrupt)?;
        }
        if status.artifact().admission_state().is_admitted()
            && matches!(
                status.delivery_plan().state(),
                AuthoredDeliveryState::Pending | AuthoredDeliveryState::Retryable
            )
        {
            sync.deliver_push(operation_id)
                .await
                .map_err(|_| Phase1DraftError::Operation)?;
        }
        self.draft_status_from(head).await
    }

    /// Signs one short-lived BUD-11 upload credential for HTTP use only.
    ///
    /// The returned value is not persisted and its distinct plan type cannot
    /// enter the relay push pipeline.
    #[allow(clippy::too_many_arguments)]
    pub async fn phase1_authorize_blossom_upload(
        &self,
        operation_id: [u8; 16],
        artifact_id: [u8; 16],
        claim: AuthoredUploadClaim,
        deadline_unix_ms: u64,
        cancellation: Phase1CancellationPolicy,
    ) -> Result<radroots_sdk::signing::AuthorizationHeader, Phase1DraftError> {
        let public_key = self
            .store_public_key
            .ok_or(Phase1DraftError::IdentityUnavailable)?;
        let actor = Actor::new(public_key, ActorSource::ExplicitPublicKey, AuthorRole::ALL)
            .map_err(|_| Phase1DraftError::IdentityUnavailable)?;
        let plan = radroots_sdk::signing::BlossomAuthorizationPlan::for_upload(&claim, public_key)
            .map_err(|_| Phase1DraftError::InvalidMedia)?;
        let operation_id =
            SigningOperationId::new(operation_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let artifact_id =
            AuthoredArtifactId::new(artifact_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let policy = SignPolicy::new(deadline_unix_ms, cancellation.signing())
            .map_err(|_| Phase1DraftError::InvalidDraft)?;
        let request = radroots_sdk::signing::blossom_upload_request(
            radroots_protocol::runtime::v1::OperationId::SyncPush,
            SigningIntentId::new(operation_id, artifact_id),
            actor,
            plan,
            policy,
        )
        .map_err(|_| Phase1DraftError::Operation)?;
        self.client
            .signing()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?
            .ok_or(Phase1DraftError::OperationUnavailable)?
            .authorize_blossom_upload(request)
            .await
            .map_err(|_| Phase1DraftError::Operation)
    }

    /// Runs the complete durable BUD-11/BUD-02/BUD-01 media transaction.
    ///
    /// Final image bytes are bound before authorization. The draft becomes
    /// verified only after the upload descriptor and a full retrieval agree.
    #[allow(clippy::too_many_arguments)]
    pub async fn phase1_upload_draft_media(
        &self,
        draft_id: [u8; 16],
        expected_revision: u64,
        request: radroots_sdk::transport::BlossomUploadRequest,
        authorization_content: radroots_blossom::authorization::AuthorizationContent,
        authorization_created_at_unix_s: u64,
        authorization_lifetime_seconds: u64,
        operation_id: [u8; 16],
        artifact_id: [u8; 16],
        signing_deadline_unix_ms: u64,
        signing_cancellation: Phase1CancellationPolicy,
        transfer_cancellation: radroots_sdk::transport::BlossomCancellation,
        updated_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let url = request.expected_url().as_str().to_owned();
        let uploading = self
            .phase1_update_draft_media(
                draft_id,
                expected_revision,
                url.as_str(),
                Phase1MediaStage::Uploading,
                None,
                updated_at_unix_ms,
            )
            .await?;
        let revision = uploading.draft().revision().get();
        let blossom = self
            .client
            .blossom()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?
            .ok_or(Phase1DraftError::OperationUnavailable)?;
        let claim = match blossom.authored_upload_claim(
            &request,
            authorization_content,
            authorization_created_at_unix_s,
            authorization_lifetime_seconds,
        ) {
            Ok(claim) => claim,
            Err(_) => {
                self.phase1_update_draft_media(
                    draft_id,
                    revision,
                    url.as_str(),
                    Phase1MediaStage::Failed,
                    Some("blossom_authorization_failed".to_owned()),
                    updated_at_unix_ms,
                )
                .await?;
                return Err(Phase1DraftError::Operation);
            }
        };
        let authorization = match self
            .phase1_authorize_blossom_upload(
                operation_id,
                artifact_id,
                claim,
                signing_deadline_unix_ms,
                signing_cancellation,
            )
            .await
        {
            Ok(authorization) => authorization,
            Err(error) => {
                self.phase1_update_draft_media(
                    draft_id,
                    revision,
                    url.as_str(),
                    Phase1MediaStage::Failed,
                    Some("blossom_authorization_failed".to_owned()),
                    updated_at_unix_ms,
                )
                .await?;
                return Err(error);
            }
        };
        match blossom
            .upload(request, authorization, transfer_cancellation)
            .await
        {
            Ok(receipt) => {
                self.phase1_complete_draft_media(
                    draft_id,
                    revision,
                    url.as_str(),
                    receipt,
                    updated_at_unix_ms,
                )
                .await
            }
            Err(error) => {
                self.phase1_fail_draft_media(
                    draft_id,
                    revision,
                    url.as_str(),
                    &error,
                    updated_at_unix_ms,
                )
                .await?;
                Err(Phase1DraftError::Operation)
            }
        }
    }

    /// Returns durable draft state composed with canonical authored-operation state.
    pub async fn phase1_draft_status(
        &self,
        draft_id: [u8; 16],
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let head = self
            .storage()?
            .authored_draft_head(draft_id)
            .await
            .map_err(|_| Phase1DraftError::Storage)?
            .ok_or(Phase1DraftError::NotFound)?;
        self.draft_status_from(head).await
    }

    /// Lists the newest immutable revision of each draft for the active author.
    pub async fn phase1_draft_heads(
        &self,
        limit: u16,
    ) -> Result<Vec<Phase1DraftStatus>, Phase1DraftError> {
        let drafts = self
            .storage()?
            .authored_draft_heads(self.draft_author()?, limit)
            .await
            .map_err(map_draft_storage_error)?;
        let mut statuses = Vec::with_capacity(drafts.len());
        for draft in drafts {
            statuses.push(self.draft_status_from(draft).await?);
        }
        Ok(statuses)
    }

    /// Cancels still-pending authored work and records uploaded-but-unreferenced
    /// media as possible orphans without deleting any evidence.
    pub async fn phase1_cancel_draft(
        &self,
        draft_id: [u8; 16],
        expected_revision: u64,
        cancelled_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let draft_id =
            AuthoredDraftId::new(draft_id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        let expected = AuthoredDraftRevision::new(expected_revision)
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let storage = self.storage()?;
        let head = storage
            .authored_draft_head(draft_id)
            .await
            .map_err(|_| Phase1DraftError::Storage)?
            .ok_or(Phase1DraftError::NotFound)?;
        if head.stage() == AuthoredDraftStage::Cancelled {
            return self.draft_status_from(head).await;
        }
        if head.revision() != expected {
            return Err(Phase1DraftError::RevisionConflict);
        }
        let mut payload = Phase1DraftPayload::decode(&head)?;
        let push = self.push_status_for(&head).await?;
        if let Some(status) = &push {
            self.sync()?
                .cancel_push(sync_id_for(&head)?)
                .await
                .map_err(|_| Phase1DraftError::Operation)?;
            if status.artifact().signed().is_none() {
                mark_possible_orphans(&mut payload.media, cancelled_at_unix_ms);
            }
        } else {
            mark_possible_orphans(&mut payload.media, cancelled_at_unix_ms);
        }
        let next = head
            .successor(
                payload.encode()?,
                AuthoredDraftStage::Cancelled,
                head.operation_id(),
                cancelled_at_unix_ms,
            )
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let receipt = storage
            .append_authored_draft(next, Some(expected))
            .await
            .map_err(map_draft_storage_error)?;
        self.draft_status_from(receipt.draft().clone()).await
    }

    /// Applies the current durable operation state to an already-projected
    /// active-author card. The overlay remains local and never changes event truth.
    pub async fn phase1_apply_draft_overlay(
        &self,
        context: &LocalNetwork,
        draft_id: [u8; 16],
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let status = self.phase1_draft_status(draft_id).await?;
        let operation_id = status
            .draft
            .operation_id()
            .map(|id| hex::encode(id.as_bytes()))
            .ok_or(Phase1DraftError::OperationUnavailable)?;
        self.phase1_set_local_author_overlay(
            context,
            status.card_id,
            Some(LocalAuthorOverlay {
                operation_id,
                state: status.state.label().to_owned(),
            }),
        )
        .await
        .map_err(map_overlay_error)?;
        Ok(status)
    }

    async fn finish_queue(
        &self,
        ready: AuthoredDraft,
        queued_at_unix_ms: u64,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        let request = push_request(&ready)?;
        self.sync()?
            .prepare_push(request)
            .await
            .map_err(|_| Phase1DraftError::Operation)?;
        let queued = ready
            .successor(
                ready.payload().to_vec(),
                AuthoredDraftStage::Queued,
                ready.operation_id(),
                queued_at_unix_ms.max(ready.updated_at_unix_ms()),
            )
            .map_err(|_| Phase1DraftError::RevisionConflict)?;
        let receipt = self
            .storage()?
            .append_authored_draft(queued, Some(ready.revision()))
            .await
            .map_err(map_draft_storage_error)?;
        self.draft_status_from(receipt.draft().clone()).await
    }

    async fn draft_status_from(
        &self,
        draft: AuthoredDraft,
    ) -> Result<Phase1DraftStatus, Phase1DraftError> {
        if draft.author() != &self.draft_author()? {
            return Err(Phase1DraftError::Corrupt);
        }
        let payload = Phase1DraftPayload::decode(&draft)?;
        let integrity = PlanWireV1::from_json(payload.plan_wire_json.as_slice())
            .map_err(|_| Phase1DraftError::Corrupt)?;
        let card_id = payload
            .target_card_id
            .map(Ok)
            .unwrap_or_else(|| card_id(payload.command_type, integrity.plan()))?;
        let push = self.push_status_for(&draft).await?;
        if draft.stage() == AuthoredDraftStage::Queued && push.is_none() {
            return Err(Phase1DraftError::Corrupt);
        }
        let state = aggregate_state(&draft, push.as_ref());
        Ok(Phase1DraftStatus {
            draft,
            kind: payload.kind,
            command_type: payload.command_type,
            form: payload.form,
            media: payload.media,
            state,
            card_id,
            push,
        })
    }

    async fn push_status_for(
        &self,
        draft: &AuthoredDraft,
    ) -> Result<Option<PushStatus>, Phase1DraftError> {
        let Some(_) = draft.operation_id() else {
            return Ok(None);
        };
        self.sync()?
            .push_status(sync_id_for(draft)?)
            .await
            .map_err(|_| Phase1DraftError::Operation)
    }

    fn storage(&self) -> Result<&dyn AuthoredDraftStore, Phase1DraftError> {
        self.client
            .storage()
            .map(|storage| storage as &dyn AuthoredDraftStore)
            .map_err(|_| Phase1DraftError::Storage)
    }

    fn sync(&self) -> Result<radroots_sdk::sync::Operations<'_>, Phase1DraftError> {
        self.client
            .sync()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?
            .ok_or(Phase1DraftError::OperationUnavailable)
    }

    fn draft_author(&self) -> Result<[u8; 32], Phase1DraftError> {
        self.store_public_key
            .map(|key| *key.as_bytes())
            .ok_or(Phase1DraftError::IdentityUnavailable)
    }
}

fn push_request(draft: &AuthoredDraft) -> Result<PushRequest, Phase1DraftError> {
    let payload = Phase1DraftPayload::decode(draft)?;
    let policy = payload.queue.ok_or(Phase1DraftError::InvalidQueuePolicy)?;
    let (targets, satisfaction, cancellation) = policy.materialize()?;
    let plan = PlanWireV1::from_json(payload.plan_wire_json.as_slice())
        .map_err(|_| Phase1DraftError::Corrupt)?
        .into_plan();
    let public_key = PublicKey::from_bytes(*draft.author())
        .map_err(|_| Phase1DraftError::IdentityUnavailable)?;
    let actor = Actor::new(public_key, ActorSource::ExplicitPublicKey, AuthorRole::ALL)
        .map_err(|_| Phase1DraftError::IdentityUnavailable)?;
    let sync_id = sync_id_for(draft)?;
    let idempotency = IdempotencyKey::parse(format!(
        "phase1-draft-{}",
        hex::encode(draft.draft_id().as_bytes())
    ))
    .map_err(|_| Phase1DraftError::InvalidDraft)?;
    PushRequest::new(
        sync_id,
        idempotency,
        actor,
        plan,
        targets,
        satisfaction,
        policy.delivery_deadline_unix_ms,
        cancellation,
    )
    .map_err(|_| Phase1DraftError::InvalidQueuePolicy)
}

fn operation_id(
    draft_id: AuthoredDraftId,
    ready_payload: &[u8],
) -> Result<SyncId, Phase1DraftError> {
    let mut hasher = Sha256::new();
    hasher.update(DRAFT_OPERATION_DOMAIN);
    hasher.update(draft_id.as_bytes());
    hasher.update(
        u64::try_from(ready_payload.len())
            .map_err(|_| Phase1DraftError::InvalidDraft)?
            .to_be_bytes(),
    );
    hasher.update(ready_payload);
    let digest: [u8; 32] = hasher.finalize().into();
    let mut value = [0_u8; 16];
    value.copy_from_slice(&digest[..16]);
    if value.iter().all(|byte| *byte == 0) {
        value[15] = 1;
    }
    SyncId::new(value).map_err(|_| Phase1DraftError::InvalidDraft)
}

fn sync_id_for(draft: &AuthoredDraft) -> Result<SyncId, Phase1DraftError> {
    draft
        .operation_id()
        .ok_or(Phase1DraftError::OperationUnavailable)
        .and_then(|id| SyncId::new(*id.as_bytes()).map_err(|_| Phase1DraftError::Corrupt))
}

fn media_urls(tags: &[Vec<String>]) -> Result<BTreeSet<&str>, Phase1DraftError> {
    let mut urls = BTreeSet::new();
    for tag in tags {
        let candidate = match tag.first().map(String::as_str) {
            Some("imeta") => tag
                .iter()
                .skip(1)
                .find_map(|value| value.strip_prefix("url ")),
            Some("image") => tag.get(1).map(String::as_str),
            _ => None,
        };
        if let Some(candidate) = candidate
            && BlobUrl::parse(candidate).is_ok()
            && !urls.insert(candidate)
        {
            return Err(Phase1DraftError::InvalidMedia);
        }
    }
    Ok(urls)
}

fn draft_stage_for_media(media: &[Phase1MediaPrerequisite]) -> AuthoredDraftStage {
    if media.is_empty() {
        AuthoredDraftStage::Draft
    } else if media
        .iter()
        .any(|media| media.stage == Phase1MediaStage::Uploading)
    {
        AuthoredDraftStage::MediaUploading
    } else {
        AuthoredDraftStage::MediaPreparing
    }
}

fn mark_possible_orphans(media: &mut [Phase1MediaPrerequisite], recorded_at_unix_ms: u64) {
    for media in media {
        if media.stage == Phase1MediaStage::Verified || media.orphan.is_some() {
            media.stage = Phase1MediaStage::Orphaned;
            media.failure_code = None;
            media.orphan = Some(Phase1MediaOrphanRecord {
                reason_code: "draft_cancelled_after_upload".to_owned(),
                recorded_at_unix_ms,
            });
        }
    }
}

const fn valid_media_transition(previous: Phase1MediaStage, next: Phase1MediaStage) -> bool {
    match previous {
        Phase1MediaStage::Pending => matches!(
            next,
            Phase1MediaStage::Pending
                | Phase1MediaStage::Preparing
                | Phase1MediaStage::Uploading
                | Phase1MediaStage::Failed
        ),
        Phase1MediaStage::Preparing => matches!(
            next,
            Phase1MediaStage::Preparing | Phase1MediaStage::Uploading | Phase1MediaStage::Failed
        ),
        Phase1MediaStage::Uploading => matches!(
            next,
            Phase1MediaStage::Uploading | Phase1MediaStage::Verified | Phase1MediaStage::Failed
        ),
        Phase1MediaStage::Failed => matches!(
            next,
            Phase1MediaStage::Preparing | Phase1MediaStage::Uploading | Phase1MediaStage::Failed
        ),
        Phase1MediaStage::Verified => matches!(next, Phase1MediaStage::Verified),
        Phase1MediaStage::Orphaned => matches!(next, Phase1MediaStage::Orphaned),
    }
}

fn aggregate_state(draft: &AuthoredDraft, push: Option<&PushStatus>) -> Phase1OutboxState {
    if draft.stage() == AuthoredDraftStage::Cancelled {
        return Phase1OutboxState::Cancelled;
    }
    let Some(push) = push else {
        return match draft.stage() {
            AuthoredDraftStage::Draft => Phase1OutboxState::Draft,
            AuthoredDraftStage::MediaPreparing => Phase1OutboxState::MediaPreparing,
            AuthoredDraftStage::MediaUploading => Phase1OutboxState::MediaUploading,
            AuthoredDraftStage::ReadyToSign => Phase1OutboxState::ReadyToSign,
            AuthoredDraftStage::Queued => Phase1OutboxState::Queued,
            AuthoredDraftStage::Cancelled => Phase1OutboxState::Cancelled,
        };
    };
    if push.settlement().is_successful() {
        return Phase1OutboxState::Complete;
    }
    if push.settlement().has_failures() {
        return if push.settlement().retryable() != 0 || push.settlement().delivery_retryable() != 0
        {
            Phase1OutboxState::Retryable
        } else if push.settlement().cancelled() != 0 || push.settlement().delivery_cancelled() != 0
        {
            Phase1OutboxState::Cancelled
        } else {
            Phase1OutboxState::Terminal
        };
    }
    match push.artifact().signing_state() {
        SigningState::Planned if push.artifact().signing_claim().is_some() => {
            Phase1OutboxState::Signing
        }
        SigningState::Planned => Phase1OutboxState::Queued,
        SigningState::Retryable => Phase1OutboxState::Retryable,
        SigningState::Indeterminate | SigningState::FailedTerminal => Phase1OutboxState::Terminal,
        SigningState::Cancelled => Phase1OutboxState::Cancelled,
        SigningState::Signed => match push.artifact().admission_state() {
            AdmissionState::Pending => Phase1OutboxState::Signed,
            AdmissionState::Retryable => Phase1OutboxState::Retryable,
            AdmissionState::Rejected => Phase1OutboxState::Terminal,
            AdmissionState::Cancelled => Phase1OutboxState::Cancelled,
            AdmissionState::Inserted | AdmissionState::Duplicate => match push
                .delivery_plan()
                .state()
            {
                AuthoredDeliveryState::Pending
                    if push.delivery_plan().claim_evidence().is_some() =>
                {
                    Phase1OutboxState::Delivering
                }
                AuthoredDeliveryState::Pending if push.delivery_plan().attempts().is_empty() => {
                    Phase1OutboxState::Queued
                }
                AuthoredDeliveryState::Pending => Phase1OutboxState::PartiallyDelivered,
                AuthoredDeliveryState::Retryable if has_delivery_success(push) => {
                    Phase1OutboxState::PartiallyDelivered
                }
                AuthoredDeliveryState::Retryable => Phase1OutboxState::Retryable,
                AuthoredDeliveryState::Satisfied => Phase1OutboxState::Complete,
                AuthoredDeliveryState::Exhausted | AuthoredDeliveryState::FailedTerminal => {
                    Phase1OutboxState::Terminal
                }
                AuthoredDeliveryState::Cancelled => Phase1OutboxState::Cancelled,
            },
        },
    }
}

fn has_delivery_success(push: &PushStatus) -> bool {
    push.delivery_plan().attempts().iter().any(|attempt| {
        let evidence = match attempt.outcome() {
            DeliveryAttemptOutcome::Receipt(receipt) => receipt.target_receipts(),
            DeliveryAttemptOutcome::SinkFailure(failure) => failure.partial_evidence(),
        };
        evidence.iter().any(|receipt| {
            matches!(
                receipt.outcome().kind(),
                DeliveryOutcomeKind::Accepted | DeliveryOutcomeKind::Delivered
            )
        })
    })
}

fn card_id(
    command_type: AddCommandType,
    plan: &radroots_event_codec::authoring::AuthoredEventPlan,
) -> Result<CardId, Phase1DraftError> {
    let card_type = match command_type {
        AddCommandType::CreateUpdate => TodayCardType::Update,
        AddCommandType::CreatePhotoUpdate => TodayCardType::PhotoUpdate,
        AddCommandType::CreateAsk => TodayCardType::Ask,
        AddCommandType::CreateEvent => TodayCardType::Event,
        AddCommandType::CreateFoodAvailability => TodayCardType::FoodAvailability,
    };
    let source = if (30_000..40_000).contains(&plan.body().kind()) {
        let identifier = plan
            .body()
            .tags()
            .iter()
            .find(|tag| tag.first().map(String::as_str) == Some("d") && tag.len() == 2)
            .and_then(|tag| tag.get(1))
            .ok_or(Phase1DraftError::Corrupt)?;
        CardSourceIdentity::address(
            plan.body().kind(),
            plan.author().to_hex(),
            identifier.clone(),
        )
        .map_err(|_| Phase1DraftError::Corrupt)?
    } else {
        CardSourceIdentity::Event(*plan.expected_event_id())
    };
    Ok(CardId::derive(card_type, &source))
}

fn map_draft_storage_error(error: radroots_storage::Error) -> Phase1DraftError {
    match error {
        radroots_storage::Error::DraftRevisionConflict => Phase1DraftError::RevisionConflict,
        radroots_storage::Error::DraftNotFound => Phase1DraftError::NotFound,
        radroots_storage::Error::CorruptAuthoredDraft => Phase1DraftError::Corrupt,
        _ => Phase1DraftError::Storage,
    }
}

fn map_overlay_error(_: TodayError) -> Phase1DraftError {
    Phase1DraftError::Overlay
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::product_surface::{
        CANONICAL_ADD_COMMAND_TYPES, CreateAsk, CreateEvent, CreateFoodAvailability,
        CreatePhotoUpdate, CreateUpdate,
    };
    use radroots_blossom::{BlobDescriptor, Sha256 as BlossomSha256};
    use radroots_event::{
        calendar::{AuthoredCalendarDateEvent, CalendarDate},
        food::availability::{
            FoodAvailabilityDetails, FoodAvailabilityDetailsParts, FoodAvailabilityStatus,
            FoodContent, FoodCurrency, FoodIdentifier, FoodPrice, FoodPublishedAt, FoodText,
            FoodUnit,
        },
        media::AuthoredImage,
        post::{AuthoredPostImage, PostImageDimensions},
    };
    use radroots_sdk::ClientBuilder;

    const AUTHOR: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
    const SECRET: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    fn runtime() -> RadrootsRuntime {
        RadrootsRuntime::from_client_builder(
            ClientBuilder::memory_default(),
            Some(PublicKey::from_hex(AUTHOR).unwrap()),
            None,
            None,
            None,
        )
        .unwrap()
    }

    fn signing_runtime() -> RadrootsRuntime {
        let signer = radroots_nostr::signing::LocalSigner::new(
            radroots_nostr::key::SecretKey::parse(SECRET).unwrap(),
        )
        .unwrap();
        RadrootsRuntime::from_client_builder(
            ClientBuilder::memory_default(),
            Some(PublicKey::from_hex(AUTHOR).unwrap()),
            Some(std::sync::Arc::new(signer)),
            None,
            None,
        )
        .unwrap()
    }

    fn policy() -> Phase1QueuePolicy {
        Phase1QueuePolicy::new(
            vec![
                "wss://relay-one.example".to_owned(),
                "wss://relay-two.example".to_owned(),
            ],
            Phase1RelaySatisfaction::AllAccepted,
            2_000_000_000_000,
            Phase1CancellationPolicy::LocalCooperative,
        )
        .unwrap()
    }

    fn update_form() -> Phase1DraftFormSnapshot {
        Phase1DraftFormSnapshot {
            command_type: AddCommandType::CreateUpdate,
            content: "Harvest update".to_owned(),
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
            food_status: None,
            media: Vec::new(),
        }
    }

    #[tokio::test]
    async fn form_snapshots_reopen_exactly_and_freeze_after_queue() {
        let runtime = runtime();
        let id = [6; 16];
        let saved = runtime
            .phase1_save_draft_with_form(
                id,
                Phase1AddCommand::CreateUpdate(CreateUpdate::new("Harvest update").unwrap()),
                1_900_000_000,
                Vec::new(),
                update_form(),
                None,
                10,
            )
            .await
            .unwrap();
        assert_eq!(saved.kind(), Phase1DraftKind::Add);
        assert_eq!(saved.form(), Some(&update_form()));
        assert_eq!(
            runtime.phase1_draft_status(id).await.unwrap().form(),
            Some(&update_form())
        );

        let queued = runtime
            .phase1_queue_draft(id, 1, policy(), 11)
            .await
            .unwrap();
        assert_eq!(queued.form(), Some(&update_form()));
        assert_eq!(
            runtime
                .phase1_save_draft_with_form(
                    id,
                    Phase1AddCommand::CreateUpdate(CreateUpdate::new("Changed").unwrap()),
                    1_900_000_001,
                    Vec::new(),
                    update_form(),
                    Some(queued.draft().revision().get()),
                    12,
                )
                .await
                .unwrap_err(),
            Phase1DraftError::RevisionConflict
        );
    }

    #[tokio::test]
    async fn retraction_is_independent_and_add_advance_attempts_delivery() {
        let runtime = signing_runtime();
        let target = CardId::parse(&"a".repeat(64)).unwrap();
        let retraction = runtime
            .phase1_save_retraction_draft(
                [5; 16],
                AddCommandType::CreateUpdate,
                target,
                &"b".repeat(64),
                1,
                None,
                "Replaced by a corrected copy",
                1_900_000_000,
                20,
            )
            .await
            .unwrap();
        assert_eq!(retraction.kind(), Phase1DraftKind::Retraction);
        assert_eq!(retraction.card_id(), target);
        assert!(retraction.form().is_none());
        let queued = runtime
            .phase1_queue_draft([5; 16], retraction.draft().revision().get(), policy(), 21)
            .await
            .unwrap();
        let signed_retraction = runtime
            .phase1_sign_queued_draft([5; 16], queued.draft().revision().get())
            .await;
        let signed_retraction = signed_retraction.unwrap();
        assert_eq!(signed_retraction.kind(), Phase1DraftKind::Retraction);
        let push = signed_retraction
            .push()
            .expect("durable retraction operation");
        assert_eq!(
            push.artifact()
                .signed()
                .expect("signed retraction")
                .event()
                .kind(),
            5
        );

        let saved = runtime
            .phase1_save_draft(
                [4; 16],
                Phase1AddCommand::CreateUpdate(CreateUpdate::new("Deliver me").unwrap()),
                1_900_000_001,
                Vec::new(),
                None,
                30,
            )
            .await
            .unwrap();
        let queued = runtime
            .phase1_queue_draft([4; 16], saved.draft().revision().get(), policy(), 31)
            .await
            .unwrap();
        let _ = runtime
            .phase1_advance_draft([4; 16], queued.draft().revision().get())
            .await;
        let advanced = runtime.phase1_draft_status([4; 16]).await.unwrap();
        let push = advanced.push().expect("durable add operation");
        assert!(push.artifact().admission_state().is_admitted());
        assert!(!push.delivery_plan().attempts().is_empty());
        assert!(matches!(
            advanced.state(),
            Phase1OutboxState::Retryable
                | Phase1OutboxState::PartiallyDelivered
                | Phase1OutboxState::Complete
                | Phase1OutboxState::Terminal
        ));
    }

    #[tokio::test]
    async fn media_free_draft_queues_offline_and_recovers_exactly() {
        let runtime = runtime();
        let id = [7; 16];
        let draft = runtime
            .phase1_save_draft(
                id,
                Phase1AddCommand::CreateUpdate(CreateUpdate::new("Harvest").unwrap()),
                1_900_000_000,
                Vec::new(),
                None,
                10,
            )
            .await
            .unwrap();
        assert_eq!(draft.state(), Phase1OutboxState::Draft);
        let queued = runtime
            .phase1_queue_draft(id, 1, policy(), 11)
            .await
            .unwrap();
        assert_eq!(queued.state(), Phase1OutboxState::Queued);
        assert_eq!(queued.draft().revision().get(), 3);
        assert!(queued.push().is_some());
        let recovered = runtime.phase1_recover_draft_queue(id, 12).await.unwrap();
        assert_eq!(recovered.draft(), queued.draft());
        assert_eq!(recovered.push(), queued.push());
    }

    #[tokio::test]
    async fn queue_recovery_closes_both_preparation_crash_windows() {
        let runtime = runtime();
        for (id_byte, prepare_before_recovery) in [(10, false), (11, true)] {
            let id = [id_byte; 16];
            let saved = runtime
                .phase1_save_draft(
                    id,
                    Phase1AddCommand::CreateUpdate(CreateUpdate::new("Recover").unwrap()),
                    1_900_000_010,
                    Vec::new(),
                    None,
                    40,
                )
                .await
                .unwrap();
            let mut payload = Phase1DraftPayload::decode(saved.draft()).unwrap();
            payload.queue = Some(policy());
            let bytes = payload.encode().unwrap();
            let draft_id = saved.draft().draft_id();
            let operation = operation_id(draft_id, bytes.as_slice()).unwrap();
            let operation = OperationInstanceId::new(*operation.as_bytes()).unwrap();
            let ready = saved
                .draft()
                .successor(bytes, AuthoredDraftStage::ReadyToSign, Some(operation), 41)
                .unwrap();
            runtime
                .storage()
                .unwrap()
                .append_authored_draft(ready.clone(), Some(saved.draft().revision()))
                .await
                .unwrap();
            if prepare_before_recovery {
                runtime
                    .sync()
                    .unwrap()
                    .prepare_push(push_request(&ready).unwrap())
                    .await
                    .unwrap();
            }
            let recovered = runtime.phase1_recover_draft_queue(id, 42).await.unwrap();
            assert_eq!(recovered.draft().stage(), AuthoredDraftStage::Queued);
            assert_eq!(recovered.draft().revision().get(), 3);
            assert!(recovered.push().is_some());
        }
    }

    #[tokio::test]
    async fn all_five_add_flows_queue_and_sign_without_network_access() {
        let runtime = signing_runtime();
        let (photo, media) = photo_command();
        let commands = [
            (
                Phase1AddCommand::CreateUpdate(CreateUpdate::new("Harvest update").unwrap()),
                Vec::new(),
            ),
            (photo, vec![media]),
            (
                Phase1AddCommand::CreateAsk(CreateAsk::new("Who has basil?", Vec::new()).unwrap()),
                Vec::new(),
            ),
            (
                Phase1AddCommand::CreateEvent(CreateEvent::date(
                    AuthoredCalendarDateEvent::new(
                        "market-day",
                        "Saturday Market",
                        CalendarDate::parse("2026-08-08").unwrap(),
                    )
                    .unwrap(),
                )),
                Vec::new(),
            ),
            (
                Phase1AddCommand::CreateFoodAvailability(CreateFoodAvailability::new(food())),
                Vec::new(),
            ),
        ];
        for (index, (command, media)) in commands.into_iter().enumerate() {
            let mut id = [30; 16];
            id[15] = u8::try_from(index + 1).unwrap();
            let saved = runtime
                .phase1_save_draft(
                    id,
                    command,
                    1_784_347_200,
                    media,
                    None,
                    100 + u64::try_from(index).unwrap() * 10,
                )
                .await
                .unwrap();
            let queued = runtime
                .phase1_queue_draft(
                    id,
                    saved.draft().revision().get(),
                    policy(),
                    101 + u64::try_from(index).unwrap() * 10,
                )
                .await
                .unwrap();
            assert_eq!(queued.state(), Phase1OutboxState::Queued);
            assert_eq!(queued.command_type(), CANONICAL_ADD_COMMAND_TYPES[index]);
            let signed = runtime
                .phase1_sign_queued_draft(id, queued.draft().revision().get())
                .await
                .unwrap();
            assert_eq!(signed.state(), Phase1OutboxState::Signed);
            assert_eq!(
                signed
                    .push()
                    .and_then(|push| push.artifact().signed())
                    .expect("signed artifact")
                    .event()
                    .kind(),
                match index {
                    0..=2 => 1,
                    3 => 31_922,
                    4 => 30_402,
                    _ => unreachable!(),
                }
            );
        }
    }

    #[tokio::test]
    async fn blossom_authorization_uses_the_same_opaque_signer_but_never_the_outbox() {
        use radroots_blossom::authorization::{
            AuthorizationContent, AuthorizationTarget, AuthorizationValidation, ServerDomain,
        };

        let runtime = signing_runtime();
        let hash = BlossomSha256::digest(b"exact upload bytes");
        let server = ServerDomain::parse("media.example").unwrap();
        let claim = AuthoredUploadClaim::new(
            AuthorizationContent::parse("Upload exact Radroots image").unwrap(),
            server.clone(),
            hash,
            1_900_000_000,
            60,
        )
        .unwrap();
        let header = runtime
            .phase1_authorize_blossom_upload(
                [71; 16],
                [72; 16],
                claim,
                u64::MAX,
                Phase1CancellationPolicy::LocalCooperative,
            )
            .await
            .unwrap();
        let verified = radroots_nostr::blossom::decode_verify_authorization_header(
            header.as_str(),
            &AuthorizationValidation::bud11(
                AuthorizationTarget::Upload(hash),
                server,
                1_900_000_001,
            ),
        )
        .unwrap();
        assert_eq!(verified.claim().hashes(), &[hash]);
        assert!(runtime.phase1_draft_heads(10).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn cancellation_is_terminal_and_preserves_operation_evidence() {
        let runtime = runtime();
        let id = [8; 16];
        runtime
            .phase1_save_draft(
                id,
                Phase1AddCommand::CreateUpdate(CreateUpdate::new("Cancelled").unwrap()),
                1_900_000_001,
                Vec::new(),
                None,
                20,
            )
            .await
            .unwrap();
        let queued = runtime
            .phase1_queue_draft(id, 1, policy(), 21)
            .await
            .unwrap();
        let cancelled = runtime
            .phase1_cancel_draft(id, queued.draft().revision().get(), 22)
            .await
            .unwrap();
        assert_eq!(cancelled.state(), Phase1OutboxState::Cancelled);
        let push = cancelled.push().expect("retained push evidence");
        assert_eq!(push.artifact().signing_state(), SigningState::Cancelled);
        assert_eq!(
            push.delivery_plan().state(),
            AuthoredDeliveryState::Cancelled
        );
        assert!(push.settlement().is_settled());
    }

    #[tokio::test]
    async fn media_phase_revisions_gate_queue_and_reject_forged_verification() {
        let runtime = runtime();
        let id = [9; 16];
        let (command, mut media) = photo_command();
        media.stage = Phase1MediaStage::Pending;
        media.upload_attempts = 0;
        media.verified_at_unix_ms = None;
        media.validate().unwrap();
        let saved = runtime
            .phase1_save_draft(id, command, 1_784_347_200, vec![media], None, 30)
            .await
            .unwrap();
        assert_eq!(saved.state(), Phase1OutboxState::MediaPreparing);
        assert_eq!(
            runtime
                .phase1_queue_draft(id, 1, policy(), 31)
                .await
                .unwrap_err(),
            Phase1DraftError::MediaNotReady
        );
        let preparing = runtime
            .phase1_update_draft_media(
                id,
                1,
                saved.media()[0].url(),
                Phase1MediaStage::Preparing,
                None,
                31,
            )
            .await
            .unwrap();
        let uploading = runtime
            .phase1_update_draft_media(
                id,
                2,
                preparing.media()[0].url(),
                Phase1MediaStage::Uploading,
                None,
                32,
            )
            .await
            .unwrap();
        assert_eq!(
            runtime
                .phase1_update_draft_media(
                    id,
                    3,
                    uploading.media()[0].url(),
                    Phase1MediaStage::Verified,
                    None,
                    33,
                )
                .await
                .unwrap_err(),
            Phase1DraftError::InvalidMedia
        );
        assert_eq!(
            runtime
                .phase1_queue_draft(id, 3, policy(), 34)
                .await
                .unwrap_err(),
            Phase1DraftError::MediaNotReady
        );
        assert_eq!(runtime.phase1_draft_heads(10).await.unwrap().len(), 1);
    }

    #[test]
    fn queue_policy_rejects_duplicates_and_noncanonical_relays() {
        assert!(
            Phase1QueuePolicy::new(
                vec!["wss://relay.example".into(), "wss://relay.example".into()],
                Phase1RelaySatisfaction::AnyAccepted,
                1,
                Phase1CancellationPolicy::LocalCooperative,
            )
            .is_err()
        );
        assert!(
            Phase1QueuePolicy::new(
                vec!["WSS://relay.example".into()],
                Phase1RelaySatisfaction::AnyAccepted,
                1,
                Phase1CancellationPolicy::LocalCooperative,
            )
            .is_err()
        );
    }

    fn photo_command() -> (Phase1AddCommand, Phase1MediaPrerequisite) {
        let bytes = b"harvest-photo";
        let hash = BlossomSha256::digest(bytes);
        let url = format!("https://media.example/{hash}.webp");
        let media_type = MediaType::parse("image/webp").unwrap();
        let descriptor = BlobDescriptor::new(
            BlobUrl::parse(url.as_str()).unwrap(),
            hash,
            bytes.len() as u64,
            media_type.clone(),
            1_784_347_100,
        )
        .unwrap()
        .approve_reference()
        .unwrap()
        .verify_bytes(bytes, &media_type)
        .unwrap();
        let mut prerequisite =
            Phase1MediaPrerequisite::new("protected://draft/photo-1", &descriptor).unwrap();
        let image = AuthoredPostImage::new(
            AuthoredImage::try_from(descriptor).unwrap(),
            PostImageDimensions::new(1200, 900).unwrap(),
            "Harvest",
        )
        .unwrap();
        let command = Phase1AddCommand::CreatePhotoUpdate(
            CreatePhotoUpdate::new(format!("Harvest photo {url}"), vec![image]).unwrap(),
        );
        prerequisite.stage = Phase1MediaStage::Verified;
        prerequisite.upload_attempts = 2;
        prerequisite.verified_at_unix_ms = Some(1_784_347_100_000);
        prerequisite.validate().unwrap();
        (command, prerequisite)
    }

    fn food() -> FoodAvailabilityDetails {
        FoodAvailabilityDetails::new(FoodAvailabilityDetailsParts {
            content: FoodContent::new("Carrots available this week.").unwrap(),
            identifier: FoodIdentifier::parse("nantes-carrots").unwrap(),
            title: FoodText::new("Nantes Carrots").unwrap(),
            summary: FoodText::new("Fresh bunches").unwrap(),
            published_at: FoodPublishedAt::new(1_784_347_100).unwrap(),
            location: FoodText::new("Central Saanich, BC").unwrap(),
            price: FoodPrice::new("3", FoodCurrency::parse("CAD").unwrap(), FoodUnit::Pound)
                .unwrap(),
            quantity: None,
            status: FoodAvailabilityStatus::Active,
            images: Vec::new(),
        })
        .unwrap()
    }
}
