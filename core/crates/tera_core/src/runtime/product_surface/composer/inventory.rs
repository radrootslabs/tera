//! Bounded live inventory over the existing owner; full forms load only by selected ID.

use super::*;
use crate::runtime::product_surface::{AddCommandType, COMPOSER_PAYLOAD_SCHEMA};
use radroots_storage::authored_draft_query::{AuthoredDraftQuery, AuthoredDraftQueryRecord};

pub const COMPOSER_PAGE_LIMIT_MAX: u16 = 256;
const CURSOR_PREFIX: &str = "tera_composer_cursor_v1:";
const CURSOR_BYTES: usize = 80;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ComposerSummary {
    id: ComposerId,
    revision: ComposerRevision,
    edit_sequence: ComposerEditSequence,
    command_type: AddCommandType,
    created_at_unix_ms: u64,
    updated_at_unix_ms: u64,
}

impl ComposerSummary {
    pub const fn id(&self) -> ComposerId {
        self.id
    }
    pub const fn revision(&self) -> ComposerRevision {
        self.revision
    }
    pub const fn edit_sequence(&self) -> ComposerEditSequence {
        self.edit_sequence
    }
    pub const fn command_type(&self) -> AddCommandType {
        self.command_type
    }
    pub const fn created_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }
    pub const fn updated_at_unix_ms(&self) -> u64 {
        self.updated_at_unix_ms
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposerRepairReason {
    UnsupportedSchema,
    CorruptRecord,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ComposerListEntry {
    Draft(ComposerSummary),
    /// An opaque owner locator, which must not be treated as a validated editing ID.
    Repair {
        draft_key: [u8; 16],
        revision: u64,
        reason: ComposerRepairReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComposerPage {
    scope: ComposerScope,
    entries: Vec<ComposerListEntry>,
    next_cursor: Option<String>,
}

impl ComposerPage {
    pub fn scope(&self) -> &ComposerScope {
        &self.scope
    }
    pub fn entries(&self) -> &[ComposerListEntry] {
        &self.entries
    }
    pub fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
}

impl TeraRuntime {
    /// Returns one schema/scope-filtered page in stable ID order.
    /// Inventory is live: resnapshot from the first page to observe insertions behind a cursor.
    /// Storage byte budgets may return fewer records than the requested count, with continuation.
    pub async fn composer_list(
        &self,
        scope: &ComposerScope,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<ComposerPage, ComposerPersistenceError> {
        let _command = self.lifecycle.enter()?;
        let repo = self.composer_repository(scope)?;
        if limit == 0 || limit > COMPOSER_PAGE_LIMIT_MAX {
            return Err(ComposerPersistenceError::InvalidListRequest);
        }
        let digest = ComposerStorageRecord::scope_digest(scope)?;
        let mut query = AuthoredDraftQuery::new(
            scope.author().into_bytes(),
            COMPOSER_PAYLOAD_SCHEMA,
            Some(digest),
            limit,
        )
        .map_err(|_| ComposerPersistenceError::InvalidListRequest)?;
        if let Some(encoded) = cursor {
            let after = decode_cursor(scope, encoded)?;
            let captured = query.cursor_after(after);
            query = query
                .with_cursor(&captured)
                .map_err(|_| ComposerPersistenceError::InvalidCursor)?;
        }
        let page = repo.store.query_authored_drafts(query).await?;
        let next_cursor = if page.next_cursor().is_some() {
            let last = page
                .records()
                .last()
                .ok_or(ComposerPersistenceError::InvalidReceipt)?;
            Some(encode_cursor(scope, last.draft_key())?)
        } else {
            None
        };
        let entries = page
            .into_records()
            .into_iter()
            .map(|record| entry(record, scope))
            .collect();
        Ok(ComposerPage {
            scope: scope.clone(),
            entries,
            next_cursor,
        })
    }
}

fn entry(record: AuthoredDraftQueryRecord, scope: &ComposerScope) -> ComposerListEntry {
    let draft_key = record.draft_key();
    let revision = record.revision().get();
    let reason = match record {
        AuthoredDraftQueryRecord::Draft(stored) => {
            match ComposerStorageRecord::decode(stored, scope) {
                Ok(record) => {
                    return ComposerListEntry::Draft(ComposerSummary {
                        id: record.draft().id(),
                        revision: record.draft().revision(),
                        edit_sequence: record.draft().edit_sequence(),
                        command_type: record.draft().form().input().command_type,
                        created_at_unix_ms: record.stored().created_at_unix_ms(),
                        updated_at_unix_ms: record.stored().updated_at_unix_ms(),
                    });
                }
                Err(ComposerStorageError::UnsupportedSchema) => {
                    ComposerRepairReason::UnsupportedSchema
                }
                Err(_) => ComposerRepairReason::CorruptRecord,
            }
        }
        AuthoredDraftQueryRecord::Corrupt { .. } => ComposerRepairReason::CorruptRecord,
    };
    ComposerListEntry::Repair {
        draft_key,
        revision,
        reason,
    }
}

fn encode_cursor(
    scope: &ComposerScope,
    after: [u8; 16],
) -> Result<String, ComposerPersistenceError> {
    let mut bytes = [0; CURSOR_BYTES];
    bytes[..32].copy_from_slice(scope.author().as_bytes());
    bytes[32..64].copy_from_slice(ComposerStorageRecord::scope_digest(scope)?.as_bytes());
    bytes[64..].copy_from_slice(&after);
    Ok(CURSOR_PREFIX.to_owned() + &hex::encode(bytes))
}

fn decode_cursor(
    scope: &ComposerScope,
    encoded: &str,
) -> Result<[u8; 16], ComposerPersistenceError> {
    if encoded.len() != CURSOR_PREFIX.len() + CURSOR_BYTES * 2 {
        return Err(ComposerPersistenceError::InvalidCursor);
    }
    let value = encoded
        .strip_prefix(CURSOR_PREFIX)
        .ok_or(ComposerPersistenceError::InvalidCursor)?;
    if !value
        .bytes()
        .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(ComposerPersistenceError::InvalidCursor);
    }
    let mut bytes = [0; CURSOR_BYTES];
    hex::decode_to_slice(value, &mut bytes).map_err(|_| ComposerPersistenceError::InvalidCursor)?;
    if &bytes[..32] != scope.author().as_bytes()
        || &bytes[32..64] != ComposerStorageRecord::scope_digest(scope)?.as_bytes()
    {
        return Err(ComposerPersistenceError::ScopeMismatch);
    }
    let mut after = [0; 16];
    after.copy_from_slice(&bytes[64..]);
    Ok(after)
}

#[cfg(test)]
#[path = "inventory_tests.rs"]
mod tests;
