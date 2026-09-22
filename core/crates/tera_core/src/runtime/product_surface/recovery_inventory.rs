//! Bounded, live, author-wide operation traversal, independent of display pages.
//! Callers retain the continuation and revisit the start after a completed pass
//! to see insertions or revisions behind it. No background task owns this scan.

use radroots_storage::{
    authored_draft::{AuthoredDraft, AuthoredDraftId},
    authored_draft_query::{AuthoredDraftQuery, AuthoredDraftQueryRecord},
};

use super::{
    COMPOSER_PAYLOAD_SCHEMA, Phase1DraftError as E, Phase1DraftRepairReason,
    SUBMISSION_INTENT_PAYLOAD_SCHEMA, SUBMISSION_RESERVATION_PAYLOAD_SCHEMA, SubmissionCommitError,
    SubmissionOperationError, SubmissionReservationError, SubmissionReservationRequest, outbox,
    submission,
};
use crate::TeraRuntime;

pub const RECOVERY_PAGE_LIMIT_MAX: u16 = 64;
const CURSOR_PREFIX: &str = "tera_recovery_cursor_v1:";

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RecoveryOwner {
    Legacy,
    Submission(SubmissionReservationRequest),
    Repair(Phase1DraftRepairReason),
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryEntry {
    pub key: [u8; 16],
    pub revision: u64,
    pub owner: RecoveryOwner,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RecoveryPage {
    pub author: [u8; 32],
    pub entries: Vec<RecoveryEntry>,
    /// Counts source positions, including known non-operation records.
    pub scanned: u16,
    pub next_cursor: Option<String>,
}

impl TeraRuntime {
    /// At most `limit` source positions. An empty result with a continuation is
    /// valid: known editable composers are not pending publication operations.
    pub async fn recovery_page(&self, limit: u16, cursor: Option<&str>) -> Result<RecoveryPage, E> {
        let _command = self.lifecycle.enter()?;
        let author = self
            .store_public_key
            .ok_or(E::IdentityUnavailable)?
            .into_bytes();
        if limit == 0 || limit > RECOVERY_PAGE_LIMIT_MAX {
            return Err(E::InvalidInventoryRequest);
        }
        let mut query = AuthoredDraftQuery::for_author_all_schemas(author, limit)
            .map_err(|_| E::InvalidInventoryRequest)?;
        if let Some(value) = cursor {
            let after = query.cursor_after(decode_cursor(author, value)?);
            query = query
                .with_cursor(&after)
                .map_err(|_| E::InvalidInventoryCursor)?;
        }
        let store = self.client.storage().map_err(|_| E::Storage)?;
        let page = store
            .query_authored_drafts(query)
            .await
            .map_err(|_| E::Storage)?;
        let scanned = u16::try_from(page.records().len()).map_err(|_| E::Corrupt)?;
        let next_cursor = if page.next_cursor().is_some() {
            let last = page.records().last().ok_or(E::Corrupt)?;
            Some(encode_cursor(author, last.draft_key()))
        } else {
            None
        };
        let mut entries = Vec::with_capacity(page.records().len());
        for row in page.into_records() {
            match row {
                AuthoredDraftQueryRecord::Draft(stored) => {
                    if let Some(entry) = self.recovery_entry(stored, author).await {
                        entries.push(entry);
                    }
                }
                AuthoredDraftQueryRecord::Corrupt {
                    draft_key,
                    revision,
                } => {
                    entries.push(RecoveryEntry {
                        key: draft_key,
                        revision: revision.get(),
                        owner: RecoveryOwner::Repair(Phase1DraftRepairReason::CorruptRecord),
                    });
                }
            }
        }
        Ok(RecoveryPage {
            author,
            entries,
            scanned,
            next_cursor,
        })
    }

    /// Exact authoritative parent identity for a persisted native transfer.
    /// `None` means no operation at that exact key, never absent from a page.
    pub async fn recovery_parent(&self, key: [u8; 16]) -> Result<Option<RecoveryEntry>, E> {
        let _command = self.lifecycle.enter()?;
        let author = self
            .store_public_key
            .ok_or(E::IdentityUnavailable)?
            .into_bytes();
        let id = AuthoredDraftId::new(key).map_err(|_| E::InvalidDraft)?;
        let store = self.client.storage().map_err(|_| E::Storage)?;
        let Some(stored) = store
            .authored_draft_head(id)
            .await
            .map_err(|_| E::Storage)?
        else {
            return Ok(None);
        };
        // Never return another account's request, scope or payload.
        if stored.author() != &author {
            return Err(E::Corrupt);
        }
        Ok(self.recovery_entry(stored, author).await)
    }

    async fn recovery_entry(
        &self,
        stored: AuthoredDraft,
        author: [u8; 32],
    ) -> Option<RecoveryEntry> {
        let key = *stored.draft_id().as_bytes();
        let revision = stored.revision().get();
        let repair = || RecoveryOwner::Repair(Phase1DraftRepairReason::CorruptRecord);
        let owner = if stored.author() != &author || stored.validate().is_err() {
            repair()
        } else {
            match stored.payload_schema() {
                crate::runtime::restore::REVIEW_SCHEMA => {
                    if crate::runtime::restore::review_metadata_is_valid(&stored) {
                        return None;
                    }
                    repair()
                }
                crate::runtime::restore::BARRIER_SCHEMA => {
                    if crate::runtime::restore::barrier_metadata_is_valid(&stored) {
                        return None;
                    }
                    repair()
                }
                super::coordinate::CLAIM_SCHEMA | super::coordinate::BINDING_SCHEMA => {
                    if super::coordinate::metadata_is_valid(&stored) {
                        return None;
                    }
                    repair()
                }
                "radroots.mobile.phase1-draft.v1" => {
                    if outbox::media_references(&stored).is_ok() {
                        RecoveryOwner::Legacy
                    } else {
                        repair()
                    }
                }
                SUBMISSION_INTENT_PAYLOAD_SCHEMA => {
                    let Ok(store) = self.client.storage() else {
                        return Some(RecoveryEntry {
                            key,
                            revision,
                            owner: RecoveryOwner::Repair(Phase1DraftRepairReason::NeedsAttention),
                        });
                    };
                    match submission::recovery_request(&stored, author, store).await {
                        Ok(request) => RecoveryOwner::Submission(request),
                        Err(error) => RecoveryOwner::Repair(submission_reason(error)),
                    }
                }
                COMPOSER_PAYLOAD_SCHEMA
                | SUBMISSION_RESERVATION_PAYLOAD_SCHEMA
                | "radroots.mobile.phase1-profile.v1" => return None,
                _ => RecoveryOwner::Repair(Phase1DraftRepairReason::UnsupportedSchema),
            }
        };
        Some(RecoveryEntry {
            key,
            revision,
            owner,
        })
    }
}

fn submission_reason(error: SubmissionOperationError) -> Phase1DraftRepairReason {
    match error {
        SubmissionOperationError::Submission(
            SubmissionCommitError::Storage(_)
            | SubmissionCommitError::Reservation(SubmissionReservationError::Storage(_)),
        ) => Phase1DraftRepairReason::NeedsAttention,
        _ => Phase1DraftRepairReason::CorruptRecord,
    }
}

fn encode_cursor(author: [u8; 32], key: [u8; 16]) -> String {
    format!("{CURSOR_PREFIX}{}{}", hex::encode(author), hex::encode(key))
}

fn decode_cursor(author: [u8; 32], value: &str) -> Result<[u8; 16], E> {
    if value.len() != CURSOR_PREFIX.len() + 96 {
        return Err(E::InvalidInventoryCursor);
    }
    let encoded = value
        .strip_prefix(CURSOR_PREFIX)
        .ok_or(E::InvalidInventoryCursor)?;
    if !encoded
        .bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    {
        return Err(E::InvalidInventoryCursor);
    }
    let mut bytes = [0; 48];
    hex::decode_to_slice(encoded, &mut bytes).map_err(|_| E::InvalidInventoryCursor)?;
    if bytes[..32] != author {
        return Err(E::InvalidInventoryCursor);
    }
    bytes[32..]
        .try_into()
        .map_err(|_| E::InvalidInventoryCursor)
}

#[cfg(test)]
mod tests;
