//! A fresh, exact-target observation is distinct from historical provenance.
use radroots_storage::{
    Storage,
    authored_draft::{AuthoredDraft, AuthoredDraftId, AuthoredDraftStage},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ApplicationRestoreGuard, RestoreError as E};
pub(crate) const REVIEW_SCHEMA: &str = "tera.restore_review.v1";
const MAX_BYTES: usize = 4096;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub enum RestoreObservation {
    /// The exact event was admitted from this target during this request.
    Observed,
    /// The bounded selector completed without this event. This is not proof
    /// that the target never accepted or retained it.
    NotObserved,
    /// Missing, failed, partial or otherwise unknown current remote evidence.
    Incomplete,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RestoreTargetReview {
    version: u16,
    pub guard_sha256: [u8; 32],
    pub inventory_sha256: [u8; 32],
    pub author: [u8; 32],
    pub draft_id: [u8; 16],
    pub operation_id: [u8; 16],
    pub event_id: [u8; 32],
    pub target_fingerprint: String,
    pub observed_at_ms: u64,
    pub observation: RestoreObservation,
}

pub(super) fn guard_digest(guard: &ApplicationRestoreGuard) -> Result<[u8; 32], E> {
    Ok(Sha256::digest(guard.encode()?).into())
}

impl RestoreTargetReview {
    pub(super) fn new(
        guard: &ApplicationRestoreGuard,
        inventory: [u8; 32],
        draft_id: [u8; 16],
        request: &radroots_sync::PushRequest,
        target: &radroots_transport::Target,
        time: u64,
        observation: RestoreObservation,
    ) -> Result<Self, E> {
        let value = Self {
            version: 1,
            guard_sha256: guard_digest(guard)?,
            inventory_sha256: inventory,
            author: guard.request().backup().author(),
            draft_id,
            operation_id: *request.operation_id().as_bytes(),
            event_id: *request.plan().expected_event_id().as_bytes(),
            target_fingerprint: target.fingerprint().as_str().to_owned(),
            observed_at_ms: time,
            observation,
        };
        value.encode()?;
        Ok(value)
    }

    fn id(&self) -> Result<AuthoredDraftId, E> {
        let mut hash = Sha256::new();
        hash.update(b"tera.restore_review.v1\0");
        hash.update(self.guard_sha256);
        hash.update(self.author);
        hash.update(self.draft_id);
        hash.update(self.target_fingerprint.as_bytes());
        let digest: [u8; 32] = hash.finalize().into();
        AuthoredDraftId::new(digest[..16].try_into().map_err(|_| E::VerificationFailed)?)
            .map_err(|_| E::VerificationFailed)
    }

    fn encode(&self) -> Result<Vec<u8>, E> {
        if self.version != 1 {
            return Err(E::UnsupportedFormat);
        }
        if self.observed_at_ms == 0
            || self.observed_at_ms > i64::MAX as u64
            || self.draft_id == [0; 16]
            || self.operation_id == [0; 16]
            || radroots_identity::PublicKey::from_bytes(self.author).is_err()
            || radroots_transport::target::TargetFingerprint::parse(&self.target_fingerprint)
                .map_or(true, |value| value.as_str() != self.target_fingerprint)
        {
            return Err(E::VerificationFailed);
        }
        let bytes = serde_json::to_vec(self).map_err(|_| E::VerificationFailed)?;
        if bytes.len() > MAX_BYTES {
            return Err(E::CapacityExceeded);
        }
        Ok(bytes)
    }

    fn decode(head: &AuthoredDraft) -> Result<Self, E> {
        head.validate().map_err(|_| E::VerificationFailed)?;
        if head.payload_schema() != REVIEW_SCHEMA
            || head.payload().len() > MAX_BYTES
            || head.stage() != AuthoredDraftStage::Draft
            || head.scope().is_some()
            || head.operation_id().is_some()
        {
            return Err(E::VerificationFailed);
        }
        let value: Self =
            serde_json::from_slice(head.payload()).map_err(|_| E::VerificationFailed)?;
        if value.encode()? != head.payload()
            || value.id()? != head.draft_id()
            || value.author != *head.author()
            || value.observed_at_ms != head.updated_at_unix_ms()
        {
            return Err(E::VerificationFailed);
        }
        Ok(value)
    }

    pub(super) async fn persist(&self, store: &dyn Storage) -> Result<(), E> {
        let previous = store
            .authored_draft_head(self.id()?)
            .await
            .map_err(|_| E::Unavailable)?;
        let (head, expected) = match previous {
            Some(head) => {
                Self::decode(&head)?;
                let next = head
                    .successor(
                        self.encode()?,
                        AuthoredDraftStage::Draft,
                        None,
                        self.observed_at_ms,
                    )
                    .map_err(|_| E::Conflict)?;
                (next, Some(head.revision()))
            }
            None => (
                AuthoredDraft::initial(
                    self.id()?,
                    self.author,
                    REVIEW_SCHEMA,
                    self.encode()?,
                    AuthoredDraftStage::Draft,
                    None,
                    self.observed_at_ms,
                )
                .map_err(|_| E::VerificationFailed)?,
                None,
            ),
        };
        store
            .append_authored_draft(head, expected)
            .await
            .map_err(|_| E::Conflict)?;
        Ok(())
    }

    pub(super) async fn load_current(&self, store: &dyn Storage) -> Result<Option<Self>, E> {
        let Some(head) = store
            .authored_draft_head(self.id()?)
            .await
            .map_err(|_| E::Unavailable)?
        else {
            return Ok(None);
        };
        let value = Self::decode(&head)?;
        if value.guard_sha256 != self.guard_sha256
            || value.inventory_sha256 != self.inventory_sha256
            || value.draft_id != self.draft_id
            || value.operation_id != self.operation_id
            || value.event_id != self.event_id
            || value.target_fingerprint != self.target_fingerprint
            || value.author != self.author
        {
            return Ok(None);
        }
        Ok(Some(value))
    }
}

pub(crate) fn review_metadata_is_valid(head: &AuthoredDraft) -> bool {
    RestoreTargetReview::decode(head).is_ok()
}
