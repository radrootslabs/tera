use super::{coordinate_support::*, *};
use crate::runtime::product_surface::coordinate::{intent_from_draft, plan_from_draft};
use radroots_event::SignedEvent;

const AT: u64 = 1_750_000_000;

fn equal_second_intent(prior: &SignedEvent, wins: bool) -> Phase1ReviseIntent {
    (0..128)
        .map(|index| intent(prior, &format!("Equal-second market {index}")))
        .find(|value| {
            let plan = value.command.authored_plan(AT, AUTHOR).unwrap();
            (*plan.expected_event_id() < *prior.id()) == wins
        })
        .expect("deterministic fixture has both canonical ID orders")
}

#[tokio::test]
async fn coordinate_equal_second_uses_shared_order_without_rewriting_capture_or_broad_delete() {
    for wins in [false, true] {
        let runtime = signing_runtime();
        let prior = signed_head(SECRET, 31_923, "equal:second", AT);
        retain(&runtime, prior.clone()).await;
        let captured = equal_second_intent(&prior, wins);
        let first = runtime
            .prepare_revision_intent_with_clock([201; 16], captured.clone(), || Ok(AT * 1_000))
            .await;
        assert_eq!(first.is_ok(), wins);
        if let Err(error) = first {
            assert_eq!(error, Phase1DraftError::RevisionConflict);
        }
        let replay = runtime
            .prepare_revision_intent_with_clock([201; 16], captured, || Ok((AT + 100) * 1_000))
            .await
            .unwrap();
        let draft = replay.replacement();
        assert_eq!(draft.coordinate_writable(), wins);
        assert_eq!(
            plan_from_draft(draft.draft()).unwrap().unwrap().created_at,
            AT
        );
        assert!(replay.retraction().is_none());
        if wins {
            let queued = queue(&runtime, draft).await.unwrap();
            let signed = runtime
                .phase1_sign_queued_draft(
                    *queued.draft().draft_id().as_bytes(),
                    queued.draft().revision().get(),
                )
                .await
                .unwrap();
            let event = signed.push().unwrap().artifact().signed().unwrap().event();
            assert_eq!(event.envelope().created_at_u64(), AT);
            assert!(event.id() < prior.id());
            assert_eq!(event.envelope().kind_u32(), 31_923);
        } else {
            assert_eq!(
                queue(&runtime, draft).await.unwrap_err(),
                Phase1DraftError::RevisionConflict
            );
            assert!(draft.push().is_none());
        }
        assert_eq!(runtime.phase1_draft_heads(100).await.unwrap().len(), 1);
    }
}

#[tokio::test]
async fn coordinate_clock_rollback_and_older_capture_hold_without_changing_the_saved_event() {
    let runtime = runtime();
    let prior = signed_head(SECRET, 31_923, "rollback:clock", AT);
    retain(&runtime, prior.clone()).await;
    let captured = intent(&prior, "Older clock");
    assert_eq!(
        runtime
            .prepare_revision_intent_with_clock(
                [202; 16],
                captured.clone(),
                || Ok((AT - 1) * 1_000)
            )
            .await
            .unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
    let held = runtime
        .prepare_revision_intent_with_clock([202; 16], captured, || Ok((AT + 10_000) * 1_000))
        .await
        .unwrap();
    let draft = held.replacement().draft();
    let capture = plan_from_draft(draft).unwrap().unwrap();
    assert_eq!(capture.created_at, AT - 1);
    assert!(!held.can_resume());
    assert!(
        !runtime
            .coordinate_known_winner_matches_at(&capture.intent, AT + 10_000)
            .await
            .unwrap()
    );
    assert!(held.retraction().is_none());

    let fresh = saved(&runtime, [203; 16], "rollback:new").await;
    let original = fresh.draft().clone();
    let capture = intent_from_draft(&original).unwrap().unwrap();
    assert!(
        !runtime
            .coordinate_known_winner_matches_at(&capture, AT - 1)
            .await
            .unwrap()
    );
    assert!(
        runtime
            .coordinate_known_winner_matches_at(&capture, AT)
            .await
            .unwrap()
    );
    assert!(
        runtime
            .coordinate_known_winner_matches_at(&capture, AT + 1_000_000)
            .await
            .unwrap()
    );
    assert_eq!(
        runtime
            .phase1_draft_status([203; 16])
            .await
            .unwrap()
            .draft(),
        &original
    );
}

#[tokio::test]
async fn coordinate_received_equal_second_winner_reconciles_without_reviving_the_old_capture() {
    let runtime = signing_runtime();
    let prior = signed_head(SECRET, 31_923, "received:winner", AT);
    retain(&runtime, prior.clone()).await;
    let request = equal_second_intent(&prior, true);
    let prepared = runtime
        .prepare_revision_intent_with_clock([204; 16], request.clone(), || Ok(AT * 1_000))
        .await
        .unwrap();
    let queued = queue(&runtime, prepared.replacement()).await.unwrap();
    let id = *queued.draft().draft_id().as_bytes();
    let signed = runtime
        .phase1_sign_queued_draft(id, queued.draft().revision().get())
        .await
        .unwrap();
    let artifact = signed.push().unwrap().artifact().signed().unwrap().clone();
    retain(&runtime, artifact.event().clone()).await;
    let intent = intent_from_draft(signed.draft()).unwrap().unwrap();
    assert!(runtime.coordinate_is_current(&intent).await.unwrap());
    let remote = (0..128)
        .map(|index| {
            signed_head_content(
                SECRET,
                31_923,
                "received:winner",
                AT,
                &format!("Remote winner {index}"),
            )
        })
        .find(|event| event.id() < artifact.event().id())
        .expect("bounded deterministic lower ID fixture");
    retain(&runtime, remote.clone()).await;
    let replay = runtime
        .prepare_revision_intent_with_clock([204; 16], request, || Ok((AT + 500) * 1_000))
        .await
        .unwrap();
    assert_eq!(replay.replacement().draft(), signed.draft());
    assert!(!replay.can_resume());
    assert!(!runtime.coordinate_is_current(&intent).await.unwrap());
    assert_eq!(
        replay
            .replacement()
            .push()
            .unwrap()
            .artifact()
            .signed()
            .unwrap(),
        &artifact
    );
    assert_eq!(
        runtime
            .phase1_sign_queued_draft(id, signed.draft().revision().get())
            .await
            .unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
    let snapshot =
        radroots_storage::event::EventStore::rebuild_visibility(runtime.client.storage().unwrap())
            .await
            .unwrap();
    assert_eq!(snapshot.current_heads().len(), 1);
    assert_eq!(snapshot.current_heads()[0].event_id, *remote.id());
    assert!(replay.retraction().is_none());
}

#[tokio::test]
async fn coordinate_event_timestamp_policy_never_rewrites_civil_calendar_dates() {
    let runtime = runtime();
    let identifier = "civil:date";
    let prior = signed_head(SECRET, 31_922, identifier, AT - 10);
    retain(&runtime, prior.clone()).await;
    let event = AuthoredCalendarDateEvent::new(
        identifier,
        "All-day market",
        CalendarDate::parse("2026-12-31").unwrap(),
    )
    .unwrap();
    let target = Phase1RevisionTarget::from_source(
        AddCommandType::CreateEvent,
        CardId::derive(
            TodayCardType::Event,
            &CardSourceIdentity::address(31_922, AUTHOR, identifier).unwrap(),
        ),
        prior.id().to_hex(),
        Some(format!("31922:{AUTHOR}:{identifier}")),
        AUTHOR,
    )
    .unwrap();
    let form = Phase1DraftFormSnapshot {
        command_type: AddCommandType::CreateEvent,
        content: String::new(),
        identifier: Some(identifier.into()),
        title: Some("All-day market".into()),
        event_timing: Some(Phase1DraftEventTiming::AllDay),
        event_start_date: Some("2026-12-31".into()),
        ..update_form()
    };
    let request = Phase1ReviseIntent::new(
        target,
        Phase1AddCommand::CreateEvent(CreateEvent::date(event)),
        vec![],
        form.clone(),
    )
    .unwrap();
    let prepared = runtime
        .prepare_revision_intent_with_clock([205; 16], request.clone(), || Ok(AT * 1_000))
        .await
        .unwrap();
    let capture = plan_from_draft(prepared.replacement().draft())
        .unwrap()
        .unwrap();
    assert!(
        !runtime
            .coordinate_known_winner_matches_at(&capture.intent, AT - 1)
            .await
            .unwrap()
    );
    let replay = runtime
        .prepare_revision_intent_with_clock([205; 16], request, || Ok((AT + 1_000_000) * 1_000))
        .await
        .unwrap();
    assert_eq!(replay.replacement().draft(), prepared.replacement().draft());
    assert_eq!(replay.replacement().form(), Some(&form));
    assert_eq!(capture.created_at, AT);
    assert!(replay.retraction().is_none());
}
