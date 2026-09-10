//! Bounded local editing values. These types carry no signing or delivery authority.

mod form;
mod identity;
mod repository;
mod storage;

pub use form::{
    COMPOSER_CONTENT_MAX_BYTES, COMPOSER_FORM_MAX_BYTES, COMPOSER_MEDIA_MAX,
    COMPOSER_TEXT_MAX_BYTES, ComposerFormInput, ComposerMediaInput, ComposerPartialForm,
};
pub use identity::{ComposerEditSequence, ComposerId, ComposerRevision, ComposerScope};
pub use repository::{ComposerPersistenceError, ComposerSaveReceipt};
pub use storage::{
    COMPOSER_PAYLOAD_SCHEMA, COMPOSER_SCHEMA_SHA256, COMPOSER_SCHEMA_VERSION, ComposerStorageError,
    ComposerStorageRecord,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ComposerError {
    #[error("invalid composer identity")]
    InvalidIdentity,
    #[error("invalid composer revision")]
    InvalidRevision,
    #[error("invalid composer edit sequence")]
    InvalidEditSequence,
    #[error("composer form exceeds its bounds or has invalid metadata")]
    InvalidForm,
    #[error("unsupported composer form representation")]
    InvalidRepresentation,
}

/// A local editing revision, distinct from a strict authored plan or sealed intent.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ComposerDraft {
    id: ComposerId,
    revision: ComposerRevision,
    scope: ComposerScope,
    edit_sequence: ComposerEditSequence,
    form: ComposerPartialForm,
}

impl ComposerDraft {
    pub fn new(
        id: ComposerId,
        revision: ComposerRevision,
        scope: ComposerScope,
        edit_sequence: ComposerEditSequence,
        form: ComposerPartialForm,
    ) -> Self {
        Self {
            id,
            revision,
            scope,
            edit_sequence,
            form,
        }
    }

    pub const fn id(&self) -> ComposerId {
        self.id
    }
    pub const fn revision(&self) -> ComposerRevision {
        self.revision
    }
    pub fn scope(&self) -> &ComposerScope {
        &self.scope
    }
    pub const fn edit_sequence(&self) -> ComposerEditSequence {
        self.edit_sequence
    }
    pub fn form(&self) -> &ComposerPartialForm {
        &self.form
    }
}

#[cfg(test)]
mod tests;
