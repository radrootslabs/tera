use super::{coordinate_support::*, *};
use crate::runtime::product_surface::{
    coordinate::intent_from_draft, recovery_inventory::RecoveryOwner,
};

#[tokio::test]
async fn coordinate_claims_with_the_same_identifier_are_independent_for_other_authors_and_kinds() {
    let first = runtime();
    let mut other = TeraRuntime::from_client_builder(
        ClientBuilder::memory_default(),
        Some(
            PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
                .unwrap(),
        ),
        None,
        None,
        None,
        None,
    )
    .unwrap();
    // Deliberately share the real memory owner while retaining distinct author
    // admissions. Persistent application accounts otherwise own separate stores.
    other.client = first.client.clone();
    let timed = saved(&first, [191; 16], "same:tag").await;
    let foreign = saved(&other, [192; 16], "same:tag").await;
    let event = AuthoredCalendarDateEvent::new(
        "same:tag",
        "Date market",
        CalendarDate::parse("2026-12-31").unwrap(),
    )
    .unwrap();
    let date = first
        .phase1_save_draft(
            [193; 16],
            Phase1AddCommand::CreateEvent(CreateEvent::date(event)),
            1_750_000_000,
            vec![],
            None,
            1_750_000_000_000,
        )
        .await
        .unwrap();
    let one = first.admit_coordinate(timed.draft()).await.unwrap();
    let two = other.admit_coordinate(foreign.draft()).await.unwrap();
    let three = first.admit_coordinate(date.draft()).await.unwrap();
    assert!(one.is_some() && two.is_some() && three.is_some());
    for (owner, draft) in [
        (&first, timed.draft()),
        (&other, foreign.draft()),
        (&first, date.draft()),
    ] {
        assert!(
            owner
                .coordinate_is_current(&intent_from_draft(draft).unwrap().unwrap())
                .await
                .unwrap()
        );
    }
}

#[tokio::test]
async fn coordinate_concurrent_revisions_replay_one_owner_and_never_reclaim_a_retired_binding() {
    let runtime = runtime();
    let prior = signed_head(SECRET, 31_923, "market:one", 1_700_000_000);
    retain(&runtime, prior.clone()).await;
    let captured = intent(&prior, "Changed market");
    let (one, two) = tokio::join!(
        runtime.prepare_revision_intent([111; 16], captured.clone()),
        runtime.prepare_revision_intent([112; 16], captured.clone())
    );
    assert_ne!(one.is_ok(), two.is_ok());
    let (winner, winner_request, loser_request) = match (one, two) {
        (Ok(value), Err(error)) => {
            assert!(matches!(
                error,
                Phase1DraftError::RevisionConflict | Phase1DraftError::OperationInProgress
            ));
            (value, [111; 16], [112; 16])
        }
        (Err(error), Ok(value)) => {
            assert!(matches!(
                error,
                Phase1DraftError::RevisionConflict | Phase1DraftError::OperationInProgress
            ));
            (value, [112; 16], [111; 16])
        }
        _ => panic!("exactly one coordinate owner"),
    };
    let winner_id = *winner.replacement().draft().draft_id().as_bytes();
    let replay = runtime
        .prepare_revision_intent(winner_request, captured.clone())
        .await
        .unwrap();
    assert_eq!(replay.replacement().draft(), winner.replacement().draft());
    assert!(replay.replacement().coordinate_captured());
    let loser = runtime
        .prepare_revision_intent(loser_request, captured.clone())
        .await
        .unwrap();
    assert!(!loser.can_resume());
    assert!(!loser.replacement().coordinate_writable());
    assert_eq!(
        queue(&runtime, loser.replacement()).await.unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
    runtime.phase1_cancel_revision(winner_id).await.unwrap();
    let successor = queue(&runtime, loser.replacement()).await.unwrap();
    assert!(successor.coordinate_writable());
    let loser_id = *successor.draft().draft_id().as_bytes();
    runtime.phase1_cancel_revision(loser_id).await.unwrap();
    // A later retired owner does not make the first immutable binding current again.
    let first = runtime
        .prepare_revision_intent(winner_request, captured)
        .await
        .unwrap();
    assert!(!first.replacement().coordinate_writable());
    assert!(
        runtime
            .admit_coordinate(first.replacement().draft())
            .await
            .is_err()
    );
    assert!(first.retraction().is_none());
    let page = runtime.recovery_page(64, None).await.unwrap();
    assert_eq!(page.entries.len(), 2);
    assert!(
        page.entries
            .iter()
            .all(|entry| matches!(entry.owner, RecoveryOwner::Legacy))
    );
    assert!(page.scanned > 2); // Valid ownership metadata is not another operation.
}

#[tokio::test]
async fn coordinate_full_identity_isolates_other_kind_author_and_identifier() {
    let runtime = runtime();
    let prior = signed_head(SECRET, 31_923, "same:identifier", 1_700_000_000);
    retain(&runtime, prior.clone()).await;
    let first = runtime
        .prepare_revision_intent([111; 16], intent(&prior, "Update"))
        .await
        .unwrap();
    let coordinate = intent_from_draft(first.replacement().draft())
        .unwrap()
        .unwrap();
    let other_secret = "0000000000000000000000000000000000000000000000000000000000000002";
    for other in [
        signed_head(other_secret, 31_923, "same:identifier", 1_900_000_000),
        signed_head(SECRET, 31_922, "same:identifier", 1_900_000_000),
        signed_head(SECRET, 31_923, "other:identifier", 1_900_000_000),
    ] {
        retain(&runtime, other).await;
        assert!(runtime.coordinate_is_current(&coordinate).await.unwrap());
    }
    let unrelated = saved(&runtime, [114; 16], "another:coordinate").await;
    assert!(
        queue(&runtime, &unrelated)
            .await
            .unwrap()
            .coordinate_writable()
    );
    retain(
        &runtime,
        signed_head(SECRET, 31_923, "same:identifier", 1_900_000_000),
    )
    .await;
    assert!(!runtime.coordinate_is_current(&coordinate).await.unwrap());
    assert!(
        !runtime
            .phase1_revision_status(*first.replacement().draft().draft_id().as_bytes())
            .await
            .unwrap()
            .can_resume()
    );
    assert_eq!(
        queue(&runtime, first.replacement()).await.unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
}

#[tokio::test]
async fn coordinate_ordinary_writers_cannot_bypass_a_claim_or_a_known_signed_winner() {
    let runtime = signing_runtime();
    let first = saved(&runtime, [121; 16], "ordinary:one").await;
    let second = saved(&runtime, [122; 16], "ordinary:one").await;
    let queued = queue(&runtime, &first).await.unwrap();
    assert_eq!(
        queue(&runtime, &second).await.unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
    let before = runtime
        .phase1_sign_queued_draft([121; 16], queued.draft().revision().get())
        .await
        .unwrap();
    let signed = before.push().unwrap().artifact().signed().unwrap().clone();
    retain(
        &runtime,
        signed_head(SECRET, 31_923, "ordinary:one", 1_950_000_000),
    )
    .await;
    assert_eq!(
        runtime
            .phase1_sign_queued_draft([121; 16], queued.draft().revision().get())
            .await
            .unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
    assert_eq!(
        runtime
            .phase1_advance_draft([121; 16], queued.draft().revision().get())
            .await
            .unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
    let after = runtime.phase1_draft_status([121; 16]).await.unwrap();
    assert_eq!(after.push().unwrap().artifact().signed().unwrap(), &signed);
    assert!(!after.coordinate_writable());
    assert!(after.push().unwrap().delivery_plan().attempts().is_empty());
    let fresh = saved(&runtime, [123; 16], "ordinary:one").await;
    assert_eq!(
        queue(&runtime, &fresh).await.unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
}

#[tokio::test]
async fn coordinate_sqlite_restart_retains_capture_claim_and_permanent_ownership_generation() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    let runtime = RuntimeBuilder::new(config.clone()).build().await.unwrap();
    let prior = signed_head(SECRET, 31_923, "restart:market", 1_700_000_000);
    retain(&runtime, prior.clone()).await;
    let first = runtime
        .prepare_revision_intent([131; 16], intent(&prior, "Retained"))
        .await
        .unwrap();
    let id = *first.replacement().draft().draft_id().as_bytes();
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = RuntimeBuilder::new(config).build().await.unwrap();
    let replay = runtime
        .prepare_revision_intent([131; 16], intent(&prior, "Retained"))
        .await
        .unwrap();
    assert_eq!(replay.replacement().draft(), first.replacement().draft());
    assert!(replay.replacement().coordinate_writable());
    runtime.phase1_cancel_revision(id).await.unwrap();
    let next = runtime
        .prepare_revision_intent([132; 16], intent(&prior, "Successor"))
        .await
        .unwrap();
    assert!(next.replacement().coordinate_writable());
    let old = runtime.phase1_revision_status(id).await.unwrap();
    assert!(!old.replacement().coordinate_writable());
    assert!(old.retraction().is_none());
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let inventory =
        crate::runtime::product_surface::media_gc::inspect_media_references(root.path(), vec![])
            .await
            .unwrap();
    assert!(inventory.permits_orphan(&"a".repeat(64), 1, 1_900_000_000_000));
}
