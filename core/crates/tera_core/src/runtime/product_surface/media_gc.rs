//! Ephemeral application reference reconciliation; no second persistence owner.
//!
//! The host must hold the exclusive file-maintenance reservation for this entire
//! inventory and every subsequent conditional unlink. No result survives that fence.

use std::collections::BTreeSet;

use radroots_storage::{
    Storage,
    authored_draft::AuthoredDraft,
    authored_draft_query::{AuthoredDraftQuery, AuthoredDraftQueryRecord},
};

use super::{composer, outbox, submission};

mod inspection;
pub use inspection::inspect_media_references;

pub const MEDIA_ORPHAN_GRACE_MS: u64 = 7 * 24 * 60 * 60 * 1000;
pub const MEDIA_ACCOUNT_LIMIT: usize = 64;
pub const MEDIA_PAGE_LIMIT: u16 = 32;
pub const MEDIA_PAGE_BUDGET: usize = 128;
pub const MEDIA_RECORD_BUDGET: usize = 4096;
pub const MEDIA_PAYLOAD_BYTE_BUDGET: usize = 64 * 1024 * 1024;
pub const MEDIA_REFERENCE_BUDGET: usize = 65536;
pub const MEDIA_DIRECTORY_ENTRY_BUDGET: usize = 8192;
pub const MEDIA_REMOVAL_BUDGET: usize = 64;

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
#[error("media ownership could not be completely reconciled")]
pub struct MediaInventoryIncomplete;
type Result<T> = std::result::Result<T, MediaInventoryIncomplete>;

/// All resource counters are aggregate, including exact historical lookups.
#[derive(Default)]
pub(super) struct InventoryBudget {
    pages: usize,
    records: usize,
    bytes: usize,
}

impl InventoryBudget {
    pub(super) fn record(&mut self, stored: &AuthoredDraft) -> Result<()> {
        self.records = self
            .records
            .checked_add(1)
            .ok_or(MediaInventoryIncomplete)?;
        self.bytes = self
            .bytes
            .checked_add(stored.payload().len())
            .ok_or(MediaInventoryIncomplete)?;
        if self.records > MEDIA_RECORD_BUDGET || self.bytes > MEDIA_PAYLOAD_BYTE_BUDGET {
            return Err(MediaInventoryIncomplete);
        }
        Ok(())
    }
}

/// Constructed only after a complete inventory. There is deliberately no API to
/// create an empty proof, remove a reference, or forgive an inventory error.
pub struct MediaReferenceInventory {
    hashes: BTreeSet<String>,
}

impl MediaReferenceInventory {
    /// Policy only: the host still proves regular single-link file identity and
    /// performs same-scan conditional unlink under its retained exclusive fence.
    pub fn permits_orphan(&self, name: &str, modified_ms: u64, now_ms: u64) -> bool {
        let old_enough = modified_ms > 0
            && now_ms <= i64::MAX as u64
            && now_ms
                .checked_sub(modified_ms)
                .is_some_and(|age| age >= MEDIA_ORPHAN_GRACE_MS);
        old_enough && ((canonical_hash(name) && !self.hashes.contains(name)) || scratch_name(name))
    }
}

pub(super) fn canonical_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(lower_hex)
}

fn lower_hex(value: u8) -> bool {
    value.is_ascii_digit() || (b'a'..=b'f').contains(&value)
}

fn scratch_name(value: &str) -> bool {
    let Some(id) = value.strip_prefix(".radroots_pending_") else {
        return false;
    };
    id.len() == 36
        && id.bytes().enumerate().all(|(index, byte)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                byte == b'-'
            } else {
                lower_hex(byte)
            }
        })
}

fn retain(hashes: &mut BTreeSet<String>, value: String) -> Result<()> {
    if !canonical_hash(&value) {
        return Err(MediaInventoryIncomplete);
    }
    hashes.insert(value);
    if hashes.len() > MEDIA_REFERENCE_BUDGET {
        return Err(MediaInventoryIncomplete);
    }
    Ok(())
}

async fn collect_account(
    store: &dyn Storage,
    author: [u8; 32],
    hashes: &mut BTreeSet<String>,
    budget: &mut InventoryBudget,
) -> Result<()> {
    let mut query = AuthoredDraftQuery::for_author_all_schemas(author, MEDIA_PAGE_LIMIT)
        .map_err(|_| MediaInventoryIncomplete)?;
    loop {
        budget.pages += 1;
        if budget.pages > MEDIA_PAGE_BUDGET {
            return Err(MediaInventoryIncomplete);
        }
        let page = store
            .query_authored_drafts(query.clone())
            .await
            .map_err(|_| MediaInventoryIncomplete)?;
        let next = page.next_cursor().cloned();
        if next.is_some() && page.records().is_empty() {
            return Err(MediaInventoryIncomplete);
        }
        for record in page.into_records() {
            let AuthoredDraftQueryRecord::Draft(stored) = record else {
                return Err(MediaInventoryIncomplete);
            };
            budget.record(&stored)?;
            if stored.author() != &author {
                return Err(MediaInventoryIncomplete);
            }
            stored.validate().map_err(|_| MediaInventoryIncomplete)?;
            let references = match stored.payload_schema() {
                super::COMPOSER_PAYLOAD_SCHEMA => composer::media_references(stored, author)?,
                super::SUBMISSION_RESERVATION_PAYLOAD_SCHEMA
                | super::SUBMISSION_INTENT_PAYLOAD_SCHEMA => {
                    submission::media_references(stored, author, store, budget).await?
                }
                "radroots.mobile.phase1-draft.v1"
                | "radroots.mobile.phase1-profile.v1"
                | super::coordinate::CLAIM_SCHEMA
                | super::coordinate::BINDING_SCHEMA => outbox::media_references(&stored)?,
                _ => return Err(MediaInventoryIncomplete),
            };
            for hash in references {
                retain(hashes, hash)?;
            }
        }
        let Some(next) = next else { return Ok(()) };
        query = query
            .with_cursor(&next)
            .map_err(|_| MediaInventoryIncomplete)?;
    }
}

#[cfg(test)]
mod tests;
