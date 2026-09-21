use super::revision_delivery_support::*;
use super::*;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn aggregate_any_acceptance_never_authorizes_the_other_relay() {
    let root = tempfile::tempdir().unwrap();
    let a = Relay::start(true).await;
    let b = Relay::start(false).await;
    let urls = vec![a.url.clone(), b.url.clone()];
    let runtime = persistent_runtime(root.path(), &urls).await;
    let saved = saved_revision(&runtime).await;
    let id = *saved.replacement().draft().draft_id().as_bytes();
    let now = phase1_operation_now_unix_ms().unwrap();
    let policy = Phase1QueuePolicy::new(
        vec![a.url.clone(), b.url.clone()],
        Phase1RelaySatisfaction::AnyAccepted,
        now + 60_000,
        Phase1CancellationPolicy::LocalCooperative,
    )
    .unwrap();
    runtime
        .phase1_queue_draft(
            id,
            saved.replacement().draft().revision().get(),
            policy,
            now,
        )
        .await
        .unwrap();
    let status = runtime.phase1_advance_revision(id).await.unwrap();
    assert_eq!(status.replacement().state(), Phase1OutboxState::Complete);
    assert_eq!(status.phase(), Phase1RevisionPhase::PartialEffect);
    assert!(!status.can_resume()); // Any policy is satisfied; B was never accepted.
    assert_eq!(
        status
            .retraction()
            .unwrap()
            .push()
            .unwrap()
            .delivery_plan()
            .request()
            .unwrap()
            .target_set()
            .len(),
        2
    );
    assert_eq!(a.kinds(), vec![1, 5]);
    assert_eq!(b.kinds(), vec![1]);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn per_target_revision_freezes_policy_and_reopens_one_signed_child() {
    let root = tempfile::tempdir().unwrap();
    let a = Relay::start(true).await;
    let b = Relay::start(false).await;
    let c = Relay::start(true).await;
    let mut urls = vec![a.url.clone(), b.url.clone()];
    let runtime = persistent_runtime(root.path(), &urls).await;
    let saved = saved_revision(&runtime).await;
    let id = *saved.replacement().draft().draft_id().as_bytes();
    let queued = runtime
        .phase1_queue_add_intent(
            Phase1QueueIntent::new(id, saved.replacement().draft().revision().get()).unwrap(),
        )
        .await
        .unwrap();
    runtime
        .phase1_advance_draft(id, queued.draft().revision().get())
        .await
        .unwrap();
    urls.push(c.url.clone());
    runtime
        .configure_simulator_relays(urls.clone())
        .await
        .unwrap();
    let partial = runtime.phase1_advance_revision(id).await.unwrap();
    let child = partial
        .retraction()
        .expect("A acceptance permits only A deletion");
    let child_id = *child.draft().draft_id().as_bytes();
    assert_eq!(a.kinds(), vec![1, 5]);
    assert_eq!(b.kinds(), vec![1]);
    assert!(c.kinds().is_empty());
    let parent_policy = Phase1DraftPayload::decode(partial.replacement().draft())
        .unwrap()
        .queue;
    assert_eq!(
        Phase1DraftPayload::decode(child.draft()).unwrap().queue,
        parent_policy
    );
    assert_eq!(
        child
            .push()
            .unwrap()
            .delivery_plan()
            .request()
            .unwrap()
            .target_set()
            .len(),
        2
    );
    let raw = raw_child(&partial);
    assert_eq!(partial.phase(), Phase1RevisionPhase::PartialEffect);
    let attempts = child.push().unwrap().delivery_plan().attempt_count();
    wait_retry(&runtime, child).await;
    let held = runtime
        .phase1_advance_draft(child_id, child.draft().revision().get())
        .await
        .unwrap();
    assert_eq!(
        held.push().unwrap().delivery_plan().attempt_count(),
        attempts
    );
    assert_eq!(a.kinds().iter().filter(|kind| **kind == 5).count(), 1);
    assert_eq!(b.kinds(), vec![1]);
    runtime.shutdown().await.unwrap();
    drop(runtime);

    b.accept_replacement.store(true, Ordering::SeqCst);
    let runtime = persistent_runtime(root.path(), &urls).await;
    let recovered = runtime.phase1_revision_status(id).await.unwrap();
    assert_eq!(raw_child(&recovered), raw);
    wait_retry(&runtime, recovered.replacement()).await;
    wait_retry(&runtime, recovered.retraction().unwrap()).await;
    let complete = runtime.phase1_advance_revision(id).await.unwrap();
    assert_eq!(complete.replacement().state(), Phase1OutboxState::Complete);
    assert_eq!(complete.phase(), Phase1RevisionPhase::Complete);
    assert_eq!(
        complete.retraction().unwrap().state(),
        Phase1OutboxState::Complete
    );
    assert_eq!(raw_child(&complete), raw);
    assert_eq!(a.kinds().iter().filter(|kind| **kind == 5).count(), 1);
    assert_eq!(b.kinds(), vec![1, 1, 5]);
    assert!(c.kinds().is_empty());
    let expected: serde_json::Value = serde_json::from_str(&raw).unwrap();
    assert_eq!(
        a.events
            .lock()
            .unwrap()
            .iter()
            .find(|event| event["kind"] == 5)
            .unwrap(),
        &expected
    );
    assert_eq!(b.events.lock().unwrap().last().unwrap(), &expected);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn cancellation_stops_both_partial_branches_and_direct_child_after_reopen() {
    let root = tempfile::tempdir().unwrap();
    let a = Relay::start(true).await;
    let b = Relay::start(false).await;
    let urls = vec![a.url.clone(), b.url.clone()];
    let runtime = persistent_runtime(root.path(), &urls).await;
    let saved = saved_revision(&runtime).await;
    let id = *saved.replacement().draft().draft_id().as_bytes();
    let partial = runtime.phase1_advance_revision(id).await.unwrap();
    let raw = raw_child(&partial);
    let child_id = *partial.retraction().unwrap().draft().draft_id().as_bytes();
    let stopped = runtime.phase1_cancel_revision(id).await.unwrap();
    assert_eq!(stopped.phase(), Phase1RevisionPhase::PartialEffect);
    assert!(!stopped.can_resume() && !stopped.can_cancel());
    assert_eq!(
        stopped.replacement().draft().stage(),
        AuthoredDraftStage::Cancelled
    );
    assert_eq!(
        stopped.retraction().unwrap().draft().stage(),
        AuthoredDraftStage::Cancelled
    );
    assert_eq!(raw_child(&stopped), raw);
    assert!(
        !stopped
            .retraction()
            .unwrap()
            .push()
            .unwrap()
            .delivery_plan()
            .delivery_facts()
            .is_empty()
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    b.accept_replacement.store(true, Ordering::SeqCst);
    let runtime = persistent_runtime(root.path(), &urls).await;
    let reopened = runtime.phase1_advance_revision(id).await.unwrap();
    assert_eq!(raw_child(&reopened), raw);
    assert!(
        runtime
            .phase1_advance_draft(
                child_id,
                reopened.retraction().unwrap().draft().revision().get()
            )
            .await
            .is_err()
    );
    assert_eq!(a.kinds(), vec![1, 5]);
    assert_eq!(b.kinds(), vec![1]);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn historical_unlinked_revision_reason_is_held_without_rewriting_old_payload() {
    let runtime = signing_runtime();
    let target = super::retraction_support::original(&runtime).await;
    let source = target.source_event_id;
    let card = target.card_id;
    let old = runtime
        .phase1_save_retraction_draft(
            [103; 16],
            AddCommandType::CreateUpdate,
            card,
            &source,
            1,
            None,
            REVISION_RETRACTION_REASON,
            1_900_000_000,
            1_900_000_000_000,
        )
        .await
        .unwrap();
    let bytes = old.draft().payload().to_vec();
    assert!(
        !std::str::from_utf8(&bytes)
            .unwrap()
            .contains("revision_parent_draft_id")
    );
    assert_eq!(
        Phase1DraftPayload::decode(old.draft())
            .unwrap()
            .encode()
            .unwrap(),
        bytes
    );
    assert_eq!(
        runtime
            .phase1_queue_draft(
                [103; 16],
                old.draft().revision().get(),
                policy(),
                1_900_000_000_001,
            )
            .await
            .unwrap_err(),
        Phase1DraftError::InvalidRevision
    );
    assert_eq!(
        runtime
            .phase1_draft_status([103; 16])
            .await
            .unwrap()
            .draft()
            .payload(),
        bytes
    );
    let independent = runtime
        .phase1_save_retraction_draft(
            [104; 16],
            AddCommandType::CreateUpdate,
            card,
            &source,
            1,
            None,
            "I retract this post",
            1_900_000_000,
            1_900_000_000_000,
        )
        .await
        .unwrap();
    let queued = runtime
        .phase1_queue_draft(
            [104; 16],
            independent.draft().revision().get(),
            policy(),
            1_900_000_000_001,
        )
        .await
        .unwrap();
    assert_eq!(queued.state(), Phase1OutboxState::Queued);
    assert!(matches!(
        runtime
            .revision_delivery_selection(queued.draft())
            .await
            .unwrap(),
        RevisionDelivery::Independent
    ));
}

#[tokio::test]
async fn child_queue_rejects_changed_frozen_policy_and_foreign_parent_link() {
    let root = tempfile::tempdir().unwrap();
    let relay = Relay::start(true).await;
    let runtime = persistent_runtime(root.path(), std::slice::from_ref(&relay.url)).await;
    let saved = saved_revision(&runtime).await;
    let id = *saved.replacement().draft().draft_id().as_bytes();
    let parent = runtime
        .phase1_queue_add_intent(
            Phase1QueueIntent::new(id, saved.replacement().draft().revision().get()).unwrap(),
        )
        .await
        .unwrap();
    let child_id = revision_child_id(parent.draft()).unwrap().unwrap();
    let child = runtime
        .phase1_create_revision_retraction(saved.target(), child_id, id)
        .await
        .unwrap();
    assert_eq!(
        runtime
            .phase1_queue_draft(
                child_id,
                child.draft().revision().get(),
                policy(),
                phase1_operation_now_unix_ms().unwrap(),
            )
            .await
            .unwrap_err(),
        Phase1DraftError::InvalidQueuePolicy
    );
    let queued = runtime
        .phase1_queue_add_intent(
            Phase1QueueIntent::new(child_id, child.draft().revision().get()).unwrap(),
        )
        .await
        .unwrap();
    let parent_admission = runtime.mutations.draft(id).unwrap();
    assert_eq!(
        runtime
            .phase1_advance_draft(child_id, queued.draft().revision().get())
            .await
            .unwrap_err(),
        Phase1DraftError::OperationInProgress
    );
    drop(parent_admission);
    assert!(matches!(
        runtime
            .revision_delivery_selection(queued.draft())
            .await
            .unwrap(),
        RevisionDelivery::Held
    ));
    runtime
        .phase1_advance_draft(child_id, queued.draft().revision().get())
        .await
        .unwrap();
    assert!(relay.kinds().is_empty());
    assert!(
        runtime
            .phase1_draft_status(child_id)
            .await
            .unwrap()
            .push()
            .unwrap()
            .artifact()
            .signed()
            .is_none()
    );
    let other = runtime
        .phase1_save_revision_intent(
            Phase1ReviseIntent::new(
                saved.target().clone(),
                Phase1AddCommand::CreateUpdate(CreateUpdate::new("Other").unwrap()),
                vec![],
                Phase1DraftFormSnapshot {
                    content: "Other".into(),
                    ..update_form()
                },
            )
            .unwrap(),
        )
        .await
        .unwrap();
    let mut malformed = Phase1DraftPayload::decode(child.draft()).unwrap();
    malformed.revision_parent_draft_id = Some(*other.replacement().draft().draft_id().as_bytes());
    let invalid = AuthoredDraft::initial(
        child.draft().draft_id(),
        *child.draft().author(),
        DRAFT_PAYLOAD_SCHEMA,
        malformed.encode().unwrap(),
        AuthoredDraftStage::Draft,
        None,
        child.draft().created_at_unix_ms(),
    )
    .unwrap();
    assert_eq!(
        runtime
            .revision_child_queue_policy(&invalid)
            .await
            .unwrap_err(),
        Phase1DraftError::Corrupt
    );
    assert!(relay.kinds().is_empty());
    runtime.shutdown().await.unwrap();
}
