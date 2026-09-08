use tera_core::runtime::product_surface::{
    Phase1DraftError, ProfileMetadataError, SettingsError, TodayError,
};
use thiserror::Error;

use crate::MOBILE_FFI_SCHEMA_VERSION;

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct TeraErrorRecord {
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
pub enum TeraAppError {
    #[error("radroots operation failed: {report:?}")]
    Failure { report: TeraErrorRecord },
}

impl TeraAppError {
    pub(crate) fn initialization(_message: impl Into<String>) -> Self {
        Self::failure(
            "initialization_failed",
            "initialization",
            true,
            &["retry"],
            "The Tera runtime could not be initialized.",
        )
    }

    pub(crate) fn invalid_argument(code: impl Into<String>) -> Self {
        Self::Failure {
            report: TeraErrorRecord {
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
            report: TeraErrorRecord {
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

    pub fn report(&self) -> &TeraErrorRecord {
        match self {
            Self::Failure { report } => report,
        }
    }

    pub(crate) fn with_operation_id(mut self, operation_id: String) -> Self {
        match &mut self {
            Self::Failure { report } => report.operation_id = Some(operation_id),
        }
        self
    }
}

impl From<tera_core::TeraAppError> for TeraAppError {
    fn from(error: tera_core::TeraAppError) -> Self {
        match error {
            tera_core::TeraAppError::Sdk { report } => Self::Failure {
                report: TeraErrorRecord {
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
            tera_core::TeraAppError::Store { report } => Self::Failure {
                report: TeraErrorRecord {
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
            tera_core::TeraAppError::Initialization(_) => Self::initialization("redacted"),
            tera_core::TeraAppError::Runtime(_) => Self::failure(
                "runtime_failed",
                "runtime",
                true,
                &["retry"],
                "The runtime operation failed.",
            ),
            tera_core::TeraAppError::Unsupported(_) => Self::failure(
                "unsupported",
                "capability",
                false,
                &[],
                "The requested capability is unsupported.",
            ),
            tera_core::TeraAppError::Internal(_) => Self::failure(
                "internal_failure",
                "internal",
                false,
                &["restart"],
                "An internal Tera error occurred.",
            ),
        }
    }
}

impl From<TodayError> for TeraAppError {
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
            TodayError::InboundRetrieval(error) => {
                if error.retryable() {
                    ("today_media_retrieval_failed", true, &["retry_media"][..])
                } else {
                    ("today_media_retrieval_failed", false, &["review_media"][..])
                }
            }
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

impl From<Phase1DraftError> for TeraAppError {
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

impl From<SettingsError> for TeraAppError {
    fn from(error: SettingsError) -> Self {
        let retryable = matches!(
            error,
            SettingsError::RevisionConflict
                | SettingsError::RevisionExhausted
                | SettingsError::Storage
        );
        let actions = if matches!(error, SettingsError::RevisionConflict) {
            &["refresh"] as &[&str]
        } else if retryable {
            &["retry"] as &[&str]
        } else {
            &["correct_input"] as &[&str]
        };
        Self::failure(
            error.code(),
            "settings",
            retryable,
            actions,
            "The settings operation could not be completed.",
        )
    }
}

impl From<ProfileMetadataError> for TeraAppError {
    fn from(error: ProfileMetadataError) -> Self {
        Self::failure(
            error.code(),
            "profile",
            false,
            &["correct_input"],
            "The profile metadata is invalid.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn internal_core_messages_are_not_copied_to_the_ffi_record() {
        let error = TeraAppError::from(tera_core::TeraAppError::internal(
            "private/path/secret-value",
        ));
        assert_eq!(error.report().code, "internal_failure");
        assert!(!format!("{error:?}").contains("secret-value"));
        assert!(!error.report().safe_message.contains("secret-value"));
    }

    #[test]
    fn every_core_error_class_maps_to_a_versioned_redacted_record() {
        let sdk = tera_core::TeraAppError::Sdk {
            report: tera_core::SdkErrorRecord {
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
        let sdk = TeraAppError::from(sdk);
        assert_eq!(sdk.report().code, "relay_unavailable");
        assert_eq!(sdk.report().operation_id.as_deref(), Some("operation"));

        let store = tera_core::TeraAppError::Store {
            report: tera_core::StoreErrorRecord {
                schema_version: 1,
                code: "store_locked".to_owned(),
                class: "storage".to_owned(),
                retryable: true,
                recovery_actions: vec!["unlock".to_owned()],
                message: "Safe store failure".to_owned(),
            },
        };
        let store = TeraAppError::from(store);
        assert_eq!(store.report().code, "store_locked");
        assert!(store.report().operation_id.is_none());

        for (core, code, category) in [
            (
                tera_core::TeraAppError::initialization("secret"),
                "initialization_failed",
                "initialization",
            ),
            (
                tera_core::TeraAppError::runtime("secret"),
                "runtime_failed",
                "runtime",
            ),
            (
                tera_core::TeraAppError::unsupported("secret"),
                "unsupported",
                "capability",
            ),
            (
                tera_core::TeraAppError::internal("secret"),
                "internal_failure",
                "internal",
            ),
        ] {
            let ffi = TeraAppError::from(core);
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
            let ffi = TeraAppError::from(error);
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
            Phase1DraftError::ClockUnavailable,
            Phase1DraftError::DeadlineOverflow,
            Phase1DraftError::NoWritableRelay,
            Phase1DraftError::InvalidRevision,
        ] {
            let ffi = TeraAppError::from(error);
            assert_eq!(ffi.report().category, "authoring");
            assert!(!ffi.report().safe_message.is_empty());
        }
    }

    #[test]
    fn cursor_media_settings_and_profile_failures_cover_every_stable_class() {
        use tera_core::runtime::product_surface::{
            CursorError, IdentitySettingsError, Phase1InboundMediaError,
        };

        for cursor in [
            CursorError::InvalidContext,
            CursorError::Malformed,
            CursorError::Integrity,
            CursorError::Version,
            CursorError::ContextMismatch,
            CursorError::SnapshotMismatch,
            CursorError::Stale,
            CursorError::InvalidPosition,
        ] {
            let error = TeraAppError::from(TodayError::Cursor(cursor));
            assert_eq!(error.report().code, "today_cursor_invalid");
        }
        for media in [
            Phase1InboundMediaError::InvalidReference,
            Phase1InboundMediaError::InvalidDigest,
            Phase1InboundMediaError::MissingDigest,
            Phase1InboundMediaError::InvalidMediaType,
            Phase1InboundMediaError::InvalidDimensions,
            Phase1InboundMediaError::InvalidByteSize,
            Phase1InboundMediaError::InvalidAlt,
            Phase1InboundMediaError::MetadataMismatch,
            Phase1InboundMediaError::InvalidOperation,
            Phase1InboundMediaError::OperationMismatch,
            Phase1InboundMediaError::InvalidFailure,
            Phase1InboundMediaError::InvalidConfiguration,
            Phase1InboundMediaError::ConfigurationMismatch,
            Phase1InboundMediaError::InvalidVerificationTime,
            Phase1InboundMediaError::InvalidCachePolicy,
            Phase1InboundMediaError::InvalidCacheObservation,
            Phase1InboundMediaError::CacheQuotaExceeded,
            Phase1InboundMediaError::ArtifactCollision,
            Phase1InboundMediaError::CorruptReceipt,
            Phase1InboundMediaError::CorruptState,
            Phase1InboundMediaError::UnsupportedSchema,
            Phase1InboundMediaError::CacheUnavailable,
            Phase1InboundMediaError::CacheIo,
            Phase1InboundMediaError::CorruptArtifact,
        ] {
            let error = TeraAppError::from(TodayError::InboundMedia(media));
            assert_eq!(error.report().code, "today_media_invalid");
        }
        for settings in [
            SettingsError::UnknownRelayAccess,
            SettingsError::InvalidRelayEndpoint,
            SettingsError::InvalidRelayEndpointCount,
            SettingsError::DuplicateRelayEndpoint,
            SettingsError::InvalidBlossomEndpoint,
            SettingsError::InvalidBlossomEndpointCount,
            SettingsError::NetworkEnvironmentMismatch,
            SettingsError::InvalidMediaCacheBytes,
            SettingsError::InvalidMediaCacheArtifacts,
            SettingsError::RevisionConflict,
            SettingsError::RevisionExhausted,
            SettingsError::UnsupportedSchema,
            SettingsError::CorruptDocument,
            SettingsError::Storage,
            SettingsError::Identity(IdentitySettingsError::InvalidIdentityId),
        ] {
            let expected_retryable = matches!(
                settings,
                SettingsError::RevisionConflict
                    | SettingsError::RevisionExhausted
                    | SettingsError::Storage
            );
            let error = TeraAppError::from(settings);
            assert_eq!(error.report().retryable, expected_retryable);
        }
        for profile in [
            ProfileMetadataError::InvalidName,
            ProfileMetadataError::InvalidDisplayName,
            ProfileMetadataError::InvalidAbout,
            ProfileMetadataError::InvalidNip05,
        ] {
            assert_eq!(TeraAppError::from(profile).report().category, "profile");
        }
        assert_eq!(
            TeraAppError::initialization("private").report().code,
            "initialization_failed"
        );
        assert_eq!(
            TeraAppError::invalid_argument("bad")
                .with_operation_id("operation".to_owned())
                .report()
                .operation_id
                .as_deref(),
            Some("operation")
        );
    }
}
