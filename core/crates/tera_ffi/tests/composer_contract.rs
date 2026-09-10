use tera_ffi::*;
mod support;

#[tokio::test]
async fn legacy_inventory_is_versioned_paged_and_separate_from_partial_composers() {
    let (root, runtime) = support::runtime().await;
    runtime.composer_save(request()).await.unwrap();
    let mut saved = Vec::new();
    for _ in 0..2 {
        saved.push(
            runtime
                .phase1_save_add_intent(legacy_input(), None, None)
                .await
                .unwrap(),
        );
    }
    let first = runtime.legacy_draft_page(1, 1, None).await.unwrap();
    assert_eq!(first.schema_version, 1);
    assert_eq!(first.author_public_key, support::PUBLIC_KEY);
    assert_eq!(first.entries.len(), 1);
    let FfiLegacyDraftListEntry::Draft { summary } = &first.entries[0] else {
        panic!("valid legacy summary")
    };
    assert_eq!(summary.schema_version, 1);
    assert_eq!(summary.revision, 1);
    assert_eq!(summary.command_type, FfiAddCommandType::CreateUpdate);
    assert_eq!(summary.state, FfiOutboxState::Draft);
    assert!(summary.has_form && !summary.is_revision);
    assert_eq!(summary.media_count, 0);
    assert_eq!(summary.verified_media_count, 0);
    assert_eq!(summary.possible_orphan_count, 0);
    assert!(summary.settlement.is_none());
    let first_id = summary.draft_id.clone();
    for (version, limit, cursor, code) in [
        (2, 1, None, "unsupported_schema_version"),
        (1, 0, None, "draft_inventory_invalid"),
        (1, 257, None, "draft_inventory_invalid"),
        (
            1,
            1,
            Some("invalid".to_owned()),
            "draft_inventory_cursor_invalid",
        ),
    ] {
        assert_eq!(
            runtime
                .legacy_draft_page(version, limit, cursor)
                .await
                .unwrap_err()
                .report()
                .code,
            code
        );
    }
    runtime.shutdown().await.unwrap();
    let runtime = TeraRuntime::new(
        root.path().to_string_lossy().into_owned(),
        support::PUBLIC_KEY.into(),
        support::GENERATION.into(),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .await
    .unwrap();
    let second = runtime
        .legacy_draft_page(1, 1, first.next_cursor)
        .await
        .unwrap();
    assert!(second.next_cursor.is_none());
    let FfiLegacyDraftListEntry::Draft { summary } = &second.entries[0] else {
        panic!("valid second summary")
    };
    let mut actual = vec![first_id, summary.draft_id.clone()];
    actual.sort();
    let mut expected: Vec<_> = saved.iter().map(|draft| draft.draft_id.clone()).collect();
    expected.sort();
    assert_eq!(actual, expected);
    for draft in saved {
        assert_eq!(
            runtime
                .phase1_draft_status(draft.draft_id.clone())
                .await
                .unwrap(),
            draft
        );
    }
    runtime.shutdown().await.unwrap();
}

fn legacy_input() -> FfiAddDraftInput {
    FfiAddDraftInput {
        schema_version: 1,
        command_type: FfiAddCommandType::CreateUpdate,
        content: "legacy fixture".into(),
        identifier: None,
        title: None,
        summary: None,
        location: None,
        event_timing: None,
        event_start_date: None,
        event_end_date: None,
        event_start_unix_s: None,
        event_end_unix_s: None,
        event_timezone: None,
        price_amount: None,
        currency: None,
        unit: None,
        quantity: None,
        food_published_at_unix_s: None,
        food_status: None,
        media: Vec::new(),
    }
}

fn scope() -> FfiComposerScopeRecord {
    FfiComposerScopeRecord {
        schema_version: 1,
        author_public_key: support::PUBLIC_KEY.into(),
        local_network_id: "nearby".into(),
    }
}

fn request() -> FfiComposerSaveRequest {
    FfiComposerSaveRequest {
        schema_version: 1,
        scope: scope(),
        id: composer_reserve_id().unwrap().id,
        expected_revision: None,
        edit_sequence: u64::MAX - 1,
        form: FfiComposerFormRecord {
            schema_version: 1,
            command_type: FfiAddCommandType::CreateEvent,
            content: " private partial\n\0é ".into(),
            identifier: None,
            title: None,
            summary: None,
            location: None,
            event_timing: Some(FfiEventTimingKind::AllDay),
            event_start_date: Some("2026-09-".into()),
            event_end_date: None,
            event_start_unix_s: Some(u64::MAX),
            event_end_unix_s: None,
            event_timezone: None,
            price_amount: Some("12.".into()),
            currency: None,
            unit: None,
            quantity: Some("-".into()),
            food_published_at_unix_s: None,
            food_status: None,
            media: vec![FfiComposerMediaRecord {
                schema_version: 1,
                opaque_reference: "media:abc".into(),
                sha256: "ab".repeat(32),
                media_type: "image/png".into(),
                byte_size: 24,
                width: 2,
                height: 2,
                alt: String::new(),
                prepared_at_unix_s: 1_800_000_000,
            }],
        },
    }
}

#[tokio::test]
async fn typed_composer_preserves_partial_fields_exact_receipts_and_restart_without_signing() {
    let (root, runtime) = support::runtime().await;
    let mut request = request();
    let created = runtime.composer_save(request.clone()).await.unwrap();
    assert_eq!(created.schema_version, 1);
    assert_eq!(created.draft.scope, request.scope);
    assert_eq!(created.draft.id, request.id);
    assert_eq!(created.draft.revision, 1);
    assert_eq!(created.draft.edit_sequence, u64::MAX - 1);
    assert_eq!(created.draft.form, request.form);
    assert!(!created.replayed);
    assert!(!format!("{created:?}").contains("private partial"));
    request.expected_revision = Some(1);
    request.edit_sequence = u64::MAX;
    request.form.content = "newest incomplete".into();
    let saved = runtime.composer_save(request.clone()).await.unwrap();
    assert_eq!(saved.draft.revision, 2);
    assert_eq!(saved.draft.edit_sequence, u64::MAX);
    let failure = runtime.composer_save(request.clone()).await.unwrap_err();
    assert_eq!(failure.report().code, "composer_revision_conflict");
    assert_eq!(failure.report().recovery_actions, ["reload_composer"]);
    assert_eq!(
        classify_error_recovery(1, failure.report().code.clone()).disposition,
        FfiRecoveryDisposition::StaleRevision
    );
    let page = runtime.composer_list(1, scope(), 1, None).await.unwrap();
    assert_eq!(page.scope, scope());
    assert_eq!(page.entries.len(), 1);
    let FfiComposerListEntry::Draft { summary } = &page.entries[0] else {
        panic!("summary")
    };
    assert_eq!(summary.id, request.id);
    assert_eq!(summary.revision, 2);
    assert_eq!(summary.edit_sequence, u64::MAX);
    assert!(page.next_cursor.is_none());
    assert!(runtime.phase1_draft_heads(100).await.unwrap().is_empty());
    runtime.shutdown().await.unwrap();
    let reopened = TeraRuntime::new(
        root.path().to_string_lossy().into_owned(),
        support::PUBLIC_KEY.into(),
        support::GENERATION.into(),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .await
    .unwrap();
    assert_eq!(
        reopened
            .composer_load(scope(), request.id.clone())
            .await
            .unwrap(),
        saved.draft
    );
    let mut foreign = scope();
    foreign.local_network_id = "elsewhere".into();
    assert_eq!(
        reopened
            .composer_load(foreign.clone(), request.id.clone())
            .await
            .unwrap_err()
            .report()
            .code,
        "composer_scope_mismatch"
    );
    assert!(
        reopened
            .composer_list(1, foreign, 10, None)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    let mut foreign = scope();
    foreign.author_public_key =
        "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5".into();
    assert_eq!(
        reopened
            .composer_load(foreign, request.id)
            .await
            .unwrap_err()
            .report()
            .code,
        "composer_scope_mismatch"
    );
    reopened.shutdown().await.unwrap();
    assert_eq!(
        reopened
            .composer_list(1, scope(), 10, None)
            .await
            .unwrap_err()
            .report()
            .code,
        "client_closed"
    );
}

#[tokio::test]
async fn typed_composer_rejects_versions_ids_and_bounds_before_writing_any_record() {
    let (_root, runtime) = support::runtime().await;
    let baseline = request();
    let mut invalid = Vec::new();
    let mut value = baseline.clone();
    value.schema_version = 2;
    invalid.push((value, "composer_schema_unsupported"));
    let mut value = baseline.clone();
    value.scope.schema_version = 2;
    invalid.push((value, "composer_schema_unsupported"));
    let mut value = baseline.clone();
    value.form.schema_version = 2;
    invalid.push((value, "composer_schema_unsupported"));
    let mut value = baseline.clone();
    value.form.media[0].schema_version = 2;
    invalid.push((value, "composer_schema_unsupported"));
    let mut value = baseline.clone();
    value.id = "00".repeat(16);
    invalid.push((value, "composer_id_invalid"));
    let mut value = baseline.clone();
    value.id = "AB".repeat(16);
    invalid.push((value, "composer_id_invalid"));
    let mut value = baseline.clone();
    value.expected_revision = Some(u64::MAX);
    invalid.push((value, "composer_revision_invalid"));
    let mut value = baseline.clone();
    value.edit_sequence = 0;
    invalid.push((value, "composer_edit_sequence_invalid"));
    let mut value = baseline.clone();
    value.scope.local_network_id = " nearby".into();
    invalid.push((value, "composer_scope_invalid"));
    let mut value = baseline.clone();
    value.form.content = "é".repeat(32_768);
    invalid.push((value, "composer_form_invalid"));
    let mut value = baseline.clone();
    value.form.media[0].opaque_reference = "file:/private".into();
    invalid.push((value, "composer_form_invalid"));
    for (input, code) in invalid {
        let error = runtime.composer_save(input).await.unwrap_err();
        assert_eq!(error.report().schema_version, 1);
        assert_eq!(error.report().code, code);
        assert!(!error.report().retryable);
        assert!(!format!("{error:?}").contains("private partial"));
    }
    assert_eq!(
        runtime
            .composer_list(2, scope(), 10, None)
            .await
            .unwrap_err()
            .report()
            .code,
        "composer_schema_unsupported"
    );
    assert_eq!(
        runtime
            .composer_list(1, scope(), 0, None)
            .await
            .unwrap_err()
            .report()
            .code,
        "composer_list_invalid"
    );
    assert_eq!(
        runtime
            .composer_list(1, scope(), 10, Some("future".into()))
            .await
            .unwrap_err()
            .report()
            .code,
        "composer_cursor_invalid"
    );
    assert!(
        runtime
            .composer_list(1, scope(), 10, None)
            .await
            .unwrap()
            .entries
            .is_empty()
    );
    runtime.shutdown().await.unwrap();
}
