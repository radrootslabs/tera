//! Runtime-local admission for existing durable authoring scopes.
//!
//! Reservations cover the whole caller-owned future, including signer callbacks.
//! The mutex protects only set insertion/removal and never crosses an await.
//! Dropping a reservation releases in-memory admission, not durable claims or
//! external effects; recovery still uses the existing draft and signing records.

use std::{collections::BTreeSet, sync::Mutex};

use radroots_signing::SigningOperationId;
use radroots_storage::authored_draft::AuthoredDraftId;

use super::product_surface::Phase1DraftError;

#[derive(Clone, Copy, Eq, Ord, PartialEq, PartialOrd)]
enum Scope {
    Draft(AuthoredDraftId),
    Authorization(SigningOperationId),
}

#[derive(Default)]
pub(super) struct MutationAdmission {
    active: Mutex<BTreeSet<Scope>>,
}

impl MutationAdmission {
    pub(super) fn draft(&self, id: [u8; 16]) -> Result<MutationPermit<'_>, Phase1DraftError> {
        let id = AuthoredDraftId::new(id).map_err(|_| Phase1DraftError::InvalidDraft)?;
        self.reserve(Scope::Draft(id))
    }

    pub(super) fn authorization(
        &self,
        id: SigningOperationId,
    ) -> Result<MutationPermit<'_>, Phase1DraftError> {
        self.reserve(Scope::Authorization(id))
    }

    fn reserve(&self, scope: Scope) -> Result<MutationPermit<'_>, Phase1DraftError> {
        let mut active = self
            .active
            .lock()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?;
        if !active.insert(scope) {
            return Err(Phase1DraftError::OperationInProgress);
        }
        Ok(MutationPermit { owner: self, scope })
    }
}

/// Unique, non-cloneable ownership of one in-flight transition.
pub(super) struct MutationPermit<'a> {
    owner: &'a MutationAdmission,
    scope: Scope,
}

impl Drop for MutationPermit<'_> {
    fn drop(&mut self) {
        // Poison remains fail-closed for future admissions. Cleanup does not
        // recover a poisoned set or perform storage, signing, or network work.
        if let Ok(mut active) = self.owner.active.lock() {
            active.remove(&self.scope);
        }
    }
}
