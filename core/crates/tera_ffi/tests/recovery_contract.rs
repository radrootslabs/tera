use tera_core::runtime::product_surface::{Phase1DraftError, Phase1InboundMediaError, TodayError};
use tera_ffi::{
    FfiRecoveryDisposition as Recovery, FfiRetryDisposition as Retry, TeraAppError,
    classify_error_recovery,
};

#[test]
fn stable_codes_distinguish_recovery_without_granting_a_new_publication() {
    for (code, disposition, retry) in [
        (
            "unknown_future_retryable_network",
            Recovery::Unknown,
            Retry::NotAllowed,
        ),
        (
            "invalid_argument",
            Recovery::InvalidInput,
            Retry::NotAllowed,
        ),
        (
            "draft_revision_conflict",
            Recovery::StaleRevision,
            Retry::AfterRecovery,
        ),
        (
            "idempotency_conflict",
            Recovery::IdempotencyConflict,
            Retry::NotAllowed,
        ),
        (
            "today_cursor_invalid",
            Recovery::StaleCursor,
            Retry::AfterRecovery,
        ),
        (
            "protected_data_unavailable",
            Recovery::ProtectedDataUnavailable,
            Retry::AfterRecovery,
        ),
        (
            "identity_unavailable",
            Recovery::IdentityUnavailable,
            Retry::AfterRecovery,
        ),
        (
            "database_busy",
            Recovery::StorageFailure,
            Retry::AfterRecovery,
        ),
        (
            "storage_space_insufficient",
            Recovery::QuotaExhausted,
            Retry::AfterRecovery,
        ),
        (
            "cancelled_no_commit",
            Recovery::CancelledBeforeEffect,
            Retry::NotAllowed,
        ),
        (
            "ios.runtime.cancelled",
            Recovery::OutcomeUnknown,
            Retry::ReconcileExistingOperation,
        ),
        (
            "deadline_exceeded",
            Recovery::OutcomeUnknown,
            Retry::ReconcileExistingOperation,
        ),
        (
            "local_committed_delivery_pending",
            Recovery::OutcomeUnknown,
            Retry::ReconcileExistingOperation,
        ),
        (
            "schema_too_new",
            Recovery::UnsupportedVersion,
            Retry::NotAllowed,
        ),
        (
            "blossom_resolution_failed",
            Recovery::NetworkUnavailable,
            Retry::AfterRecovery,
        ),
        (
            "relay_rate_limited",
            Recovery::NetworkPolicy,
            Retry::AfterRecovery,
        ),
        (
            "today_relay_partial",
            Recovery::PartialResult,
            Retry::AfterRecovery,
        ),
        (
            "today_media_corrupt",
            Recovery::MediaCorrupt,
            Retry::NotAllowed,
        ),
        (
            "client_closed",
            Recovery::RuntimeUnavailable,
            Retry::NotAllowed,
        ),
    ] {
        let decision = classify_error_recovery(1, code.into());
        assert_eq!(decision.disposition, disposition, "{code}");
        assert_eq!(decision.retry, retry, "{code}");
    }
    for version in [0, 2, u16::MAX] {
        let decision = classify_error_recovery(version, "today_relay_offline".into());
        assert_eq!(decision.disposition, Recovery::UnsupportedVersion);
        assert_eq!(decision.retry, Retry::NotAllowed);
    }
}

#[test]
fn real_application_error_adapters_preserve_storage_quota_and_corruption() {
    for (error, expected) in [
        (
            TodayError::InboundMedia(Phase1InboundMediaError::CacheQuotaExceeded),
            Recovery::QuotaExhausted,
        ),
        (
            TodayError::InboundMedia(Phase1InboundMediaError::CacheIo),
            Recovery::StorageFailure,
        ),
        (
            TodayError::InboundMedia(Phase1InboundMediaError::UnsupportedSchema),
            Recovery::UnsupportedVersion,
        ),
        (
            TodayError::InboundMedia(Phase1InboundMediaError::CorruptArtifact),
            Recovery::MediaCorrupt,
        ),
        (TodayError::CorruptProjection, Recovery::StaleRevision),
    ] {
        let error = TeraAppError::from(error);
        let report = error.report();
        assert_eq!(
            classify_error_recovery(report.schema_version, report.code.clone()).disposition,
            expected
        );
    }
    for (error, expected) in [
        (Phase1DraftError::Storage, Recovery::StorageFailure),
        (
            Phase1DraftError::IdentityUnavailable,
            Recovery::IdentityUnavailable,
        ),
        (Phase1DraftError::Operation, Recovery::OutcomeUnknown),
        (Phase1DraftError::Overlay, Recovery::StaleRevision),
    ] {
        let error = TeraAppError::from(error);
        assert_eq!(
            classify_error_recovery(1, error.report().code.clone()).disposition,
            expected
        );
    }
}

#[test]
fn complete_safe_envelopes_survive_mapping_without_relabeling_future_versions() {
    let sdk = tera_core::SdkErrorRecord {
        schema_version: 2,
        code: "future_network_code".into(),
        class: "network".into(),
        retryable: true,
        recovery_actions: vec!["publish_again".into()],
        operation_id: Some("existing-operation".into()),
        capability_id: Some("transport".into()),
        message: "Texte localisé: retry corruption offline".into(),
    };
    let error = TeraAppError::from(tera_core::TeraAppError::Sdk {
        report: sdk.clone(),
    });
    let report = error.report();
    assert_eq!(report.schema_version, sdk.schema_version);
    assert_eq!(report.code, sdk.code);
    assert_eq!(report.category, sdk.class);
    assert_eq!(report.retryable, sdk.retryable);
    assert_eq!(report.recovery_actions, sdk.recovery_actions);
    assert_eq!(report.operation_id, sdk.operation_id);
    assert_eq!(report.capability_id, sdk.capability_id);
    assert_eq!(report.safe_message, sdk.message);
    let decision = classify_error_recovery(report.schema_version, report.code.clone());
    assert_eq!(decision.disposition, Recovery::UnsupportedVersion);
    assert_eq!(decision.retry, Retry::NotAllowed);

    let store = tera_core::StoreErrorRecord {
        schema_version: 2,
        code: sdk.code,
        class: sdk.class,
        retryable: sdk.retryable,
        recovery_actions: sdk.recovery_actions,
        message: sdk.message,
    };
    let error = TeraAppError::from(tera_core::TeraAppError::Store {
        report: store.clone(),
    });
    let report = error.report();
    assert_eq!(report.schema_version, store.schema_version);
    assert_eq!(report.code, store.code);
    assert_eq!(report.category, store.class);
    assert_eq!(report.retryable, store.retryable);
    assert_eq!(report.recovery_actions, store.recovery_actions);
    assert_eq!(report.safe_message, store.message);
    assert_eq!(report.operation_id, None);
    assert_eq!(report.capability_id, None);
}
