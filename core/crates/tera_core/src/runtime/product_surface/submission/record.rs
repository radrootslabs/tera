//! Immutable metadata carried by the existing authored draft owner.

use radroots_storage::{
    authored_draft::{AuthoredDraft, AuthoredDraftId, AuthoredDraftRevision, AuthoredDraftStage},
    authored_draft_submission::AuthoredDraftSource,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{SubmissionCommandId, SubmissionReservationError as E, SubmissionReservationRequest};
use crate::runtime::product_surface::{
    ComposerPersistenceError, ComposerScope, ComposerStorageRecord,
};

pub const SUBMISSION_RESERVATION_PAYLOAD_SCHEMA: &str = "tera.submission_reservation.v1";
pub const SUBMISSION_RESERVATION_SCHEMA_VERSION: u64 = 1;
pub const SUBMISSION_RESERVATION_SCHEMA_SHA256: &str =
    "83c4caab57a30b7f28d78e558a369ca94254d4d5481e8e27d28d598350858d1f";
pub const SUBMISSION_RESERVATION_MAX_BYTES: usize = 16 * 1024;

#[cfg(test)]
#[path = "record_tests.rs"]
mod tests;

#[derive(Deserialize)]
struct SchemaHeader {
    schema_version: u64,
    schema_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ReservationWire {
    schema_version: u64,
    schema_sha256: String,
    command_id: SubmissionCommandId,
    scope: ComposerScope,
    pub(super) source: AuthoredDraftSource,
}

pub(super) fn reservation_id(request: &SubmissionReservationRequest) -> Result<AuthoredDraftId, E> {
    let mut digest = Sha256::new();
    digest.update(b"tera.submission_reservation.v1\0");
    digest.update(request.scope.author().as_bytes());
    digest.update(request.command_id.as_bytes());
    let mut id = [0; 16];
    id.copy_from_slice(&digest.finalize()[..16]);
    AuthoredDraftId::new(id).map_err(|_| E::InvalidCommandId)
}

pub(super) fn initial(
    request: &SubmissionReservationRequest,
    source: &ComposerStorageRecord,
    observed_time: Option<u64>,
) -> Result<AuthoredDraft, E> {
    let time = observed_time
        .filter(|time| *time > 0 && *time <= i64::MAX as u64)
        .filter(|time| *time >= source.stored().updated_at_unix_ms())
        .ok_or(E::ClockUnavailable)?;
    let wire = ReservationWire {
        schema_version: SUBMISSION_RESERVATION_SCHEMA_VERSION,
        schema_sha256: SUBMISSION_RESERVATION_SCHEMA_SHA256.to_owned(),
        command_id: request.command_id,
        scope: request.scope.clone(),
        source: AuthoredDraftSource::capture(source.stored()).map_err(|_| E::CorruptRecord)?,
    };
    let payload = serde_json::to_vec(&wire).map_err(|_| E::CorruptRecord)?;
    if payload.len() > SUBMISSION_RESERVATION_MAX_BYTES {
        return Err(E::CorruptRecord);
    }
    AuthoredDraft::initial(
        reservation_id(request)?,
        request.scope.author().into_bytes(),
        SUBMISSION_RESERVATION_PAYLOAD_SCHEMA,
        payload,
        AuthoredDraftStage::Draft,
        None,
        time,
    )
    .and_then(|draft| {
        draft.with_scope(
            source
                .stored()
                .scope()
                .ok_or(radroots_storage::Error::CorruptAuthoredDraft)?,
        )
    })
    .map_err(|_| E::CorruptRecord)
}

pub(super) fn decode(
    stored: &AuthoredDraft,
    request: &SubmissionReservationRequest,
) -> Result<ReservationWire, E> {
    stored.validate().map_err(|_| E::CorruptRecord)?;
    if stored.draft_id() != reservation_id(request)?
        || stored.author() != request.scope.author().as_bytes()
        || stored.payload_schema() != SUBMISSION_RESERVATION_PAYLOAD_SCHEMA
        || stored.revision() != AuthoredDraftRevision::INITIAL
        || stored.stage() != AuthoredDraftStage::Draft
        || stored.operation_id().is_some()
        || stored.payload().len() > SUBMISSION_RESERVATION_MAX_BYTES
        || stored.created_at_unix_ms() == 0
        || stored.created_at_unix_ms() > i64::MAX as u64
        || stored.created_at_unix_ms() != stored.updated_at_unix_ms()
    {
        return Err(E::CorruptRecord);
    }
    let header: SchemaHeader =
        serde_json::from_slice(stored.payload()).map_err(|_| E::CorruptRecord)?;
    if header.schema_version != SUBMISSION_RESERVATION_SCHEMA_VERSION
        || header.schema_sha256 != SUBMISSION_RESERVATION_SCHEMA_SHA256
    {
        return Err(E::UnsupportedSchema);
    }
    let wire: ReservationWire =
        serde_json::from_slice(stored.payload()).map_err(|_| E::CorruptRecord)?;
    let scope =
        ComposerStorageRecord::scope_digest(&wire.scope).map_err(ComposerPersistenceError::from)?;
    if stored.scope() != Some(scope)
        || wire.source.scope() != Some(scope)
        || wire.source.author() != wire.scope.author().as_bytes()
        || wire.source.payload_schema() != super::super::COMPOSER_PAYLOAD_SCHEMA
    {
        return Err(E::CorruptRecord);
    }
    if wire.command_id != request.command_id
        || wire.scope != request.scope
        || wire.source.draft_id().as_bytes() != request.composer_id.as_bytes()
        || wire.source.revision().get() != request.expected_revision.get()
    {
        return Err(E::IdempotencyConflict);
    }
    Ok(wire)
}
