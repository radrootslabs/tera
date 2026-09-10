//! Bounded author-owned legacy selection. Payloads and operation state are
//! interpreted per record; one damaged record cannot hide the next page.

use super::*;
use radroots_storage::authored::OperationSettlement;
use radroots_storage::authored_draft::AUTHORED_DRAFT_QUERY_LIMIT_MAX;

const CURSOR_PREFIX: &str = "tera_legacy_draft_cursor_v1:";

#[cfg(test)]
#[path = "inventory_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Phase1DraftSummary {
    draft_id: AuthoredDraftId,
    revision: AuthoredDraftRevision,
    kind: Phase1DraftKind,
    command_type: AddCommandType,
    state: Phase1OutboxState,
    has_form: bool,
    is_revision: bool,
    created_at_unix_ms: u64,
    updated_at_unix_ms: u64,
    media_count: u64,
    verified_media_count: u64,
    possible_orphan_count: u64,
    settlement: Option<OperationSettlement>,
}

impl Phase1DraftSummary {
    pub const fn draft_id(&self) -> AuthoredDraftId {
        self.draft_id
    }
    pub const fn revision(&self) -> AuthoredDraftRevision {
        self.revision
    }
    pub const fn kind(&self) -> Phase1DraftKind {
        self.kind
    }
    pub const fn command_type(&self) -> AddCommandType {
        self.command_type
    }
    pub const fn state(&self) -> Phase1OutboxState {
        self.state
    }
    pub const fn has_form(&self) -> bool {
        self.has_form
    }
    pub const fn is_revision(&self) -> bool {
        self.is_revision
    }
    pub const fn created_at_unix_ms(&self) -> u64 {
        self.created_at_unix_ms
    }
    pub const fn updated_at_unix_ms(&self) -> u64 {
        self.updated_at_unix_ms
    }
    pub const fn media_count(&self) -> u64 {
        self.media_count
    }
    pub const fn verified_media_count(&self) -> u64 {
        self.verified_media_count
    }
    pub const fn possible_orphan_count(&self) -> u64 {
        self.possible_orphan_count
    }
    pub const fn settlement(&self) -> Option<OperationSettlement> {
        self.settlement
    }
}

impl From<Phase1DraftStatus> for Phase1DraftSummary {
    fn from(value: Phase1DraftStatus) -> Self {
        Self {
            draft_id: value.draft.draft_id(),
            revision: value.draft.revision(),
            kind: value.kind,
            command_type: value.command_type,
            state: value.state,
            has_form: value.form.is_some(),
            is_revision: value.revision_policy.is_some(),
            created_at_unix_ms: value.draft.created_at_unix_ms(),
            updated_at_unix_ms: value.draft.updated_at_unix_ms(),
            media_count: value.media.len() as u64,
            verified_media_count: value
                .media
                .iter()
                .filter(|media| media.stage == Phase1MediaStage::Verified)
                .count() as u64,
            possible_orphan_count: value
                .media
                .iter()
                .filter(|media| media.orphan.is_some())
                .count() as u64,
            settlement: value.push.as_ref().map(|push| push.settlement()),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase1DraftRepairReason {
    UnsupportedSchema,
    CorruptRecord,
    /// A local operation/status dependency is unavailable. This is not proof
    /// of a corrupt database and must remain retryable as a local read.
    NeedsAttention,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Phase1DraftListEntry {
    Draft(Phase1DraftSummary),
    Repair {
        draft_key: [u8; 16],
        revision: u64,
        reason: Phase1DraftRepairReason,
    },
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Phase1DraftPage {
    author: [u8; 32],
    entries: Vec<Phase1DraftListEntry>,
    next_cursor: Option<String>,
}

impl Phase1DraftPage {
    pub const fn author(&self) -> &[u8; 32] {
        &self.author
    }
    pub fn entries(&self) -> &[Phase1DraftListEntry] {
        &self.entries
    }
    pub fn next_cursor(&self) -> Option<&str> {
        self.next_cursor.as_deref()
    }
}

impl TeraRuntime {
    /// A live stable-ID page of author-bound, unscoped legacy records. Revisit
    /// the first page for insertions behind a cursor; no complete snapshot is implied.
    pub async fn phase1_draft_page(
        &self,
        limit: u16,
        cursor: Option<&str>,
    ) -> Result<Phase1DraftPage, Phase1DraftError> {
        let _command = self.lifecycle.enter()?;
        if limit == 0 || limit > AUTHORED_DRAFT_QUERY_LIMIT_MAX {
            return Err(Phase1DraftError::InvalidInventoryRequest);
        }
        let author = self.draft_author()?;
        let mut query = AuthoredDraftQuery::new(author, DRAFT_PAYLOAD_SCHEMA, None, limit)
            .map_err(|_| Phase1DraftError::InvalidInventoryRequest)?;
        if let Some(cursor) = cursor {
            let captured = query.cursor_after(decode_cursor(author, cursor)?);
            query = query
                .with_cursor(&captured)
                .map_err(|_| Phase1DraftError::InvalidInventoryCursor)?;
        }
        let page = self
            .storage()?
            .query_authored_drafts(query)
            .await
            .map_err(map_draft_storage_error)?;
        let next_cursor = if page.next_cursor().is_some() {
            let last = page.records().last().ok_or(Phase1DraftError::Corrupt)?;
            Some(encode_cursor(author, record_key(last)))
        } else {
            None
        };
        let mut entries = Vec::with_capacity(page.records().len());
        for record in page.into_records() {
            entries.push(self.draft_inventory_entry(record).await);
        }
        Ok(Phase1DraftPage {
            author,
            entries,
            next_cursor,
        })
    }

    async fn draft_inventory_entry(
        &self,
        record: AuthoredDraftQueryRecord,
    ) -> Phase1DraftListEntry {
        let draft_key = record_key(&record);
        let (revision, reason) = match record {
            AuthoredDraftQueryRecord::Corrupt { revision, .. } => {
                (revision.get(), Phase1DraftRepairReason::CorruptRecord)
            }
            AuthoredDraftQueryRecord::Draft(draft) => {
                let revision = draft.revision().get();
                if let Ok(header) = serde_json::from_slice::<SchemaHeader>(draft.payload())
                    && header.schema_version != DRAFT_SCHEMA_VERSION
                {
                    return Phase1DraftListEntry::Repair {
                        draft_key,
                        revision,
                        reason: Phase1DraftRepairReason::UnsupportedSchema,
                    };
                }
                let reason = match self.draft_status_from(draft).await {
                    Ok(status) => return Phase1DraftListEntry::Draft(status.into()),
                    Err(
                        Phase1DraftError::Storage
                        | Phase1DraftError::Operation
                        | Phase1DraftError::OperationUnavailable
                        | Phase1DraftError::OperationInProgress
                        | Phase1DraftError::Lifecycle(_),
                    ) => Phase1DraftRepairReason::NeedsAttention,
                    Err(_) => Phase1DraftRepairReason::CorruptRecord,
                };
                (revision, reason)
            }
        };
        Phase1DraftListEntry::Repair {
            draft_key,
            revision,
            reason,
        }
    }
}

#[derive(Deserialize)]
struct SchemaHeader {
    schema_version: u16,
}

fn record_key(record: &AuthoredDraftQueryRecord) -> [u8; 16] {
    match record {
        AuthoredDraftQueryRecord::Draft(draft) => *draft.draft_id().as_bytes(),
        AuthoredDraftQueryRecord::Corrupt { draft_key, .. } => *draft_key,
    }
}

fn encode_cursor(author: [u8; 32], after: [u8; 16]) -> String {
    format!(
        "{CURSOR_PREFIX}{}{}",
        hex::encode(author),
        hex::encode(after)
    )
}

fn decode_cursor(author: [u8; 32], value: &str) -> Result<[u8; 16], Phase1DraftError> {
    let invalid = || Phase1DraftError::InvalidInventoryCursor;
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
    let mut decoded = [0; 48];
    hex::decode_to_slice(encoded, &mut decoded).map_err(|_| invalid())?;
    if decoded[..32] != author {
        return Err(invalid());
    }
    decoded[32..].try_into().map_err(|_| invalid())
}
