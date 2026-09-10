//! Application save policy over the existing owner; no SQL, signer or network port.

use super::{
    ComposerDraft, ComposerEditSequence, ComposerId, ComposerPartialForm, ComposerRevision,
    ComposerScope, ComposerStorageError, ComposerStorageRecord,
};
use crate::{TeraRuntime, runtime::lifecycle::RuntimeLifecycleError};
use radroots_storage::{
    Error,
    authored_draft::{AuthoredDraftId, AuthoredDraftStore, DraftAppendDisposition},
};

#[derive(Clone, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ComposerPersistenceError {
    #[error(transparent)]
    Lifecycle(#[from] RuntimeLifecycleError),
    #[error("composer store identity is unavailable")]
    OwnerUnavailable,
    #[error("composer belongs to a different account or context")]
    ScopeMismatch,
    #[error("composer was not found")]
    NotFound,
    #[error("composer revision conflicts with durable state")]
    RevisionConflict,
    #[error("composer edit sequence does not advance durable editing")]
    EditSequenceConflict,
    #[error("composer revision cannot advance")]
    RevisionOverflow,
    #[error("composer save receipt does not match the requested revision")]
    InvalidReceipt,
    #[error(transparent)]
    Record(ComposerStorageError),
    #[error("composer storage failed: {0}")]
    Storage(Error),
}

impl From<ComposerStorageError> for ComposerPersistenceError {
    fn from(error: ComposerStorageError) -> Self {
        match error {
            ComposerStorageError::ScopeMismatch => Self::ScopeMismatch,
            other => Self::Record(other),
        }
    }
}

impl From<Error> for ComposerPersistenceError {
    fn from(error: Error) -> Self {
        match error {
            Error::DraftRevisionConflict => Self::RevisionConflict,
            Error::DraftNotFound => Self::NotFound,
            Error::CorruptAuthoredDraft => Self::Record(ComposerStorageError::CorruptRecord),
            other => Self::Storage(other),
        }
    }
}

/// Acknowledges exactly one owner-committed revision, possibly an exact replay.
/// Later revisions can exist by callback time; callers must retain edit-order guards.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComposerSaveReceipt {
    draft: ComposerDraft,
    replayed: bool,
}

impl ComposerSaveReceipt {
    pub fn draft(&self) -> &ComposerDraft {
        &self.draft
    }
    pub const fn is_replay(&self) -> bool {
        self.replayed
    }
}

struct ComposerRepository<'a> {
    store: &'a dyn AuthoredDraftStore,
    scope: &'a ComposerScope,
}

impl ComposerRepository<'_> {
    async fn load(
        &self,
        id: ComposerId,
    ) -> Result<ComposerStorageRecord, ComposerPersistenceError> {
        let id = AuthoredDraftId::new(*id.as_bytes())
            .map_err(|_| ComposerStorageError::InvalidRecord)?;
        let stored = self
            .store
            .authored_draft_head(id)
            .await?
            .ok_or(ComposerPersistenceError::NotFound)?;
        if stored.draft_id() != id {
            return Err(ComposerStorageError::CorruptRecord.into());
        }
        Ok(ComposerStorageRecord::decode(stored, self.scope)?)
    }

    async fn create(
        &self,
        id: ComposerId,
        edit_sequence: ComposerEditSequence,
        form: ComposerPartialForm,
        now_unix_ms: u64,
    ) -> Result<ComposerSaveReceipt, ComposerPersistenceError> {
        let candidate = ComposerStorageRecord::initial(
            id,
            self.scope.clone(),
            edit_sequence,
            form,
            now_unix_ms,
        )?;
        self.commit(candidate, None).await
    }

    async fn save(
        &self,
        id: ComposerId,
        expected_revision: ComposerRevision,
        edit_sequence: ComposerEditSequence,
        form: ComposerPartialForm,
        now_unix_ms: u64,
    ) -> Result<ComposerSaveReceipt, ComposerPersistenceError> {
        let current = self.load(id).await?;
        if current.draft().revision() != expected_revision {
            return Err(ComposerPersistenceError::RevisionConflict);
        }
        if edit_sequence <= current.draft().edit_sequence() {
            return Err(ComposerPersistenceError::EditSequenceConflict);
        }
        expected_revision
            .next()
            .map_err(|_| ComposerPersistenceError::RevisionOverflow)?;
        let candidate = current.successor(edit_sequence, form, now_unix_ms)?;
        self.commit(candidate, Some(current.stored().revision()))
            .await
    }

    async fn commit(
        &self,
        candidate: ComposerStorageRecord,
        expected: Option<radroots_storage::authored_draft::AuthoredDraftRevision>,
    ) -> Result<ComposerSaveReceipt, ComposerPersistenceError> {
        let receipt = self
            .store
            .append_authored_draft(candidate.stored().clone(), expected)
            .await?;
        if receipt.draft() != candidate.stored() {
            return Err(ComposerPersistenceError::InvalidReceipt);
        }
        Ok(ComposerSaveReceipt {
            draft: candidate.draft().clone(),
            replayed: receipt.disposition() == DraftAppendDisposition::Replay,
        })
    }
}

impl TeraRuntime {
    /// Creates local editing under a previously reserved ID; no publish authority is acquired.
    pub async fn composer_create(
        &self,
        scope: &ComposerScope,
        id: ComposerId,
        edit_sequence: ComposerEditSequence,
        form: ComposerPartialForm,
    ) -> Result<ComposerSaveReceipt, ComposerPersistenceError> {
        let _command = self.lifecycle.enter()?;
        self.composer_repository(scope)?
            .create(id, edit_sequence, form, now_unix_ms()?)
            .await
    }

    /// Loads exactly the selected ID and stable account/context without repairing bytes.
    pub async fn composer_load(
        &self,
        scope: &ComposerScope,
        id: ComposerId,
    ) -> Result<ComposerDraft, ComposerPersistenceError> {
        let _command = self.lifecycle.enter()?;
        Ok(self
            .composer_repository(scope)?
            .load(id)
            .await?
            .draft()
            .clone())
    }

    /// Saves a newer edit with owner CAS, returning only its exact durable acknowledgement.
    pub async fn composer_save(
        &self,
        scope: &ComposerScope,
        id: ComposerId,
        expected_revision: ComposerRevision,
        edit_sequence: ComposerEditSequence,
        form: ComposerPartialForm,
    ) -> Result<ComposerSaveReceipt, ComposerPersistenceError> {
        let _command = self.lifecycle.enter()?;
        self.composer_repository(scope)?
            .save(id, expected_revision, edit_sequence, form, now_unix_ms()?)
            .await
    }

    fn composer_repository<'a>(
        &'a self,
        scope: &'a ComposerScope,
    ) -> Result<ComposerRepository<'a>, ComposerPersistenceError> {
        let author = self
            .store_public_key
            .ok_or(ComposerPersistenceError::OwnerUnavailable)?;
        if author != scope.author() {
            return Err(ComposerPersistenceError::ScopeMismatch);
        }
        let store = self
            .client
            .storage()
            .map_err(|_| ComposerPersistenceError::Storage(Error::BackendUnavailable))?;
        Ok(ComposerRepository { store, scope })
    }
}

fn now_unix_ms() -> Result<u64, ComposerStorageError> {
    u64::try_from(chrono::Utc::now().timestamp_millis())
        .ok()
        .filter(|time| *time > 0)
        .ok_or(ComposerStorageError::InvalidTimestamp)
}

#[cfg(test)]
#[path = "repository_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "repository_sqlite_tests.rs"]
mod sqlite_tests;
