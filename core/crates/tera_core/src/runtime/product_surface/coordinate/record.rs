//! Strict metadata codec; neither record is signable or contains media.

use radroots_storage::authored_draft::{AuthoredDraftRevision, AuthoredDraftStage};

use super::*;

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct OwnershipRecord {
    pub intent: CoordinateIntent,
    pub captured_revision: u64,
    pub captured_sha256: [u8; 32],
    pub claim_revision: u64,
}

impl OwnershipRecord {
    pub(super) fn encode(&self) -> Result<Vec<u8>, E> {
        self.intent.validate()?;
        AuthoredDraftRevision::new(self.captured_revision).map_err(|_| E::Corrupt)?;
        AuthoredDraftRevision::new(self.claim_revision).map_err(|_| E::Corrupt)?;
        serde_json::to_vec(self).map_err(|_| E::Corrupt)
    }

    pub(super) fn decode(draft: &AuthoredDraft) -> Result<Self, E> {
        draft.validate().map_err(|_| E::Corrupt)?;
        let value: Self = serde_json::from_slice(draft.payload()).map_err(|_| E::Corrupt)?;
        if value.encode()? != draft.payload()
            || draft.author() != &value.intent.author
            || draft.scope().is_some()
            || draft.stage() != AuthoredDraftStage::Draft
            || draft.operation_id().is_some()
        {
            return Err(E::Corrupt);
        }
        let valid_identity = match draft.payload_schema() {
            CLAIM_SCHEMA => {
                draft.draft_id() == value.intent.claim_id()?
                    && draft.revision().get() == value.claim_revision
            }
            BINDING_SCHEMA => {
                draft.draft_id() == value.intent.binding_id()?
                    && draft.revision() == AuthoredDraftRevision::INITIAL
                    && draft.created_at_unix_ms() == draft.updated_at_unix_ms()
            }
            _ => false,
        };
        if !valid_identity
            || value.intent.claim_id()? == value.intent.binding_id()?
            || value.intent.claim_id()?.as_bytes() == &value.intent.draft_id
            || value.intent.binding_id()?.as_bytes() == &value.intent.draft_id
        {
            return Err(E::Corrupt);
        }
        Ok(value)
    }
}
