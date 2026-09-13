use tera_core::runtime::product_surface::{AddCommandType, ComposerFormInput, ComposerPartialForm};
use tera_ffi::*;
mod support;

fn source() -> FfiComposerSaveRequest {
    let mut form = ComposerFormInput::empty(AddCommandType::CreateUpdate);
    form.content = "PRIVATE captured publication".into();
    FfiComposerSaveRequest {
        schema_version: 1,
        scope: FfiComposerScopeRecord {
            schema_version: 1,
            author_public_key: support::PUBLIC_KEY.into(),
            local_network_id: "nearby".into(),
        },
        id: composer_reserve_id().unwrap().id,
        expected_revision: None,
        edit_sequence: 1,
        form: (&ComposerPartialForm::new(form).unwrap()).into(),
    }
}

fn request(source: &FfiComposerSaveRequest) -> FfiSubmissionReservationRequest {
    FfiSubmissionReservationRequest {
        schema_version: 1,
        command_id: submission_reserve_id().unwrap().id,
        scope: source.scope.clone(),
        composer_id: source.id.clone(),
        expected_revision: 1,
    }
}

#[tokio::test]
async fn native_operation_bridge_preserves_one_capture_across_duplicates_edits_and_sqlite_restart()
{
    let (root, runtime) = support::runtime().await;
    runtime
        .configure_simulator_relays(vec!["ws://127.0.0.1:19999".into()])
        .unwrap();
    let mut source = source();
    let saved = runtime.composer_save(source.clone()).await.unwrap();
    let request = request(&source);
    assert!(
        runtime
            .submission_recover(request.clone())
            .await
            .unwrap()
            .is_none()
    );
    let (left, right) = tokio::join!(
        runtime.submission_prepare(request.clone(), vec![]),
        runtime.submission_prepare(request.clone(), vec![])
    );
    let first = left.unwrap();
    assert_eq!(first, right.unwrap());
    assert_eq!(first.captured, saved.draft);
    assert_eq!(first.request, request);
    assert_eq!(first.state, FfiOutboxState::ReadyToSign);
    assert_eq!(first.revision, 1);
    assert_eq!(first.settlement.signed, 0);
    assert!(!format!("{first:?}").contains("PRIVATE"));
    assert!(
        runtime
            .phase1_draft_status(first.intent_id.clone())
            .await
            .is_err()
    );
    source.expected_revision = Some(1);
    source.edit_sequence = 2;
    source.form.content = "later editable text".into();
    let latest = runtime.composer_save(source.clone()).await.unwrap();
    assert_eq!(
        runtime
            .submission_prepare(request.clone(), vec![])
            .await
            .unwrap(),
        first
    );
    // No signer is installed. A failed advance can still have durably queued the original intent.
    assert!(
        runtime
            .submission_advance(request.clone(), 1)
            .await
            .is_err()
    );
    let queued = runtime.submission_status(request.clone()).await.unwrap();
    assert_eq!(queued.operation_id, first.operation_id);
    assert_eq!(queued.intent_id, first.intent_id);
    assert_eq!(queued.captured, first.captured);
    assert_eq!(queued.revision, 2);
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
    runtime
        .configure_simulator_relays(vec!["ws://127.0.0.1:19998".into()])
        .unwrap();
    assert_eq!(
        runtime.submission_recover(request.clone()).await.unwrap(),
        Some(queued.clone())
    );
    assert_eq!(
        runtime
            .submission_prepare(request.clone(), vec![])
            .await
            .unwrap(),
        queued
    );
    assert_eq!(
        runtime
            .composer_load(source.scope.clone(), source.id.clone())
            .await
            .unwrap(),
        latest.draft
    );
    let page = runtime
        .submission_page(source.scope.clone(), 1, None)
        .await
        .unwrap();
    assert_eq!(page.schema_version, 1);
    assert_eq!(page.scope, source.scope);
    assert!(page.next_cursor.is_none());
    let [
        FfiSubmissionListEntry::Submission {
            request: selected,
            state:
                FfiSubmissionSummaryState::Operation {
                    intent_id,
                    operation_id,
                    revision,
                    ..
                },
            ..
        },
    ] = page.entries.as_slice()
    else {
        panic!("one scoped operation")
    };
    assert_eq!(selected, &request);
    assert_eq!(intent_id, &first.intent_id);
    assert_eq!(operation_id, &first.operation_id);
    assert_eq!(*revision, 2);
    assert!(!format!("{page:?}").contains("PRIVATE"));
    assert!(runtime.phase1_draft_heads(100).await.unwrap().is_empty());
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn missing_policy_and_invalid_scoped_requests_cannot_acknowledge_an_operation() {
    let (_root, runtime) = support::runtime().await;
    let settings = runtime.phase1_settings().await.unwrap();
    runtime
        .phase1_replace_settings(FfiReplaceSettingsRecord {
            schema_version: 1,
            expected_revision: settings.revision,
            relays: FfiRelayPreferencesRecord {
                schema_version: 1,
                environment: FfiMobileNetworkEnvironment::Public,
                endpoints: vec![FfiRelayPreferenceRecord {
                    schema_version: 1,
                    url: "wss://read.example".into(),
                    access: FfiRelayAccessPreference::ReadOnly,
                }],
            },
            blossom: settings.blossom,
            media_network: settings.media_network,
            local_storage: settings.local_storage,
        })
        .await
        .unwrap();
    runtime.phase1_apply_settings_to_runtime().await.unwrap();
    let source = source();
    let request = request(&source);
    runtime.composer_save(source.clone()).await.unwrap();
    let error = runtime
        .submission_prepare(request.clone(), vec![])
        .await
        .unwrap_err();
    assert_eq!(error.report().code, "submission_policy_unavailable");
    assert!(!error.report().retryable);
    assert_eq!(
        classify_error_recovery(1, error.report().code.clone()).disposition,
        FfiRecoveryDisposition::NetworkPolicy
    );
    assert!(
        runtime
            .submission_recover(request.clone())
            .await
            .unwrap()
            .is_none()
    );
    let page = runtime
        .submission_page(source.scope.clone(), 1, None)
        .await
        .unwrap();
    assert!(matches!(
        page.entries.as_slice(),
        [FfiSubmissionListEntry::Submission {
            state: FfiSubmissionSummaryState::Reserved,
            ..
        }]
    ));
    runtime
        .configure_simulator_relays(vec!["ws://127.0.0.1:19999".into()])
        .unwrap();
    let status = runtime
        .submission_prepare(request.clone(), vec![])
        .await
        .unwrap();
    let mut changed = request.clone();
    changed.expected_revision = 2;
    assert_eq!(
        runtime
            .submission_prepare(changed, vec![])
            .await
            .unwrap_err()
            .report()
            .code,
        "idempotency_conflict"
    );
    let mut changed = request.clone();
    changed.schema_version = 2;
    assert_eq!(
        runtime
            .submission_recover(changed)
            .await
            .unwrap_err()
            .report()
            .code,
        "submission_schema_unsupported"
    );
    let mut foreign = source.scope.clone();
    foreign.local_network_id = "other".into();
    assert!(
        runtime
            .submission_page(foreign, 100, None)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    assert!(
        runtime
            .submission_page(source.scope.clone(), 0, None)
            .await
            .is_err()
    );
    assert!(
        runtime
            .submission_page(source.scope, 1, Some("malformed".into()))
            .await
            .is_err()
    );
    assert_eq!(runtime.submission_status(request).await.unwrap(), status);
    runtime.shutdown().await.unwrap();
}
