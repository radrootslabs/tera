use super::super::media_gc::MediaInventoryIncomplete as E;
use super::*;

fn restore_request(draft: &AuthoredDraft) -> Result<Option<PushRequest>, Phase1DraftError> {
    media_references(draft).map_err(|_| Phase1DraftError::Corrupt)?;
    if draft.stage() == AuthoredDraftStage::Cancelled {
        return Ok(None);
    }
    match draft.stage() {
        AuthoredDraftStage::ReadyToSign | AuthoredDraftStage::Queued => {
            let request = if draft.payload_schema() == PROFILE_PAYLOAD_SCHEMA {
                profile_push_request(draft)?
            } else {
                push_request(draft)?
            };
            Ok(Some(request))
        }
        _ => {
            if draft.operation_id().is_some() {
                return Err(Phase1DraftError::Corrupt);
            }
            Ok(None)
        }
    }
}

impl TeraRuntime {
    pub(in crate::runtime) async fn restore_legacy_request(
        &self,
        head: &AuthoredDraft,
    ) -> Result<Option<PushRequest>, Phase1DraftError> {
        let request = restore_request(head)?;
        if request.is_none() {
            return Ok(None);
        }
        match self.push_status_for(head).await? {
            Some(push)
                if push.delivery_plan().state().is_terminal()
                    || push.delivery_plan().stop_requested_at_unix_ms().is_some() =>
            {
                Ok(None)
            }
            None if head.stage() == AuthoredDraftStage::Queued => Err(Phase1DraftError::Corrupt),
            _ => Ok(request),
        }
    }
}

pub(in crate::runtime::product_surface) fn media_references(
    draft: &AuthoredDraft,
) -> Result<Vec<super::super::media_gc::MediaReference>, E> {
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
                .map(|media| super::super::media_gc::MediaReference {
                    sha256: media.sha256().to_owned(),
                    byte_length: media.byte_size(),
                })
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
