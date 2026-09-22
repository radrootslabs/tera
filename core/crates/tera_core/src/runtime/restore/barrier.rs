//! Restore admission is durable application metadata, never a delivery stop.

use radroots_storage::{
    Storage,
    authored_draft::{AuthoredDraft, AuthoredDraftId, AuthoredDraftStage},
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{ApplicationRestoreGuard, RestoreError as E};

pub(crate) const BARRIER_SCHEMA: &str = "tera.restore_barrier.v1";
const MAX_BYTES: usize = 16 * 1024;

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Barrier {
    version: u16,
    guard: Vec<u8>,
    pub(super) state: BarrierState,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) enum BarrierState {
    Held,
    Reviewed { inventory: [u8; 32] },
    Resumed { inventory: [u8; 32] },
}

pub(super) fn barrier_id(author: [u8; 32]) -> Result<AuthoredDraftId, E> {
    let mut hash = Sha256::new();
    hash.update(b"tera.restore_barrier.v1\0");
    hash.update(author);
    let digest: [u8; 32] = hash.finalize().into();
    AuthoredDraftId::new(digest[..16].try_into().map_err(|_| E::VerificationFailed)?)
        .map_err(|_| E::VerificationFailed)
}

impl Barrier {
    pub(super) fn held(guard: &ApplicationRestoreGuard) -> Result<Self, E> {
        Ok(Self {
            version: 1,
            guard: guard.encode()?,
            state: BarrierState::Held,
        })
    }

    pub(super) fn guard(&self) -> Result<ApplicationRestoreGuard, E> {
        ApplicationRestoreGuard::decode(&self.guard)
    }

    pub(super) fn encode(&self) -> Result<Vec<u8>, E> {
        if self.version != 1 {
            return Err(E::UnsupportedFormat);
        }
        self.guard()?;
        let bytes = serde_json::to_vec(self).map_err(|_| E::VerificationFailed)?;
        if bytes.len() > MAX_BYTES {
            return Err(E::CapacityExceeded);
        }
        Ok(bytes)
    }

    pub(super) fn decode(head: &AuthoredDraft) -> Result<Self, E> {
        head.validate().map_err(|_| E::VerificationFailed)?;
        if head.payload_schema() != BARRIER_SCHEMA
            || head.payload().len() > MAX_BYTES
            || head.stage() != AuthoredDraftStage::Draft
            || head.operation_id().is_some()
            || head.scope().is_some()
            || head.draft_id() != barrier_id(*head.author())?
        {
            return Err(E::VerificationFailed);
        }
        let value: Self =
            serde_json::from_slice(head.payload()).map_err(|_| E::VerificationFailed)?;
        let guard = value.guard()?;
        if value.encode()? != head.payload()
            || guard.request().backup().author() != *head.author()
            || head.updated_at_unix_ms() < guard.request().requested_at_ms()
        {
            return Err(E::VerificationFailed);
        }
        Ok(value)
    }
}

pub(crate) fn barrier_metadata_is_valid(head: &AuthoredDraft) -> bool {
    Barrier::decode(head).is_ok()
}

pub(super) async fn load(
    store: &dyn Storage,
    author: [u8; 32],
) -> Result<Option<(AuthoredDraft, Barrier)>, E> {
    let head = store
        .authored_draft_head(barrier_id(author)?)
        .await
        .map_err(|_| E::Unavailable)?;
    head.map(|head| Barrier::decode(&head).map(|barrier| (head, barrier)))
        .transpose()
}

/// Called only under cold recovery admission after owner reopen and complete
/// inventory validation. A retry of this exact attempt never rewinds its review.
pub(super) async fn install(store: &dyn Storage, guard: &ApplicationRestoreGuard) -> Result<(), E> {
    let author = guard.request().backup().author();
    let previous = load(store, author).await?;
    if let Some((_, value)) = &previous {
        let old = value.guard()?;
        if old == *guard {
            return Ok(());
        }
        if old.request().attempt_id() == guard.request().attempt_id() {
            return Err(E::Conflict);
        }
    }
    let payload = Barrier::held(guard)?.encode()?;
    let time = guard.request().requested_at_ms();
    let (head, expected) = match previous {
        Some((head, _)) => {
            let next = head
                .successor(payload, AuthoredDraftStage::Draft, None, time)
                .map_err(|_| E::Conflict)?;
            (next, Some(head.revision()))
        }
        None => (
            AuthoredDraft::initial(
                barrier_id(author)?,
                author,
                BARRIER_SCHEMA,
                payload,
                AuthoredDraftStage::Draft,
                None,
                time,
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

impl crate::TeraRuntime {
    pub(crate) async fn validate_restore_startup(&self) -> Result<(), E> {
        let Some(expected) = &self.restore_guard else {
            return Ok(());
        };
        let author = self
            .store_public_key
            .ok_or(E::IdentityMismatch)?
            .into_bytes();
        let store = self.client.storage().map_err(|_| E::Unavailable)?;
        let (_, barrier) = load(store, author).await?.ok_or(E::RecoveryRequired)?;
        if barrier.guard()? != *expected {
            return Err(E::RecoveryRequired);
        }
        Ok(())
    }

    /// All signing, upload and publication paths consult the same durable hold.
    /// Failure to read or validate it cannot be interpreted as no restore.
    pub(crate) async fn require_restore_effects_allowed(&self) -> Result<(), E> {
        let Some(author) = self.store_public_key else {
            return if self.restore_guard.is_none() {
                Ok(())
            } else {
                Err(E::IdentityMismatch)
            };
        };
        let store = self.client.storage().map_err(|_| E::Unavailable)?;
        let barrier = load(store, author.into_bytes()).await?;
        if let Some(expected) = &self.restore_guard {
            let Some((_, barrier)) = &barrier else {
                return Err(E::RecoveryRequired);
            };
            if barrier.guard()? != *expected {
                return Err(E::RecoveryRequired);
            }
        }
        match barrier {
            None => Ok(()),
            Some((
                _,
                Barrier {
                    state: BarrierState::Resumed { .. },
                    ..
                },
            )) => Ok(()),
            Some(_) => Err(E::ReconciliationRequired),
        }
    }
}
