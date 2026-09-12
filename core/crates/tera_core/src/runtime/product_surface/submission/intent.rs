//! App-owned immutable capture, carried by the existing shared authored transaction.

use radroots_event::contract::AuthorRole;
use radroots_event_codec::authoring::PlanWireV1;
use radroots_signing::{Actor, actor::ActorSource};
use radroots_storage::{
    atomic::AtomicCommitId,
    authored_atomic::PrepareAuthoredOperation,
    authored_draft::{
        AUTHORED_DRAFT_PAYLOAD_MAX_BYTES, AuthoredDraft, AuthoredDraftId, AuthoredDraftRevision,
        AuthoredDraftStage,
    },
    authored_draft_submission::PrepareFromDraft,
    journal::{IdempotencyKey, OperationInstanceId},
};
use radroots_sync::{PushRequest, policy::SyncId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    CapturedSubmission, SubmissionCommandId, SubmissionCommitError as E,
    SubmissionReservationReceipt, SubmissionReservationRequest,
};
use crate::runtime::product_surface::{
    AddCommandType, ComposerScope, Phase1MediaPrerequisite, Phase1MediaStage, Phase1QueuePolicy,
};

pub const SUBMISSION_INTENT_PAYLOAD_SCHEMA: &str = "tera.publication_intent.v1";
pub const SUBMISSION_INTENT_SCHEMA_VERSION: u64 = 1;
pub const SUBMISSION_INTENT_SCHEMA_SHA256: &str =
    "9e9b21b186c816c7996e7a94fba802d58563dfaefa1ddc1f88a486daa05b25a6";
pub const SUBMISSION_INTENT_MAX_BYTES: usize = AUTHORED_DRAFT_PAYLOAD_MAX_BYTES;

#[cfg(test)]
#[path = "intent_tests.rs"]
mod tests;

#[derive(Deserialize)]
struct Header {
    schema_version: u64,
    schema_sha256: String,
}

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct IntentPayload {
    schema_version: u64,
    schema_sha256: String,
    command_id: SubmissionCommandId,
    scope: ComposerScope,
    reservation_id: AuthoredDraftId,
    command_type: AddCommandType,
    plan_wire_json: Vec<u8>,
    media: Vec<Phase1MediaPrerequisite>,
    media_policy: Option<[u8; 32]>,
    policy: Phase1QueuePolicy,
}

pub(super) fn intent_id(request: &SubmissionReservationRequest) -> Result<AuthoredDraftId, E> {
    AuthoredDraftId::new(derive_id(b"tera.publication_intent.v1\0", request))
        .map_err(|_| E::InvalidIntent)
}

pub(super) fn operation_id(
    request: &SubmissionReservationRequest,
) -> Result<OperationInstanceId, E> {
    OperationInstanceId::new(derive_id(b"tera.submission_operation.v1\0", request))
        .map_err(|_| E::InvalidIntent)
}

fn derive_id(domain: &[u8], request: &SubmissionReservationRequest) -> [u8; 16] {
    let mut hash = Sha256::new();
    hash.update(domain);
    hash.update(request.scope().author().as_bytes());
    hash.update(request.command_id().as_bytes());
    let mut id = [0; 16];
    id.copy_from_slice(&hash.finalize()[..16]);
    id
}

pub(super) fn commit_id(request: &SubmissionReservationRequest) -> AtomicCommitId {
    PrepareFromDraft::commit_id_for(
        request.scope().author().as_bytes(),
        AtomicCommitId::new(*request.command_id().as_bytes()).expect("validated nonzero command"),
    )
}

impl IntentPayload {
    pub(super) fn capture(value: &CapturedSubmission) -> Result<PrepareFromDraft, E> {
        let reservation = value.reservation();
        let payload = Self {
            schema_version: SUBMISSION_INTENT_SCHEMA_VERSION,
            schema_sha256: SUBMISSION_INTENT_SCHEMA_SHA256.to_owned(),
            command_id: reservation.request().command_id(),
            scope: reservation.request().scope().clone(),
            reservation_id: reservation.reservation_id(),
            command_type: value.command().command_type(),
            plan_wire_json: PlanWireV1::from_plan(value.plan())
                .to_json()
                .map_err(|_| E::InvalidIntent)?,
            media: value.media().to_vec(),
            media_policy: value.media_policy().map(|value| *value.as_bytes()),
            policy: value.policy().clone(),
        };
        let preparation = payload.preparation(reservation)?;
        let bytes = serde_json::to_vec(&payload).map_err(|_| E::InvalidIntent)?;
        if bytes.len() > SUBMISSION_INTENT_MAX_BYTES {
            return Err(E::InvalidIntent);
        }
        let ready = payload.media.is_empty();
        let digest = Sha256::digest(&bytes).into();
        // This initial intent is installed by PrepareFromDraft, not the ordinary
        // draft append API (whose initial constructor deliberately rejects ready).
        let intent = AuthoredDraft::reconstruct(
            intent_id(reservation.request())?,
            AuthoredDraftRevision::INITIAL,
            reservation.request().scope().author().into_bytes(),
            SUBMISSION_INTENT_PAYLOAD_SCHEMA,
            bytes,
            digest,
            if ready {
                AuthoredDraftStage::ReadyToSign
            } else {
                AuthoredDraftStage::MediaPreparing
            },
            ready.then_some(preparation.operation().operation_id()),
            reservation.reserved_at_unix_ms(),
            reservation.reserved_at_unix_ms(),
        )
        .and_then(|intent| {
            intent.with_scope(
                reservation
                    .source
                    .scope()
                    .ok_or(radroots_storage::Error::InvalidAuthoredDraft)?,
            )
        })
        .map_err(|_| E::InvalidIntent)?;
        let constructor = if ready {
            PrepareFromDraft::new
        } else {
            PrepareFromDraft::new_waiting
        };
        constructor(
            AtomicCommitId::new(*payload.command_id.as_bytes()).map_err(|_| E::InvalidIntent)?,
            reservation.source.clone(),
            intent,
            preparation,
        )
        .map_err(|_| E::InvalidIntent)
    }

    pub(super) fn validate_committed(
        request: &PrepareFromDraft,
        reservation: &SubmissionReservationReceipt,
    ) -> Result<(), E> {
        request.validate().map_err(|_| E::InvalidReceipt)?;
        let intent = request.intent();
        if intent.payload_schema() != SUBMISSION_INTENT_PAYLOAD_SCHEMA {
            return Err(E::UnsupportedSchema);
        }
        if intent.payload().len() > SUBMISSION_INTENT_MAX_BYTES
            || intent.draft_id() != intent_id(reservation.request())?
            || intent.revision() != AuthoredDraftRevision::INITIAL
            || request.source() != &reservation.source
            || request.command_id().as_bytes() != reservation.request().command_id().as_bytes()
        {
            return Err(E::InvalidReceipt);
        }
        let header: Header =
            serde_json::from_slice(intent.payload()).map_err(|_| E::CorruptRecord)?;
        if header.schema_version != SUBMISSION_INTENT_SCHEMA_VERSION
            || header.schema_sha256 != SUBMISSION_INTENT_SCHEMA_SHA256
        {
            return Err(E::UnsupportedSchema);
        }
        let payload: Self =
            serde_json::from_slice(intent.payload()).map_err(|_| E::CorruptRecord)?;
        let preparation = payload.preparation(reservation)?;
        let stage = if payload.media.is_empty() {
            AuthoredDraftStage::ReadyToSign
        } else {
            AuthoredDraftStage::MediaPreparing
        };
        if intent.stage() != stage || &preparation != request.preparation() {
            return Err(E::InvalidReceipt);
        }
        Ok(())
    }

    fn preparation(
        &self,
        reservation: &SubmissionReservationReceipt,
    ) -> Result<PrepareAuthoredOperation, E> {
        let request = reservation.request();
        let input = reservation.captured().form().input();
        if self.command_id != request.command_id()
            || &self.scope != request.scope()
            || self.reservation_id != reservation.reservation_id()
            || self.command_type != input.command_type
            || self.media.len() != input.media.len()
            || self.media.len() > 20
            || self.media.is_empty() != self.media_policy.is_none()
        {
            return Err(E::CorruptRecord);
        }
        for (media, input) in self.media.iter().zip(&input.media) {
            media.validate().map_err(|_| E::CorruptRecord)?;
            if media.stage() != Phase1MediaStage::Pending
                || media.local_reference() != input.opaque_reference
                || media.sha256() != input.sha256
                || media.media_type() != input.media_type
                || media.byte_size() != input.byte_size
            {
                return Err(E::CorruptRecord);
            }
        }
        let plan = PlanWireV1::from_json(&self.plan_wire_json)
            .map_err(|_| E::CorruptRecord)?
            .into_plan();
        if plan.author() != &request.scope().author()
            || plan.created_at() != reservation.reserved_at_unix_ms() / 1000
        {
            return Err(E::CorruptRecord);
        }
        let (targets, satisfaction, cancellation) =
            self.policy.materialize().map_err(|_| E::CorruptRecord)?;
        let actor = Actor::new(
            request.scope().author(),
            ActorSource::ExplicitPublicKey,
            AuthorRole::ALL,
        )
        .map_err(|_| E::InvalidIntent)?;
        PushRequest::new(
            SyncId::new(*operation_id(request)?.as_bytes()).map_err(|_| E::InvalidIntent)?,
            IdempotencyKey::parse(format!(
                "tera.submission.v1.{}",
                hex::encode(request.command_id().as_bytes())
            ))
            .map_err(|_| E::InvalidIntent)?,
            actor,
            plan,
            targets,
            satisfaction,
            self.policy.delivery_deadline_unix_ms,
            cancellation,
        )
        .and_then(|request| request.authored_preparation(reservation.reserved_at_unix_ms()))
        .map_err(|_| E::InvalidIntent)
    }
}
