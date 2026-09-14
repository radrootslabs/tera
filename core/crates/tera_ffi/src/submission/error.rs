use crate::TeraAppError;
use tera_core::runtime::product_surface::{
    SubmissionCaptureError, SubmissionCommitError, SubmissionOperationError,
};

impl From<SubmissionCommitError> for TeraAppError {
    fn from(error: SubmissionCommitError) -> Self {
        use SubmissionCaptureError as C;
        use SubmissionCommitError as E;
        let (code, retryable, actions): (_, _, &[&str]) = match error {
            E::Reservation(error) | E::Capture(C::Reservation(error)) => return error.into(),
            E::Capture(C::InvalidInput(code)) => return Self::invalid_argument(code),
            E::Capture(C::PolicyUnavailable) => (
                "submission_policy_unavailable",
                false,
                &["review_publication_settings", "retry_same_command"],
            ),
            E::IdempotencyConflict => {
                ("idempotency_conflict", false, &["restore_original_request"])
            }
            E::RevisionConflict => (
                "composer_revision_conflict",
                false,
                &["preserve_local_work", "recover_original_submission"],
            ),
            E::InvalidIntent => return Self::invalid_argument("submission_intent_invalid"),
            E::CorruptRecord => (
                "submission_record_corrupt",
                false,
                &["preserve_local_work", "inspect_local_stores"],
            ),
            E::UnsupportedSchema => (
                "submission_schema_unsupported",
                false,
                &["preserve_local_work", "update_app"],
            ),
            E::InvalidReceipt => (
                "submission_receipt_mismatch",
                true,
                &["recover_original_submission"],
            ),
            E::Storage(_) => (
                "submission_storage_failed",
                true,
                &["inspect_local_stores", "recover_original_submission"],
            ),
        };
        Self::failure(
            code,
            "submission",
            retryable,
            actions,
            "The local submission could not be confirmed. Preserve the original request for recovery.",
        )
    }
}

impl From<SubmissionOperationError> for TeraAppError {
    fn from(error: SubmissionOperationError) -> Self {
        use SubmissionOperationError as E;
        let (code, retryable, actions): (_, _, &[&str]) = match error {
            E::Submission(error) => return error.into(),
            E::Operation(error) => return error.into(),
            E::NotFound => (
                "submission_not_found",
                false,
                &["recover_original_submission"],
            ),
            E::PrerequisitesPending => (
                "submission_prerequisites_pending",
                false,
                &["complete_submission_media"],
            ),
            E::Stopped => (
                "submission_stopped",
                false,
                &["recover_original_submission"],
            ),
            E::Corrupt => (
                "submission_record_corrupt",
                false,
                &["preserve_local_work", "inspect_local_stores"],
            ),
            E::InvalidMedia => return Self::invalid_argument("submission_media_invalid"),
            E::MediaPolicyChanged => (
                "submission_media_policy_changed",
                false,
                &["review_original_media_policy"],
            ),
        };
        Self::failure(
            code,
            "submission",
            retryable,
            actions,
            "The saved submission needs attention. Its original operation remains the recovery authority.",
        )
    }
}
