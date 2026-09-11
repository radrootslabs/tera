use tera_core::runtime::product_surface::{AddCommandType, ComposerFormInput, ComposerPartialForm};
use tera_ffi::*;
mod support;

fn save_request() -> FfiComposerSaveRequest {
    let mut input = ComposerFormInput::empty(AddCommandType::CreateEvent);
    input.content = "PRIVATE incomplete\n\0é".into();
    input.event_start_date = Some("2026-09-".into());
    FfiComposerSaveRequest {
        schema_version: 1,
        scope: FfiComposerScopeRecord {
            schema_version: 1,
            author_public_key: support::PUBLIC_KEY.into(),
            local_network_id: "nearby".into(),
        },
        id: composer_reserve_id().unwrap().id,
        expected_revision: None,
        edit_sequence: u64::MAX - 1,
        form: (&ComposerPartialForm::new(input).unwrap()).into(),
    }
}

fn submission(saved: &FfiComposerSaveRequest) -> FfiSubmissionReservationRequest {
    FfiSubmissionReservationRequest {
        schema_version: 1,
        command_id: submission_reserve_id().unwrap().id,
        scope: saved.scope.clone(),
        composer_id: saved.id.clone(),
        expected_revision: 1,
    }
}

#[tokio::test]
async fn typed_reservation_replays_exact_historical_input_and_keeps_intentional_posts_distinct() {
    let (root, runtime) = support::runtime().await;
    let mut saved = save_request();
    let original = runtime.composer_save(saved.clone()).await.unwrap();
    let request = submission(&saved);
    let (a, b) = tokio::join!(
        runtime.submission_reserve(request.clone()),
        runtime.submission_reserve(request.clone())
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.captured, original.draft);
    assert_eq!(a.reservation_id, b.reservation_id);
    assert_ne!(a.replayed, b.replayed);
    assert!(!format!("{a:?}").contains("PRIVATE"));
    saved.expected_revision = Some(1);
    saved.edit_sequence = u64::MAX;
    saved.form.content = "later".into();
    let latest = runtime.composer_save(saved.clone()).await.unwrap();
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = TeraRuntime::new(
        root.path().to_string_lossy().into_owned(),
        support::PUBLIC_KEY.into(),
        support::GENERATION.into(),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .await
    .unwrap();
    let recovered = runtime.submission_reserve(request.clone()).await.unwrap();
    assert!(recovered.replayed);
    assert_eq!(recovered.captured, original.draft);
    assert_eq!(recovered.reserved_at_unix_ms, a.reserved_at_unix_ms);
    assert_eq!(
        runtime
            .composer_load(saved.scope.clone(), saved.id.clone())
            .await
            .unwrap(),
        latest.draft
    );
    let mut changed = request.clone();
    changed.expected_revision = 2;
    let failure = runtime
        .submission_reserve(changed.clone())
        .await
        .unwrap_err();
    assert_eq!(failure.report().code, "idempotency_conflict");
    assert_eq!(
        classify_error_recovery(1, failure.report().code.clone()).disposition,
        FfiRecoveryDisposition::IdempotencyConflict
    );
    changed.command_id = submission_reserve_id().unwrap().id;
    let second = runtime.submission_reserve(changed.clone()).await.unwrap();
    changed.command_id = submission_reserve_id().unwrap().id;
    let third = runtime.submission_reserve(changed).await.unwrap();
    assert_eq!(second.captured, third.captured);
    assert_ne!(second.reservation_id, third.reservation_id);
    assert!(runtime.phase1_draft_heads(100).await.unwrap().is_empty());
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_submission_identity_versions_and_widths_fail_before_reservation() {
    let (_root, runtime) = support::runtime().await;
    let saved = save_request();
    runtime.composer_save(saved.clone()).await.unwrap();
    let request = submission(&saved);
    let mut invalid = Vec::new();
    for command_id in [
        "00".repeat(16),
        "AB".repeat(16),
        "a".repeat(31),
        "a".repeat(33),
        "z".repeat(32),
    ] {
        let mut changed = request.clone();
        changed.command_id = command_id;
        invalid.push((changed, "submission_command_id_invalid"));
    }
    for expected_revision in [0, u64::MAX] {
        let mut changed = request.clone();
        changed.expected_revision = expected_revision;
        invalid.push((changed, "composer_revision_invalid"));
    }
    let mut changed = request.clone();
    changed.schema_version = 2;
    invalid.push((changed, "submission_schema_unsupported"));
    let mut changed = request.clone();
    changed.scope.schema_version = 2;
    invalid.push((changed, "composer_schema_unsupported"));
    let mut changed = request.clone();
    changed.composer_id = "00".repeat(16);
    invalid.push((changed, "composer_id_invalid"));
    for (request, code) in invalid {
        let error = runtime.submission_reserve(request).await.unwrap_err();
        assert_eq!(error.report().code, code);
        assert!(!error.report().retryable);
    }
    assert!(!runtime.submission_reserve(request).await.unwrap().replayed);
    runtime.shutdown().await.unwrap();
}

#[test]
fn typed_submission_failures_preserve_recovery_without_leaking_storage_details() {
    use tera_core::runtime::product_surface::SubmissionReservationError as E;
    for (error, code, disposition) in [
        (
            E::CorruptRecord,
            "submission_record_corrupt",
            FfiRecoveryDisposition::StorageFailure,
        ),
        (
            E::UnsupportedSchema,
            "submission_schema_unsupported",
            FfiRecoveryDisposition::UnsupportedVersion,
        ),
        (
            E::InvalidReceipt,
            "submission_receipt_mismatch",
            FfiRecoveryDisposition::OutcomeUnknown,
        ),
        (
            E::ClockUnavailable,
            "operation_clock_unavailable",
            FfiRecoveryDisposition::RuntimeUnavailable,
        ),
    ] {
        let error = TeraAppError::from(error);
        assert_eq!(error.report().code, code);
        assert_eq!(
            classify_error_recovery(1, code.into()).disposition,
            disposition
        );
    }
}
