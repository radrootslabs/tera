//! Bounded reservation pages recover operations without a retained native handle.

use super::{
    SubmissionCommitError, SubmissionOperationError as E, SubmissionReservationError,
    SubmissionReservationRequest, record, repository::SubmissionRepository,
};
use crate::{
    TeraRuntime,
    runtime::product_surface::{
        ComposerScope, ComposerStorageRecord, Phase1DraftError, Phase1DraftRepairReason,
        Phase1OutboxState,
    },
};
use radroots_storage::{
    Error,
    authored_draft::{AUTHORED_DRAFT_QUERY_LIMIT_MAX, AuthoredDraftId},
    authored_draft_query::{AuthoredDraftQuery, AuthoredDraftQueryRecord, AuthoredDraftScope},
    journal::OperationInstanceId,
};

const CURSOR_PREFIX: &str = "tera_submission_cursor_v1:";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SubmissionSummaryState {
    Reserved,
    Operation {
        intent_id: AuthoredDraftId,
        operation_id: OperationInstanceId,
        revision: u64,
        state: Phase1OutboxState,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmissionSummary {
    request: SubmissionReservationRequest,
    reservation_id: AuthoredDraftId,
    reserved_at_unix_ms: u64,
    state: SubmissionSummaryState,
}

impl SubmissionSummary {
    pub fn request(&self) -> &SubmissionReservationRequest {
        &self.request
    }
    pub const fn reservation_id(&self) -> AuthoredDraftId {
        self.reservation_id
    }
    pub const fn reserved_at_unix_ms(&self) -> u64 {
        self.reserved_at_unix_ms
    }
    pub const fn state(&self) -> SubmissionSummaryState {
        self.state
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum SubmissionListEntry {
    Submission(SubmissionSummary),
    Repair {
        reservation_key: [u8; 16],
        revision: u64,
        reason: Phase1DraftRepairReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmissionPage {
    scope: ComposerScope,
    entries: Vec<SubmissionListEntry>,
    next_cursor: Option<String>,
}

impl SubmissionPage {
    pub fn scope(&self) -> &ComposerScope {
        &self.scope
    }
    pub fn entries(&self) -> &[SubmissionListEntry] {
        &self.entries
    }
    pub fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
}

impl TeraRuntime {
    /// Live stable-ID traversal; revisit the first page for new earlier IDs.
    /// Captured forms are validated one at a time, never retained by the page.
    pub async fn submission_page(
        &self,
        scope: &ComposerScope,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<SubmissionPage, E> {
        let _command = self.lifecycle.enter().map_err(Phase1DraftError::from)?;
        if self.store_public_key != Some(scope.author()) {
            return Err(
                SubmissionCommitError::from(SubmissionReservationError::Source(
                    super::ComposerPersistenceError::ScopeMismatch,
                ))
                .into(),
            );
        }
        if limit == 0 || limit > AUTHORED_DRAFT_QUERY_LIMIT_MAX {
            return Err(Phase1DraftError::InvalidInventoryRequest.into());
        }
        let digest = ComposerStorageRecord::scope_digest(scope).map_err(|_| E::Corrupt)?;
        let mut query = AuthoredDraftQuery::new(
            scope.author().into_bytes(),
            record::SUBMISSION_RESERVATION_PAYLOAD_SCHEMA,
            Some(digest),
            limit,
        )?;
        if let Some(cursor) = cursor {
            let after = query.cursor_after(decode_cursor(digest, cursor)?);
            query = query.with_cursor(&after)?;
        }
        let store = self
            .client
            .storage()
            .map_err(|_| Error::BackendUnavailable)?;
        let page = store.query_authored_drafts(query).await?;
        let next_cursor = if page.next_cursor().is_some() {
            let (key, _) = key(page.records().last().ok_or(E::Corrupt)?);
            Some(format!(
                "{CURSOR_PREFIX}{}{}",
                hex::encode(digest.as_bytes()),
                hex::encode(key)
            ))
        } else {
            None
        };
        let mut entries = Vec::with_capacity(page.records().len());
        for record in page.into_records() {
            let (reservation_key, revision) = key(&record);
            entries.push(match self.submission_inventory_entry(scope, record).await {
                Ok(summary) => SubmissionListEntry::Submission(summary),
                Err(error) => SubmissionListEntry::Repair {
                    reservation_key,
                    revision,
                    reason: repair_reason(error),
                },
            });
        }
        Ok(SubmissionPage {
            scope: scope.clone(),
            entries,
            next_cursor,
        })
    }

    async fn submission_inventory_entry(
        &self,
        scope: &ComposerScope,
        record: AuthoredDraftQueryRecord,
    ) -> Result<SubmissionSummary, E> {
        let AuthoredDraftQueryRecord::Draft(draft) = record else {
            return Err(E::Corrupt);
        };
        let request = record::request_from(&draft, scope).map_err(SubmissionCommitError::from)?;
        let store = self
            .client
            .storage()
            .map_err(|_| Error::BackendUnavailable)?;
        let reservation = SubmissionRepository { store }
            .replay(&request)
            .await
            .map_err(SubmissionCommitError::from)?
            .ok_or(E::Corrupt)?;
        let state = match self.submission_operation_status(&request).await {
            Ok(status) => SubmissionSummaryState::Operation {
                intent_id: status.receipt().intent_id(),
                operation_id: status.receipt().operation_id(),
                revision: status.intent().revision().get(),
                state: status.state(),
            },
            Err(E::NotFound) => SubmissionSummaryState::Reserved,
            Err(error) => return Err(error),
        };
        Ok(SubmissionSummary {
            request,
            reservation_id: reservation.reservation_id(),
            reserved_at_unix_ms: reservation.reserved_at_unix_ms(),
            state,
        })
    }
}

fn key(record: &AuthoredDraftQueryRecord) -> ([u8; 16], u64) {
    match record {
        AuthoredDraftQueryRecord::Draft(draft) => {
            (*draft.draft_id().as_bytes(), draft.revision().get())
        }
        AuthoredDraftQueryRecord::Corrupt {
            draft_key,
            revision,
        } => (*draft_key, revision.get()),
    }
}

fn repair_reason(error: E) -> Phase1DraftRepairReason {
    match error {
        E::Submission(
            SubmissionCommitError::UnsupportedSchema
            | SubmissionCommitError::Reservation(SubmissionReservationError::UnsupportedSchema),
        ) => Phase1DraftRepairReason::UnsupportedSchema,
        E::Submission(
            SubmissionCommitError::Storage(_)
            | SubmissionCommitError::Reservation(SubmissionReservationError::Storage(_)),
        )
        | E::Operation(
            Phase1DraftError::Storage
            | Phase1DraftError::Operation
            | Phase1DraftError::OperationUnavailable
            | Phase1DraftError::OperationInProgress
            | Phase1DraftError::Lifecycle(_),
        ) => Phase1DraftRepairReason::NeedsAttention,
        _ => Phase1DraftRepairReason::CorruptRecord,
    }
}

fn decode_cursor(scope: AuthoredDraftScope, value: &str) -> Result<[u8; 16], E> {
    let invalid = || E::Operation(Phase1DraftError::InvalidInventoryCursor);
    if value.len() != CURSOR_PREFIX.len() + 96 {
        return Err(invalid());
    }
    let encoded = value.strip_prefix(CURSOR_PREFIX).ok_or_else(invalid)?;
    if !encoded
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(invalid());
    }
    let mut bytes = [0; 48];
    hex::decode_to_slice(encoded, &mut bytes).map_err(|_| invalid())?;
    if &bytes[..32] != scope.as_bytes() {
        return Err(invalid());
    }
    bytes[32..].try_into().map_err(|_| invalid())
}
