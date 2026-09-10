//! Versioned application payloads carried by the existing shared storage owner.

use radroots_storage::{
    authored_draft::{AuthoredDraft, AuthoredDraftId, AuthoredDraftStage},
    authored_draft_query::AuthoredDraftScope,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{
    COMPOSER_FORM_MAX_BYTES, ComposerDraft, ComposerEditSequence, ComposerId, ComposerPartialForm,
    ComposerRevision, ComposerScope,
};

pub const COMPOSER_PAYLOAD_SCHEMA: &str = "tera.composer.v1";
pub const COMPOSER_SCHEMA_VERSION: u64 = 1;
/// Pins the exact standalone schema descriptor, including field and resource bounds.
pub const COMPOSER_SCHEMA_SHA256: &str =
    "48ac3978641bb1485e9a4c7037533a5eefb1ad176e0eb43ead800d7e12c1f084";
const SCOPE_DOMAIN: &[u8] = b"tera.composer-scope.v1\0";

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ComposerStorageError {
    #[error("record is not a composer")]
    WrongSchema,
    #[error("composer belongs to a different account or context")]
    ScopeMismatch,
    #[error("composer schema is unsupported")]
    UnsupportedSchema,
    #[error("composer record requires repair")]
    CorruptRecord,
    #[error("composer timestamp is not representable")]
    InvalidTimestamp,
    #[error("composer storage representation is invalid")]
    InvalidRecord,
}

/// Probe identity before interpreting a future body. Complete current decoding below
/// still rejects unknown and duplicate fields, including nested form/media fields.
#[derive(Deserialize)]
struct SchemaHeader {
    schema_version: u64,
    schema_sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredComposerV1 {
    schema_version: u64,
    schema_sha256: String,
    scope: ComposerScope,
    edit_sequence: ComposerEditSequence,
    form: ComposerPartialForm,
}

/// A validated application/storage mapping. Construction is not a durable save receipt.
#[derive(Clone)]
pub struct ComposerStorageRecord {
    stored: AuthoredDraft,
    draft: ComposerDraft,
}

impl std::fmt::Debug for ComposerStorageRecord {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ComposerStorageRecord")
            .field("draft", &self.draft)
            .finish_non_exhaustive()
    }
}

impl ComposerStorageRecord {
    /// Creates an initial scoped envelope for the existing owner to commit.
    pub fn initial(
        id: ComposerId,
        scope: ComposerScope,
        edit_sequence: ComposerEditSequence,
        form: ComposerPartialForm,
        created_at_unix_ms: u64,
    ) -> Result<Self, ComposerStorageError> {
        validate_time(created_at_unix_ms)?;
        let stored_scope = Self::scope_digest(&scope)?;
        let (payload, form) = encode(scope.clone(), edit_sequence, form)?;
        let stored = AuthoredDraft::initial(
            AuthoredDraftId::new(*id.as_bytes())
                .map_err(|_| ComposerStorageError::InvalidRecord)?,
            scope.author().into_bytes(),
            COMPOSER_PAYLOAD_SCHEMA,
            payload,
            AuthoredDraftStage::Draft,
            None,
            created_at_unix_ms,
        )
        .and_then(|draft| draft.with_scope(stored_scope))
        .map_err(|_| ComposerStorageError::InvalidRecord)?;
        Ok(Self {
            stored,
            draft: ComposerDraft::new(id, ComposerRevision::INITIAL, scope, edit_sequence, form),
        })
    }

    /// Validates existing bytes for an independently supplied stable scope without writes.
    pub fn decode(
        stored: AuthoredDraft,
        scope: &ComposerScope,
    ) -> Result<Self, ComposerStorageError> {
        if stored.payload_schema() != COMPOSER_PAYLOAD_SCHEMA {
            return Err(ComposerStorageError::WrongSchema);
        }
        if stored.author() != scope.author().as_bytes()
            || stored.scope() != Some(Self::scope_digest(scope)?)
        {
            return Err(ComposerStorageError::ScopeMismatch);
        }
        stored
            .validate()
            .map_err(|_| ComposerStorageError::CorruptRecord)?;
        if stored.stage() != AuthoredDraftStage::Draft
            || stored.operation_id().is_some()
            || stored.payload().len() > COMPOSER_FORM_MAX_BYTES
        {
            return Err(ComposerStorageError::CorruptRecord);
        }
        validate_time(stored.created_at_unix_ms())?;
        validate_time(stored.updated_at_unix_ms())?;
        let header: SchemaHeader = serde_json::from_slice(stored.payload())
            .map_err(|_| ComposerStorageError::CorruptRecord)?;
        if header.schema_version != COMPOSER_SCHEMA_VERSION
            || header.schema_sha256 != COMPOSER_SCHEMA_SHA256
        {
            return Err(ComposerStorageError::UnsupportedSchema);
        }
        let wire: StoredComposerV1 = serde_json::from_slice(stored.payload())
            .map_err(|_| ComposerStorageError::CorruptRecord)?;
        if &wire.scope != scope {
            return Err(ComposerStorageError::CorruptRecord);
        }
        let draft = ComposerDraft::new(
            ComposerId::new(*stored.draft_id().as_bytes())
                .map_err(|_| ComposerStorageError::CorruptRecord)?,
            ComposerRevision::new(stored.revision().get())
                .map_err(|_| ComposerStorageError::CorruptRecord)?,
            wire.scope,
            wire.edit_sequence,
            wire.form,
        );
        Ok(Self { stored, draft })
    }

    pub fn draft(&self) -> &ComposerDraft {
        &self.draft
    }
    pub fn stored(&self) -> &AuthoredDraft {
        &self.stored
    }
    pub fn into_stored(self) -> AuthoredDraft {
        self.stored
    }

    pub(super) fn successor(
        &self,
        edit_sequence: ComposerEditSequence,
        form: ComposerPartialForm,
        persisted_at_unix_ms: u64,
    ) -> Result<Self, ComposerStorageError> {
        validate_time(persisted_at_unix_ms)?;
        let revision = self
            .draft
            .revision()
            .next()
            .map_err(|_| ComposerStorageError::InvalidRecord)?;
        if edit_sequence <= self.draft.edit_sequence() {
            return Err(ComposerStorageError::InvalidRecord);
        }
        let scope = self.draft.scope().clone();
        let (payload, form) = encode(scope.clone(), edit_sequence, form)?;
        let stored = self
            .stored
            .successor(
                payload,
                AuthoredDraftStage::Draft,
                None,
                persisted_at_unix_ms.max(self.stored.updated_at_unix_ms()),
            )
            .map_err(|_| ComposerStorageError::InvalidRecord)?;
        Ok(Self {
            stored,
            draft: ComposerDraft::new(self.draft.id(), revision, scope, edit_sequence, form),
        })
    }

    /// Stable namespace, independent of process/session generation and relay configuration.
    pub fn scope_digest(scope: &ComposerScope) -> Result<AuthoredDraftScope, ComposerStorageError> {
        let context = scope.local_network().as_bytes();
        let length =
            u32::try_from(context.len()).map_err(|_| ComposerStorageError::InvalidRecord)?;
        let mut digest = Sha256::new();
        digest.update(SCOPE_DOMAIN);
        digest.update(scope.author().as_bytes());
        digest.update(length.to_be_bytes());
        digest.update(context);
        AuthoredDraftScope::new(digest.finalize().into())
            .map_err(|_| ComposerStorageError::InvalidRecord)
    }
}

fn encode(
    scope: ComposerScope,
    edit_sequence: ComposerEditSequence,
    form: ComposerPartialForm,
) -> Result<(Vec<u8>, ComposerPartialForm), ComposerStorageError> {
    let wire = StoredComposerV1 {
        schema_version: COMPOSER_SCHEMA_VERSION,
        schema_sha256: COMPOSER_SCHEMA_SHA256.to_owned(),
        scope,
        edit_sequence,
        form,
    };
    let payload = serde_json::to_vec(&wire).map_err(|_| ComposerStorageError::InvalidRecord)?;
    if payload.len() > COMPOSER_FORM_MAX_BYTES {
        return Err(ComposerStorageError::InvalidRecord);
    }
    Ok((payload, wire.form))
}

fn validate_time(value: u64) -> Result<(), ComposerStorageError> {
    if value == 0 || value > i64::MAX as u64 {
        return Err(ComposerStorageError::InvalidTimestamp);
    }
    Ok(())
}

#[cfg(test)]
#[path = "storage_tests.rs"]
mod tests;

#[cfg(all(test, not(target_arch = "wasm32")))]
#[path = "storage_sqlite_tests.rs"]
mod sqlite_tests;
