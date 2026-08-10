use radroots_mobile_core::runtime::product_surface::{Phase1DraftError, TodayError};
use thiserror::Error;

use crate::MOBILE_FFI_SCHEMA_VERSION;

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct RadrootsErrorRecord {
    pub schema_version: u16,
    pub code: String,
    pub category: String,
    pub retryable: bool,
    pub recovery_actions: Vec<String>,
    pub operation_id: Option<String>,
    pub capability_id: Option<String>,
    pub safe_message: String,
}

/// The only error envelope exported across the native language boundary.
#[derive(Debug, Error, uniffi::Error)]
pub enum RadrootsAppError {
    #[error("radroots operation failed: {report:?}")]
    Failure { report: RadrootsErrorRecord },
}

impl RadrootsAppError {
    pub(crate) fn initialization(_message: impl Into<String>) -> Self {
        Self::failure(
            "initialization_failed",
            "initialization",
            true,
            &["retry"],
            "The Radroots runtime could not be initialized.",
        )
    }

    pub(crate) fn invalid_argument(code: impl Into<String>) -> Self {
        Self::Failure {
            report: RadrootsErrorRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                code: code.into(),
                category: "validation".to_owned(),
                retryable: false,
                recovery_actions: vec!["correct_input".to_owned()],
                operation_id: None,
                capability_id: None,
                safe_message: "The request is invalid.".to_owned(),
            },
        }
    }

    pub(crate) fn failure(
        code: &str,
        category: &str,
        retryable: bool,
        recovery_actions: &[&str],
        safe_message: &str,
    ) -> Self {
        Self::Failure {
            report: RadrootsErrorRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                code: code.to_owned(),
                category: category.to_owned(),
                retryable,
                recovery_actions: recovery_actions
                    .iter()
                    .map(|action| (*action).to_owned())
                    .collect(),
                operation_id: None,
                capability_id: None,
                safe_message: safe_message.to_owned(),
            },
        }
    }

    pub fn report(&self) -> &RadrootsErrorRecord {
        match self {
            Self::Failure { report } => report,
        }
    }
}

impl From<radroots_mobile_core::RadrootsAppError> for RadrootsAppError {
    fn from(error: radroots_mobile_core::RadrootsAppError) -> Self {
        match error {
            radroots_mobile_core::RadrootsAppError::Sdk { report } => Self::Failure {
                report: RadrootsErrorRecord {
                    schema_version: MOBILE_FFI_SCHEMA_VERSION,
                    code: report.code,
                    category: report.class,
                    retryable: report.retryable,
                    recovery_actions: report.recovery_actions,
                    operation_id: report.operation_id,
                    capability_id: report.capability_id,
                    safe_message: report.message,
                },
            },
            radroots_mobile_core::RadrootsAppError::Store { report } => Self::Failure {
                report: RadrootsErrorRecord {
                    schema_version: MOBILE_FFI_SCHEMA_VERSION,
                    code: report.code,
                    category: report.class,
                    retryable: report.retryable,
                    recovery_actions: report.recovery_actions,
                    operation_id: None,
                    capability_id: None,
                    safe_message: report.message,
                },
            },
            radroots_mobile_core::RadrootsAppError::Initialization(_) => {
                Self::initialization("redacted")
            }
            radroots_mobile_core::RadrootsAppError::Runtime(_) => Self::failure(
                "runtime_failed",
                "runtime",
                true,
                &["retry"],
                "The runtime operation failed.",
            ),
            radroots_mobile_core::RadrootsAppError::Unsupported(_) => Self::failure(
                "unsupported",
                "capability",
                false,
                &[],
                "The requested capability is unsupported.",
            ),
            radroots_mobile_core::RadrootsAppError::Internal(_) => Self::failure(
                "internal_failure",
                "internal",
                false,
                &["restart"],
                "An internal Radroots error occurred.",
            ),
        }
    }
}

impl From<TodayError> for RadrootsAppError {
    fn from(error: TodayError) -> Self {
        let (code, retryable, actions) = match error {
            TodayError::InvalidRequest | TodayError::EventNotVisible => {
                ("today_invalid_request", false, &["correct_input"][..])
            }
            TodayError::ProjectionMissing | TodayError::SnapshotMissing => {
                ("today_refresh_required", true, &["refresh"][..])
            }
            TodayError::CursorPositionMissing | TodayError::Cursor(_) => {
                ("today_cursor_invalid", true, &["restart_pagination"][..])
            }
            TodayError::RuntimeUnavailable => ("today_runtime_unavailable", true, &["retry"][..]),
            TodayError::InboundMedia(_) => ("today_media_invalid", false, &["retry_media"][..]),
            TodayError::CorruptProjection | TodayError::Serialization | TodayError::Storage(_) => {
                ("today_state_failed", true, &["rebuild", "retry"][..])
            }
        };
        Self::failure(
            code,
            "today",
            retryable,
            actions,
            "The Today operation could not be completed.",
        )
    }
}

impl From<Phase1DraftError> for RadrootsAppError {
    fn from(error: Phase1DraftError) -> Self {
        let (code, retryable, actions) = match error {
            Phase1DraftError::IdentityUnavailable => (
                "identity_unavailable",
                true,
                &["unlock_identity", "retry"][..],
            ),
            Phase1DraftError::InvalidDraft => ("draft_invalid", false, &["correct_input"][..]),
            Phase1DraftError::InvalidMedia => {
                ("draft_media_invalid", false, &["replace_media"][..])
            }
            Phase1DraftError::InvalidQueuePolicy => {
                ("draft_queue_policy_invalid", false, &["correct_input"][..])
            }
            Phase1DraftError::RevisionConflict => {
                ("draft_revision_conflict", true, &["refresh"][..])
            }
            Phase1DraftError::NotFound => ("draft_not_found", false, &[][..]),
            Phase1DraftError::Terminal => ("draft_terminal", false, &[][..]),
            Phase1DraftError::MediaNotReady => {
                ("draft_media_not_ready", true, &["retry_media"][..])
            }
            Phase1DraftError::OperationUnavailable => {
                ("authoring_unavailable", true, &["retry"][..])
            }
            Phase1DraftError::Operation | Phase1DraftError::Storage | Phase1DraftError::Overlay => {
                ("authoring_failed", true, &["retry", "inspect_outbox"][..])
            }
            Phase1DraftError::Corrupt => ("draft_corrupt", false, &["recover_draft"][..]),
            Phase1DraftError::ClockUnavailable => {
                ("operation_clock_unavailable", true, &["retry"][..])
            }
            Phase1DraftError::DeadlineOverflow => ("operation_deadline_overflow", false, &[][..]),
            Phase1DraftError::NoWritableRelay => (
                "writable_relay_unavailable",
                true,
                &["configure_relay", "retry"][..],
            ),
            Phase1DraftError::InvalidRevision => {
                ("revision_invalid", false, &["review_revision"][..])
            }
        };
        Self::failure(
            code,
            "authoring",
            retryable,
            actions,
            "The authored operation could not be completed.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_core_messages_are_not_copied_to_the_ffi_record() {
        let error = RadrootsAppError::from(radroots_mobile_core::RadrootsAppError::internal(
            "private/path/secret-value",
        ));
        assert_eq!(error.report().code, "internal_failure");
        assert!(!format!("{error:?}").contains("secret-value"));
        assert!(!error.report().safe_message.contains("secret-value"));
    }

    #[test]
    fn every_core_error_class_maps_to_a_versioned_redacted_record() {
        let sdk = radroots_mobile_core::RadrootsAppError::Sdk {
            report: radroots_mobile_core::SdkErrorRecord {
                schema_version: 1,
                code: "relay_unavailable".to_owned(),
                class: "transport".to_owned(),
                retryable: true,
                recovery_actions: vec!["retry".to_owned()],
                operation_id: Some("operation".to_owned()),
                capability_id: Some("relay".to_owned()),
                message: "Safe relay failure".to_owned(),
            },
        };
        let sdk = RadrootsAppError::from(sdk);
        assert_eq!(sdk.report().code, "relay_unavailable");
        assert_eq!(sdk.report().operation_id.as_deref(), Some("operation"));

        let store = radroots_mobile_core::RadrootsAppError::Store {
            report: radroots_mobile_core::StoreErrorRecord {
                schema_version: 1,
                code: "store_locked".to_owned(),
                class: "storage".to_owned(),
                retryable: true,
                recovery_actions: vec!["unlock".to_owned()],
                message: "Safe store failure".to_owned(),
            },
        };
        let store = RadrootsAppError::from(store);
        assert_eq!(store.report().code, "store_locked");
        assert!(store.report().operation_id.is_none());

        for (core, code, category) in [
            (
                radroots_mobile_core::RadrootsAppError::initialization("secret"),
                "initialization_failed",
                "initialization",
            ),
            (
                radroots_mobile_core::RadrootsAppError::runtime("secret"),
                "runtime_failed",
                "runtime",
            ),
            (
                radroots_mobile_core::RadrootsAppError::unsupported("secret"),
                "unsupported",
                "capability",
            ),
            (
                radroots_mobile_core::RadrootsAppError::internal("secret"),
                "internal_failure",
                "internal",
            ),
        ] {
            let ffi = RadrootsAppError::from(core);
            assert_eq!(ffi.report().code, code);
            assert_eq!(ffi.report().category, category);
            assert!(!ffi.report().safe_message.contains("secret"));
        }
    }

    #[test]
    fn today_and_draft_failures_have_stable_recovery_classes() {
        for error in [
            TodayError::InvalidRequest,
            TodayError::EventNotVisible,
            TodayError::ProjectionMissing,
            TodayError::SnapshotMissing,
            TodayError::CursorPositionMissing,
            TodayError::RuntimeUnavailable,
            TodayError::CorruptProjection,
            TodayError::Serialization,
        ] {
            let ffi = RadrootsAppError::from(error);
            assert_eq!(ffi.report().category, "today");
            assert!(!ffi.report().safe_message.is_empty());
        }

        for error in [
            Phase1DraftError::IdentityUnavailable,
            Phase1DraftError::InvalidDraft,
            Phase1DraftError::InvalidMedia,
            Phase1DraftError::InvalidQueuePolicy,
            Phase1DraftError::RevisionConflict,
            Phase1DraftError::NotFound,
            Phase1DraftError::Terminal,
            Phase1DraftError::MediaNotReady,
            Phase1DraftError::OperationUnavailable,
            Phase1DraftError::Operation,
            Phase1DraftError::Storage,
            Phase1DraftError::Overlay,
            Phase1DraftError::Corrupt,
        ] {
            let ffi = RadrootsAppError::from(error);
            assert_eq!(ffi.report().category, "authoring");
            assert!(!ffi.report().safe_message.is_empty());
        }
    }
}
