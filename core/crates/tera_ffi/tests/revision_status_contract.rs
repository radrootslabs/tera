use tera_ffi::*;
#[path = "support/retraction.rs"]
mod retraction;
mod support;

#[tokio::test]
async fn revision_status_maps_original_relation_actions_and_stopped_facts_across_reopen() {
    let (root, runtime) = support::runtime().await;
    runtime.shutdown().await.unwrap();
    let (card_id, source_event_id) = retraction::seed(root.path(), input()).await;
    let runtime = TeraRuntime::new(
        root.path().to_string_lossy().into_owned(),
        support::PUBLIC_KEY.into(),
        support::GENERATION.into(),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .await
    .unwrap();
    let saved = runtime
        .phase1_save_revision_intent(FfiRevisionInputRecord {
            request_id: "ac".repeat(16),
            schema_version: 1,
            card_id: card_id.clone(),
            source_event_id: source_event_id.clone(),
            source_address: None,
            author_public_key: support::PUBLIC_KEY.into(),
            replacement: input(),
        })
        .await
        .unwrap();
    assert_eq!(saved.original.card_id, card_id);
    assert_eq!(saved.original.source_event_id, source_event_id);
    assert_eq!(saved.original.author_public_key, support::PUBLIC_KEY);
    assert!(saved.original.source_address.is_none());
    assert_eq!(saved.operation_id, saved.replacement.draft_id);
    assert!(saved.replacement.revision_parent_draft_id.is_none());
    assert!(saved.can_resume && saved.can_cancel);
    assert!(!saved.replacement_progress.stopped);
    assert!(saved.replacement_progress.targets.is_none() && saved.retraction_progress.is_none());
    let stopped = runtime
        .phase1_cancel_revision(saved.operation_id.clone())
        .await
        .unwrap();
    assert_eq!(stopped.phase, FfiRevisionPhase::Cancelled);
    assert!(stopped.replacement_progress.stopped && !stopped.can_resume && !stopped.can_cancel);
    assert_eq!(stopped.original, saved.original);
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
    assert_eq!(
        runtime
            .phase1_revision_status(saved.operation_id.clone())
            .await
            .unwrap(),
        stopped
    );
    assert_eq!(
        runtime
            .phase1_advance_revision(saved.operation_id)
            .await
            .unwrap(),
        stopped
    );
    runtime.shutdown().await.unwrap();
}

fn input() -> FfiAddDraftInput {
    FfiAddDraftInput {
        schema_version: 1,
        command_type: FfiAddCommandType::CreateUpdate,
        content: "Corrected harvest".into(),
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
        media: vec![],
    }
}
