use super::revision_delivery_support::*;
use super::*;
use std::sync::atomic::Ordering;

#[tokio::test]
async fn revision_status_lost_child_ack_survives_stop_and_reopen_without_false_completion() {
    let root = tempfile::tempdir().unwrap();
    let a = Relay::start(true).await;
    let b = Relay::start(true).await;
    b.accept_retraction.store(false, Ordering::SeqCst);
    let urls = vec![a.url.clone(), b.url.clone()];
    let runtime = persistent_runtime(root.path(), &urls).await;
    let saved = saved_revision(&runtime).await;
    let id = *saved.replacement().draft().draft_id().as_bytes();
    let partial = runtime.phase1_advance_revision(id).await.unwrap();
    assert_eq!(partial.phase(), Phase1RevisionPhase::PartialEffect);
    assert_eq!(partial.replacement().state(), Phase1OutboxState::Complete);
    let child = partial.retraction().unwrap();
    let child_id = *child.draft().draft_id().as_bytes();
    assert_eq!(child.revision_parent_draft_id(), Some(id));
    let summary = Phase1DraftSummary::from(child.clone());
    assert_eq!(summary.revision_parent_draft_id(), Some(id));
    let targets = partial
        .retraction_progress()
        .unwrap()
        .targets
        .as_ref()
        .unwrap();
    assert!(targets.targets[0].accepted);
    assert!(targets.targets[1].uncertain && !targets.targets[1].accepted);
    assert_eq!(a.kinds(), vec![1, 5]);
    assert_eq!(b.kinds(), vec![1, 5]); // receiving bytes is not an ACK
    let raw = raw_child(&partial);
    runtime
        .phase1_cancel_add_intent(child_id, child.draft().revision().get())
        .await
        .unwrap();
    let stopped = runtime.phase1_advance_revision(id).await.unwrap();
    assert_eq!(stopped.phase(), Phase1RevisionPhase::PartialEffect);
    assert!(stopped.retraction_progress().unwrap().stopped);
    assert!(!stopped.can_resume() && !stopped.can_cancel());
    assert_eq!(raw_child(&stopped), raw);
    runtime.shutdown().await.unwrap();
    drop(runtime);
    b.accept_retraction.store(true, Ordering::SeqCst);
    let runtime = persistent_runtime(root.path(), &urls).await;
    let recovered = runtime.phase1_advance_revision(id).await.unwrap();
    assert_eq!(recovered.phase(), Phase1RevisionPhase::PartialEffect);
    assert!(!recovered.can_resume() && recovered.retraction_progress().unwrap().stopped);
    assert_eq!(raw_child(&recovered), raw);
    let cancelled_again = runtime.phase1_cancel_revision(id).await.unwrap();
    assert_eq!(raw_child(&cancelled_again), raw);
    assert_eq!(a.kinds(), vec![1, 5]);
    assert_eq!(b.kinds(), vec![1, 5]);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn revision_status_unknown_replacement_is_partial_and_pre_effect_stop_is_cancelled() {
    let root = tempfile::tempdir().unwrap();
    let relay = Relay::start(false).await;
    let urls = vec![relay.url.clone()];
    let runtime = persistent_runtime(root.path(), &urls).await;
    let saved = saved_revision(&runtime).await;
    assert!(saved.can_resume() && saved.can_cancel());
    let id = *saved.replacement().draft().draft_id().as_bytes();
    let unknown = runtime.phase1_advance_revision(id).await.unwrap();
    assert_eq!(unknown.phase(), Phase1RevisionPhase::PartialEffect);
    assert!(unknown.retraction().is_none());
    let stopped = runtime.phase1_cancel_revision(id).await.unwrap();
    assert_eq!(stopped.phase(), Phase1RevisionPhase::PartialEffect);
    assert!(stopped.replacement_progress().stopped && !stopped.can_resume());
    assert!(
        stopped
            .replacement_progress()
            .targets
            .as_ref()
            .unwrap()
            .targets[0]
            .uncertain
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = persistent_runtime(root.path(), &urls).await;
    let restored = runtime.phase1_revision_status(id).await.unwrap();
    assert_eq!(restored.phase(), Phase1RevisionPhase::PartialEffect);
    assert!(!restored.can_resume() && !restored.can_cancel());
    runtime.shutdown().await.unwrap();

    let untouched = signing_runtime();
    let saved = saved_revision(&untouched).await;
    let id = *saved.replacement().draft().draft_id().as_bytes();
    let stopped = untouched.phase1_cancel_revision(id).await.unwrap();
    assert_eq!(stopped.phase(), Phase1RevisionPhase::Cancelled);
    assert!(stopped.replacement_progress().targets.is_none());
    assert!(!stopped.can_resume() && !stopped.can_cancel());
}

#[tokio::test]
async fn revision_status_resumes_signed_replacement_and_child_after_reopen() {
    let root = tempfile::tempdir().unwrap();
    let relay = Relay::start(true).await;
    let urls = vec![relay.url.clone()];
    let runtime = persistent_runtime(root.path(), &urls).await;
    let saved = saved_revision(&runtime).await;
    let id = *saved.replacement().draft().draft_id().as_bytes();
    let queued = runtime
        .phase1_queue_add_intent(
            Phase1QueueIntent::new(id, saved.replacement().draft().revision().get()).unwrap(),
        )
        .await
        .unwrap();
    let signed = runtime
        .phase1_sign_queued_draft(id, queued.draft().revision().get())
        .await
        .unwrap();
    assert_eq!(signed.state(), Phase1OutboxState::Signed);
    assert!(
        runtime
            .phase1_revision_status(id)
            .await
            .unwrap()
            .can_resume()
    );
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = persistent_runtime(root.path(), &urls).await;
    // Finish replacement alone, then simulate process loss after child signing.
    runtime
        .phase1_advance_draft(id, signed.draft().revision().get())
        .await
        .unwrap();
    let pending = runtime.phase1_revision_status(id).await.unwrap();
    assert_eq!(pending.phase(), Phase1RevisionPhase::RetractionPending);
    let child_id = revision_child_id(pending.replacement().draft())
        .unwrap()
        .unwrap();
    let child = runtime
        .phase1_create_revision_retraction(pending.target(), child_id, id)
        .await
        .unwrap();
    let child = runtime
        .phase1_queue_add_intent(
            Phase1QueueIntent::new(child_id, child.draft().revision().get()).unwrap(),
        )
        .await
        .unwrap();
    let child = runtime
        .phase1_sign_queued_draft(child_id, child.draft().revision().get())
        .await
        .unwrap();
    assert_eq!(child.state(), Phase1OutboxState::Signed);
    let status = runtime.phase1_revision_status(id).await.unwrap();
    assert!(status.can_resume());
    let raw = raw_child(&status);
    runtime.shutdown().await.unwrap();
    drop(runtime);
    let runtime = persistent_runtime(root.path(), &urls).await;
    let completed = runtime.phase1_advance_revision(id).await.unwrap();
    assert_eq!(completed.phase(), Phase1RevisionPhase::Complete);
    assert!(!completed.can_resume() && !completed.can_cancel());
    assert_eq!(raw_child(&completed), raw);
    assert_eq!(relay.kinds(), vec![1, 5]);
    runtime.shutdown().await.unwrap();
}
