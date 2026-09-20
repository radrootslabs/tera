use super::*;

#[cfg(test)]
#[path = "intent_inventory_tests.rs"]
mod tests;

fn decode(stored: &AuthoredDraft) -> Result<IntentPayload, E> {
    stored.validate().map_err(|_| E::CorruptRecord)?;
    if stored.payload_schema() != SUBMISSION_INTENT_PAYLOAD_SCHEMA
        || stored.payload().len() > SUBMISSION_INTENT_MAX_BYTES
    {
        return Err(E::CorruptRecord);
    }
    let payload: IntentPayload =
        serde_json::from_slice(stored.payload()).map_err(|_| E::CorruptRecord)?;
    if payload.schema_version != SUBMISSION_INTENT_SCHEMA_VERSION
        || payload.schema_sha256 != SUBMISSION_INTENT_SCHEMA_SHA256
        || serde_json::to_vec(&payload).map_err(|_| E::CorruptRecord)? != stored.payload()
    {
        return Err(E::CorruptRecord);
    }
    Ok(payload)
}

pub(in crate::runtime::product_surface::submission) fn inventory_reservation(
    stored: &AuthoredDraft,
    author: [u8; 32],
) -> Result<AuthoredDraftId, E> {
    let payload = decode(stored)?;
    if stored.author() != &author || payload.scope.author().as_bytes() != &author {
        return Err(E::CorruptRecord);
    }
    Ok(payload.reservation_id)
}

pub(in crate::runtime::product_surface::submission) fn validate_inventory(
    stored: &AuthoredDraft,
    reservation: &SubmissionReservationReceipt,
) -> Result<(), E> {
    let payload = decode(stored)?;
    payload.validated_request(reservation, false)?;
    let stage = payload.stage();
    let request = reservation.request();
    let scope =
        crate::runtime::product_surface::ComposerStorageRecord::scope_digest(request.scope())
            .map_err(|_| E::CorruptRecord)?;
    if stored.draft_id() != intent_id(request)?
        || stored.author() != request.scope().author().as_bytes()
        || stored.scope() != Some(scope)
        || stored.created_at_unix_ms() != reservation.reserved_at_unix_ms()
        || stored.updated_at_unix_ms() > i64::MAX as u64
        || payload
            .media
            .iter()
            .any(|media| media.stage() == Phase1MediaStage::Orphaned)
        || !(stored.stage() == stage
            || (stage == AuthoredDraftStage::ReadyToSign
                && stored.stage() == AuthoredDraftStage::Queued))
        || stored.operation_id()
            != (stage == AuthoredDraftStage::ReadyToSign)
                .then(|| operation_id(request))
                .transpose()?
    {
        return Err(E::CorruptRecord);
    }
    Ok(())
}
