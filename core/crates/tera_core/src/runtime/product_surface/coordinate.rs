//! Application coordinate ownership over opaque shared authored revisions.
//! Captures remain with their existing owner. A claim and its immutable request
//! binding commit together; historical pair replay never grants current rights.

use radroots_event::envelope::event_head::EventHeadCoordinate;
use radroots_event_codec::authoring::AuthoredEventPlan;
use radroots_identity::PublicKey;
use radroots_storage::authored_draft::{AuthoredDraft, AuthoredDraftId};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::Phase1DraftError as E;

mod admission;
mod authority;
mod ordering;
mod record;

pub(super) const CLAIM_SCHEMA: &str = "tera.coordinate_claim.v1";
pub(super) const BINDING_SCHEMA: &str = "tera.coordinate_binding.v1";

/// Ephemeral view of the existing validated capture, never another persisted schema.
pub(super) struct CoordinatePlan {
    pub intent: CoordinateIntent,
    pub created_at: u64,
}

impl CoordinatePlan {
    pub(super) fn from_plan(
        draft: &AuthoredDraft,
        plan: &AuthoredEventPlan,
        prior_event_id: Option<[u8; 32]>,
    ) -> Result<Option<Self>, E> {
        Ok(
            CoordinateIntent::from_plan(draft, plan, prior_event_id)?.map(|intent| Self {
                intent,
                created_at: plan.created_at(),
            }),
        )
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CoordinateIntent {
    pub draft_id: [u8; 16],
    pub author: [u8; 32],
    pub kind: u32,
    pub identifier: String,
    pub event_id: [u8; 32],
    pub prior_event_id: Option<[u8; 32]>,
}

impl CoordinateIntent {
    pub(super) fn from_plan(
        draft: &AuthoredDraft,
        plan: &AuthoredEventPlan,
        prior_event_id: Option<[u8; 32]>,
    ) -> Result<Option<Self>, E> {
        let kind = plan.body().kind();
        if !(30_000..40_000).contains(&kind) {
            return Ok(None);
        }
        let identifier = plan
            .body()
            .tags()
            .iter()
            .find(|tag| tag.first().is_some_and(|name| name == "d"))
            .and_then(|tag| tag.get(1))
            .ok_or(E::InvalidRevision)?
            .clone();
        let value = Self {
            draft_id: *draft.draft_id().as_bytes(),
            author: *plan.author().as_bytes(),
            kind,
            identifier,
            event_id: *plan.expected_event_id().as_bytes(),
            prior_event_id,
        };
        value.validate()?;
        if draft.author() != &value.author {
            return Err(E::Corrupt);
        }
        Ok(Some(value))
    }

    fn validate(&self) -> Result<(), E> {
        AuthoredDraftId::new(self.draft_id).map_err(|_| E::Corrupt)?;
        PublicKey::from_hex(&hex::encode(self.author)).map_err(|_| E::Corrupt)?;
        radroots_event::id::DTag::parse(&self.identifier).map_err(|_| E::Corrupt)?;
        if !matches!(self.kind, 30_402 | 31_922 | 31_923) {
            return Err(E::InvalidRevision);
        }
        Ok(())
    }

    pub(super) fn coordinate(&self) -> Result<EventHeadCoordinate, E> {
        self.validate()?;
        Ok(EventHeadCoordinate::Addressable {
            kind: self.kind,
            pubkey: PublicKey::from_hex(&hex::encode(self.author)).map_err(|_| E::Corrupt)?,
            d_tag: self.identifier.clone(),
        })
    }

    pub(super) fn scope_key(&self) -> [u8; 32] {
        let mut hash = Sha256::new();
        hash.update(b"tera.coordinate_claim.v1\0");
        hash.update(self.kind.to_be_bytes());
        hash.update(self.author);
        hash.update((self.identifier.len() as u64).to_be_bytes());
        hash.update(self.identifier.as_bytes());
        hash.finalize().into()
    }

    pub(super) fn claim_id(&self) -> Result<AuthoredDraftId, E> {
        id(self.scope_key())
    }

    pub(super) fn binding_id(&self) -> Result<AuthoredDraftId, E> {
        let mut hash = Sha256::new();
        hash.update(b"tera.coordinate_binding.v1\0");
        hash.update(self.author);
        hash.update(self.draft_id);
        id(hash.finalize().into())
    }
}

fn id(digest: [u8; 32]) -> Result<AuthoredDraftId, E> {
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    AuthoredDraftId::new(bytes).map_err(|_| E::Corrupt)
}

pub(super) fn metadata_is_valid(draft: &AuthoredDraft) -> bool {
    record::OwnershipRecord::decode(draft).is_ok()
}

pub(super) fn intent_from_draft(draft: &AuthoredDraft) -> Result<Option<CoordinateIntent>, E> {
    Ok(plan_from_draft(draft)?.map(|plan| plan.intent))
}

pub(super) fn plan_from_draft(draft: &AuthoredDraft) -> Result<Option<CoordinatePlan>, E> {
    match draft.payload_schema() {
        super::outbox::DRAFT_PAYLOAD_SCHEMA => super::outbox::coordinate_plan(draft),
        super::SUBMISSION_INTENT_PAYLOAD_SCHEMA => super::submission::coordinate_plan(draft),
        _ => Err(E::Corrupt),
    }
}
