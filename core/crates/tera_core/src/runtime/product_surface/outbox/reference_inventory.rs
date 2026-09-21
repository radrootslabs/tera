use super::super::media_gc::MediaInventoryIncomplete as E;
use super::*;

pub(in crate::runtime::product_surface) fn media_references(
    draft: &AuthoredDraft,
) -> Result<Vec<String>, E> {
    if draft.scope().is_some() {
        return Err(E);
    }
    match draft.payload_schema() {
        super::super::coordinate::CLAIM_SCHEMA | super::super::coordinate::BINDING_SCHEMA => {
            if super::super::coordinate::metadata_is_valid(draft) {
                Ok(Vec::new())
            } else {
                Err(E)
            }
        }
        DRAFT_PAYLOAD_SCHEMA => {
            let payload = Phase1DraftPayload::decode(draft).map_err(|_| E)?;
            if payload.encode().map_err(|_| E)? != draft.payload() {
                return Err(E);
            }
            // The owning codec validates form-to-prerequisite equality. Cancellation,
            // remote verification and orphan evidence never erase a local reference.
            Ok(payload
                .media
                .iter()
                .map(|media| media.sha256().to_owned())
                .collect())
        }
        PROFILE_PAYLOAD_SCHEMA => {
            let payload = Phase1ProfilePayload::decode(draft).map_err(|_| E)?;
            if payload.encode().map_err(|_| E)? != draft.payload() {
                return Err(E);
            }
            Ok(Vec::new())
        }
        _ => Err(E),
    }
}
