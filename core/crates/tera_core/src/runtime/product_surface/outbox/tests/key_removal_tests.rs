use super::{coordinate_effect_support as effects, retraction_support::*, *};
use std::sync::atomic::Ordering;

#[tokio::test]
async fn key_removal_rejects_foreign_author_and_linked_revision_retractions() {
    let runtime = signing_runtime();
    let saved = saved(&runtime, [216; 16]).await;
    let other = nostr::Keys::parse(&"02".repeat(32))
        .unwrap()
        .public_key()
        .to_string();
    let foreign = TeraRuntime::from_client_builder(
        ClientBuilder::memory_default(),
        Some(PublicKey::from_hex(&other).unwrap()),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    foreign
        .storage()
        .unwrap()
        .append_authored_draft(saved.draft().clone(), None)
        .await
        .unwrap();
    assert!(
        foreign
            .prepare_retraction_for_key_removal([216; 16], saved.draft().revision().get())
            .await
            .is_err()
    );
    let target = original(&runtime).await;
    let permit = runtime.mutations.draft([217; 16]).unwrap();
    let linked = runtime
        .phase1_save_retraction_draft_admitted(
            &permit,
            [217; 16],
            target.command_type,
            target.card_id,
            &target.source_event_id,
            1,
            None,
            "Linked",
            1_700_000_001,
            1_700_000_001_000,
            Some([218; 16]),
        )
        .await
        .unwrap();
    drop(permit);
    assert!(
        runtime
            .prepare_retraction_for_key_removal([217; 16], linked.draft().revision().get())
            .await
            .is_err()
    );
    assert!(
        runtime
            .storage()
            .unwrap()
            .authored_draft_head(AuthoredDraftId::new([217; 16]).unwrap())
            .await
            .unwrap()
            .unwrap()
            .operation_id()
            .is_none()
    );
}

async fn saved(runtime: &TeraRuntime, id: [u8; 16]) -> Phase1DraftStatus {
    let target = original(runtime).await;
    runtime
        .phase1_save_retraction_draft(
            id,
            target.command_type,
            target.card_id,
            &target.source_event_id,
            1,
            None,
            "Remove this post",
            1_700_000_001,
            1_700_000_001_000,
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn key_removal_retains_signed_request_across_sqlite_restart_without_signer() {
    let root = tempfile::tempdir().unwrap();
    let config = coordinate_support::config(root.path());
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    let signer = effects::PausedSigner::new();
    signer.pause.store(false, Ordering::SeqCst);
    let runtime = RuntimeBuilder::new(config.clone())
        .signer(signer.clone())
        .build()
        .await
        .unwrap();
    let saved = saved(&runtime, [211; 16]).await;
    let signed = runtime
        .prepare_retraction_for_key_removal([211; 16], saved.draft().revision().get())
        .await
        .unwrap();
    let retained = signed.push().unwrap().artifact().signed().unwrap().clone();
    assert_eq!(signer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(signed.state(), Phase1OutboxState::Signed);
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let reopened = RuntimeBuilder::new(config).build().await.unwrap();
    let replay = reopened
        .prepare_retraction_for_key_removal([211; 16], signed.draft().revision().get())
        .await
        .unwrap();
    assert_eq!(replay.push().unwrap().artifact().signed(), Some(&retained));
    assert_eq!(replay, signed);
    assert_eq!(signer.calls.load(Ordering::SeqCst), 1);
    reopened
        .require_revision_source(&original_target(&Phase1AddCommand::CreateUpdate(
            CreateUpdate::new("Original harvest").unwrap(),
        )))
        .await
        .unwrap();
    reopened.shutdown().await.unwrap();
}

#[tokio::test]
async fn key_removal_missing_signer_keeps_request_and_rejects_success() {
    let runtime = runtime();
    let saved = saved(&runtime, [212; 16]).await;
    let queued = runtime
        .phase1_queue_draft(
            [212; 16],
            saved.draft().revision().get(),
            policy(),
            1_700_000_001_001,
        )
        .await
        .unwrap();
    assert!(
        runtime
            .prepare_retraction_for_key_removal([212; 16], queued.draft().revision().get())
            .await
            .is_err()
    );
    let after = runtime.phase1_draft_status([212; 16]).await.unwrap();
    assert_eq!(after.draft(), queued.draft());
    assert!(after.push().unwrap().artifact().signed().is_none());
}

#[tokio::test]
async fn key_removal_cancelled_signing_retains_queued_request_without_delivery() {
    let signer = effects::PausedSigner::new();
    let runtime = effects::runtime(signer.clone(), "ws://127.0.0.1:9");
    let saved = saved(&runtime, [213; 16]).await;
    let owner = runtime.clone();
    let task = tokio::spawn(async move {
        owner
            .prepare_retraction_for_key_removal([213; 16], saved.draft().revision().get())
            .await
    });
    signer.wait().await;
    task.abort();
    assert!(task.await.unwrap_err().is_cancelled());
    let after = runtime.phase1_draft_status([213; 16]).await.unwrap();
    assert!(after.push().unwrap().artifact().signed().is_none());
    assert!(
        runtime
            .prepare_retraction_for_key_removal([213; 16], after.draft().revision().get())
            .await
            .is_err(),
        "An interrupted signer cannot be silently treated as prepared"
    );
    let held = runtime.phase1_draft_status([213; 16]).await.unwrap();
    assert!(held.push().unwrap().artifact().signed().is_none());
}

#[tokio::test]
async fn key_removal_rejects_wrong_kind_stale_revision_and_unsigned_stopped_work() {
    let runtime = signing_runtime();
    let draft = runtime
        .phase1_save_draft(
            [214; 16],
            Phase1AddCommand::CreateUpdate(CreateUpdate::new("Keep this").unwrap()),
            1_700_000_001,
            vec![],
            None,
            1_700_000_001_000,
        )
        .await
        .unwrap();
    assert!(
        runtime
            .prepare_retraction_for_key_removal([214; 16], draft.draft().revision().get())
            .await
            .is_err()
    );
    let saved = saved(&runtime, [215; 16]).await;
    assert_eq!(
        runtime
            .prepare_retraction_for_key_removal([215; 16], saved.draft().revision().get() + 1)
            .await
            .unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
    let cancelled = runtime
        .phase1_cancel_draft([215; 16], saved.draft().revision().get(), 1_700_000_001_001)
        .await
        .unwrap();
    assert!(
        runtime
            .prepare_retraction_for_key_removal([215; 16], cancelled.draft().revision().get())
            .await
            .is_err()
    );
    assert_eq!(
        runtime.phase1_draft_status([215; 16]).await.unwrap(),
        cancelled
    );
}
