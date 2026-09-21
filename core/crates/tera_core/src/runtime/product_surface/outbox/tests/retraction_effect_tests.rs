use super::{coordinate_effect_support as effects, retraction_support::*, *};
use radroots_storage::EventStore;
use std::{sync::atomic::Ordering, time::Duration};

#[tokio::test]
async fn retained_revision_without_source_is_readable_and_stoppable_but_cannot_resume() {
    let first = signing_runtime();
    let saved = super::revision_delivery_support::saved_revision(&first).await;
    let recovered = signing_runtime();
    recovered
        .storage()
        .unwrap()
        .append_authored_draft(saved.replacement().draft().clone(), None)
        .await
        .unwrap();
    let id = *saved.replacement().draft().draft_id().as_bytes();
    let status = recovered.phase1_revision_status(id).await.unwrap();
    assert_eq!(status.replacement().draft(), saved.replacement().draft());
    assert!(!status.can_resume());
    assert!(status.can_cancel());
    let held = recovered.phase1_advance_revision(id).await.unwrap();
    assert_eq!(held.replacement().draft(), status.replacement().draft());
    assert!(!held.can_resume());
    assert!(
        recovered
            .phase1_draft_status(id)
            .await
            .unwrap()
            .push()
            .is_none()
    );
    let stopped = recovered.phase1_cancel_revision(id).await.unwrap();
    assert_eq!(stopped.phase(), Phase1RevisionPhase::Cancelled);
}

#[tokio::test]
async fn retained_unproven_retractions_cannot_sign_admit_suppress_or_deliver() {
    for already_signed in [false, true] {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay = format!("ws://{}", listener.local_addr().unwrap());
        let signer = effects::PausedSigner::new();
        signer.pause.store(false, Ordering::SeqCst);
        let runtime = effects::runtime(signer.clone(), &relay);
        let target = original_target(&Phase1AddCommand::CreateUpdate(
            CreateUpdate::new("Original harvest").unwrap(),
        ));
        let saved = {
            // Reproduce a historical capture that predated source authorization.
            let permit = runtime.mutations.draft([202; 16]).unwrap();
            runtime
                .phase1_save_retraction_draft_admitted(
                    &permit,
                    [202; 16],
                    target.command_type,
                    target.card_id,
                    &target.source_event_id,
                    1,
                    None,
                    "Remove",
                    1_700_000_001,
                    1_700_000_001_000,
                    None,
                )
                .await
                .unwrap()
        };
        let policy = Phase1QueuePolicy::new(
            vec![relay],
            Phase1RelaySatisfaction::AllAccepted,
            2_000_000_000_000,
            Phase1CancellationPolicy::LocalCooperative,
        )
        .unwrap();
        let queued = runtime
            .phase1_queue_draft(
                [202; 16],
                saved.draft().revision().get(),
                policy,
                1_700_000_001_001,
            )
            .await
            .unwrap();
        let before = queued.draft().clone();
        if already_signed {
            // Reproduce an old signed artifact without granting it new admission.
            runtime
                .sync()
                .unwrap()
                .sign_prepared(push_request(&before).unwrap())
                .await
                .unwrap();
        }
        assert_eq!(
            runtime
                .phase1_sign_queued_draft([202; 16], before.revision().get())
                .await
                .unwrap_err(),
            Phase1DraftError::InvalidRevision
        );
        assert_eq!(
            runtime
                .phase1_advance_draft([202; 16], before.revision().get())
                .await
                .unwrap_err(),
            Phase1DraftError::InvalidRevision
        );
        let after = runtime.phase1_draft_status([202; 16]).await.unwrap();
        assert_eq!(after.draft(), &before);
        let push = after.push().unwrap();
        assert_eq!(push.artifact().signed().is_some(), already_signed);
        assert!(!push.artifact().admission_state().is_admitted());
        assert!(push.delivery_plan().attempts().is_empty());
        assert_eq!(
            signer.calls.load(Ordering::SeqCst),
            usize::from(already_signed)
        );
        assert_eq!(
            EventStore::status(runtime.client.storage().unwrap())
                .await
                .unwrap()
                .raw_events(),
            0
        );
        assert!(
            tokio::time::timeout(Duration::from_millis(50), listener.accept())
                .await
                .is_err()
        );
        runtime
            .phase1_cancel_draft([202; 16], before.revision().get(), 1_700_000_001_002)
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn locally_retracted_source_remains_proof_for_a_repeated_request() {
    let runtime = signing_runtime();
    let target = original_target(&Phase1AddCommand::CreateUpdate(
        CreateUpdate::new("Original harvest").unwrap(),
    ));
    let source = runtime
        .phase1_save_draft(
            [200; 16],
            Phase1AddCommand::CreateUpdate(CreateUpdate::new("Original harvest").unwrap()),
            1_700_000_000,
            vec![],
            None,
            1_700_000_000_000,
        )
        .await
        .unwrap();
    let source = runtime
        .phase1_queue_draft(
            [200; 16],
            source.draft().revision().get(),
            policy(),
            1_700_000_000_001,
        )
        .await
        .unwrap();
    let source = runtime
        .phase1_sign_queued_draft([200; 16], source.draft().revision().get())
        .await
        .unwrap();
    runtime
        .sync()
        .unwrap()
        .admit_signed(sync_id_for(source.draft()).unwrap())
        .await
        .unwrap();
    let visible = EventStore::rebuild_visibility(runtime.client.storage().unwrap())
        .await
        .unwrap();
    assert!(
        visible
            .visible_event_ids()
            .iter()
            .any(|id| id.to_hex() == target.source_event_id)
    );
    let saved = runtime
        .phase1_save_retraction_draft(
            [203; 16],
            target.command_type,
            target.card_id,
            &target.source_event_id,
            1,
            None,
            "Remove",
            1_700_000_001,
            1_700_000_001_000,
        )
        .await
        .unwrap();
    let queued = runtime
        .phase1_queue_draft(
            [203; 16],
            saved.draft().revision().get(),
            policy(),
            1_700_000_001_001,
        )
        .await
        .unwrap();
    let signed = runtime
        .phase1_sign_queued_draft([203; 16], queued.draft().revision().get())
        .await
        .unwrap();
    runtime
        .sync()
        .unwrap()
        .admit_signed(sync_id_for(signed.draft()).unwrap())
        .await
        .unwrap();
    let suppressed = EventStore::rebuild_visibility(runtime.client.storage().unwrap())
        .await
        .unwrap();
    assert!(
        suppressed
            .suppressed_event_ids()
            .iter()
            .any(|id| id.to_hex() == target.source_event_id)
    );
    assert!(
        !suppressed
            .visible_event_ids()
            .iter()
            .any(|id| id.to_hex() == target.source_event_id)
    );
    runtime.require_revision_source(&target).await.unwrap();
    let repeated = runtime
        .phase1_save_retraction_draft(
            [204; 16],
            target.command_type,
            target.card_id,
            &target.source_event_id,
            1,
            None,
            "Remove",
            1_700_000_001,
            1_700_000_001_000,
        )
        .await
        .unwrap();
    let first = PlanWireV1::from_json(
        &Phase1DraftPayload::decode(saved.draft())
            .unwrap()
            .plan_wire_json,
    )
    .unwrap();
    let second = PlanWireV1::from_json(
        &Phase1DraftPayload::decode(repeated.draft())
            .unwrap()
            .plan_wire_json,
    )
    .unwrap();
    assert_eq!(first.plan(), second.plan());
}
