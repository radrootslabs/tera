//! Admission over one existing journal owner; reservation has no external effects.

use radroots_storage::{
    Error,
    authored_draft::{AuthoredDraft, AuthoredDraftId, AuthoredDraftStore, DraftAppendDisposition},
};

use super::{
    SubmissionReservationError as E, SubmissionReservationReceipt, SubmissionReservationRequest,
    record,
};
use crate::{
    TeraRuntime,
    runtime::product_surface::{ComposerPersistenceError as SourceError, ComposerStorageRecord},
};

struct SubmissionRepository<'a> {
    store: &'a dyn AuthoredDraftStore,
}

#[cfg(test)]
#[path = "sqlite_tests.rs"]
mod sqlite_tests;
#[cfg(test)]
#[path = "repository_tests.rs"]
mod tests;

impl SubmissionRepository<'_> {
    async fn replay(
        &self,
        request: &SubmissionReservationRequest,
    ) -> Result<Option<SubmissionReservationReceipt>, E> {
        let Some(stored) = self
            .store
            .authored_draft_head(record::reservation_id(request)?)
            .await?
        else {
            return Ok(None);
        };
        self.receipt(request, stored, true).await.map(Some)
    }

    async fn receipt(
        &self,
        request: &SubmissionReservationRequest,
        stored: AuthoredDraft,
        replayed: bool,
    ) -> Result<SubmissionReservationReceipt, E> {
        let wire = record::decode(&stored, request)?;
        let source = self
            .store
            .authored_draft_revision(wire.source.draft_id(), wire.source.revision())
            .await?
            .ok_or(E::CorruptRecord)?;
        if !wire.source.matches(&source)
            || source.updated_at_unix_ms() > stored.created_at_unix_ms()
        {
            return Err(E::CorruptRecord);
        }
        let captured = ComposerStorageRecord::decode(source, &request.scope)
            .map_err(SourceError::from)?
            .draft()
            .clone();
        Ok(SubmissionReservationReceipt {
            request: request.clone(),
            reservation_id: stored.draft_id(),
            captured,
            reserved_at_unix_ms: stored.created_at_unix_ms(),
            replayed,
        })
    }

    async fn reserve(
        &self,
        request: &SubmissionReservationRequest,
        observed_time: Option<u64>,
    ) -> Result<SubmissionReservationReceipt, E> {
        // A saved reservation outranks a newer live composer head and a fresh clock.
        if let Some(receipt) = self.replay(request).await? {
            return Ok(receipt);
        }
        let id =
            AuthoredDraftId::new(*request.composer_id.as_bytes()).map_err(|_| E::CorruptRecord)?;
        let stored = self
            .store
            .authored_draft_head(id)
            .await?
            .ok_or(SourceError::NotFound)?;
        if stored.draft_id() != id {
            return Err(E::CorruptRecord);
        }
        let source =
            ComposerStorageRecord::decode(stored, &request.scope).map_err(SourceError::from)?;
        if source.draft().revision() != request.expected_revision {
            // A concurrent equivalent reservation may have won before a newer edit.
            return self
                .replay(request)
                .await?
                .ok_or(SourceError::RevisionConflict.into());
        }
        let candidate = record::initial(request, &source, observed_time)?;
        match self
            .store
            .append_authored_draft(candidate.clone(), None)
            .await
        {
            Ok(receipt) => {
                if receipt.draft() != &candidate {
                    return Err(E::InvalidReceipt);
                }
                self.receipt(
                    request,
                    candidate,
                    receipt.disposition() == DraftAppendDisposition::Replay,
                )
                .await
            }
            Err(Error::DraftRevisionConflict) => {
                self.replay(request).await?.ok_or(E::InvalidReceipt)
            }
            Err(error) => Err(error.into()),
        }
    }
}

impl TeraRuntime {
    /// Reserves one saved source before strict publication, signing or delivery.
    /// Retry the same command to recover an uncertain acknowledgement.
    pub async fn submission_reserve(
        &self,
        request: &SubmissionReservationRequest,
    ) -> Result<SubmissionReservationReceipt, E> {
        let _command = self.lifecycle.enter()?;
        let author = self.store_public_key.ok_or(SourceError::OwnerUnavailable)?;
        if author != request.scope.author() {
            return Err(SourceError::ScopeMismatch.into());
        }
        let store = self
            .client
            .storage()
            .map_err(|_| Error::BackendUnavailable)?;
        SubmissionRepository { store }
            .reserve(
                request,
                u64::try_from(chrono::Utc::now().timestamp_millis()).ok(),
            )
            .await
    }
}
