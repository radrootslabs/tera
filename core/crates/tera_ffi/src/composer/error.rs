use crate::TeraAppError;
use tera_core::runtime::product_surface::{
    ComposerError, ComposerPersistenceError, ComposerStorageError,
};

impl From<ComposerError> for TeraAppError {
    fn from(error: ComposerError) -> Self {
        let code = match error {
            ComposerError::InvalidIdentity => "composer_id_invalid",
            ComposerError::InvalidRevision => "composer_revision_invalid",
            ComposerError::InvalidEditSequence => "composer_edit_sequence_invalid",
            ComposerError::InvalidForm | ComposerError::InvalidRepresentation => {
                "composer_form_invalid"
            }
        };
        Self::invalid_argument(code)
    }
}

impl From<ComposerPersistenceError> for TeraAppError {
    fn from(error: ComposerPersistenceError) -> Self {
        use ComposerPersistenceError as Error;
        let (code, retryable, actions): (_, _, &[&str]) = match error {
            Error::Lifecycle(error) => return tera_core::TeraAppError::from(error).into(),
            Error::OwnerUnavailable => ("composer_owner_unavailable", true, &["unlock_identity"]),
            Error::ScopeMismatch | Error::Record(ComposerStorageError::ScopeMismatch) => (
                "composer_scope_mismatch",
                false,
                &["select_matching_account_context"],
            ),
            Error::NotFound => ("composer_not_found", false, &["refresh_composers"]),
            Error::RevisionConflict => ("composer_revision_conflict", true, &["reload_composer"]),
            Error::EditSequenceConflict => (
                "composer_edit_sequence_conflict",
                true,
                &["reload_composer"],
            ),
            Error::RevisionOverflow => (
                "composer_revision_exhausted",
                false,
                &["preserve_local_work"],
            ),
            Error::InvalidReceipt => ("composer_receipt_mismatch", true, &["reconcile_composer"]),
            Error::InvalidListRequest => ("composer_list_invalid", false, &["correct_input"]),
            Error::InvalidCursor => ("composer_cursor_invalid", true, &["refresh_composers"]),
            Error::Record(ComposerStorageError::UnsupportedSchema) => (
                "composer_schema_unsupported",
                false,
                &["preserve_local_work", "update_app"],
            ),
            Error::Record(_) => ("composer_record_corrupt", false, &["repair_composer"]),
            Error::Storage(_) => (
                "composer_storage_failed",
                true,
                &["inspect_local_stores", "reload_composer"],
            ),
        };
        Self::failure(
            code,
            "composer",
            retryable,
            actions,
            "The local composer operation could not be completed.",
        )
    }
}
