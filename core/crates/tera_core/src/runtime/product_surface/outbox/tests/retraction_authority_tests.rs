use super::{retraction_support::*, *};

#[tokio::test]
async fn retraction_authority_rejects_forged_or_missing_source_without_creating_work() {
    let runtime = signing_runtime();
    let target = original(&runtime).await;
    let missing = "b".repeat(64);
    let cases = [
        (target.card_id, missing.clone(), 1, None),
        (
            CardId::derive(
                TodayCardType::Update,
                &CardSourceIdentity::Event(radroots_event::EventId::parse(&missing).unwrap()),
            ),
            target.source_event_id.clone(),
            1,
            None,
        ),
        (target.card_id, target.source_event_id.clone(), 5, None),
        (
            target.card_id,
            target.source_event_id.clone(),
            1,
            Some(format!("30402:{AUTHOR}:foreign")),
        ),
    ];
    for (index, (card, id, kind, address)) in cases.into_iter().enumerate() {
        let result = runtime
            .phase1_save_retraction_draft(
                [index as u8 + 1; 16],
                AddCommandType::CreateUpdate,
                card,
                &id,
                kind,
                address.as_deref(),
                "Remove",
                1_700_000_001,
                1_700_000_001_000,
            )
            .await;
        assert!(matches!(result, Err(Phase1DraftError::InvalidRevision)));
    }
    assert!(runtime.phase1_draft_heads(20).await.unwrap().is_empty());
    runtime.require_revision_source(&target).await.unwrap();
}

#[tokio::test]
async fn retraction_authority_rejects_other_authors_even_with_matching_native_card_metadata() {
    let runtime = signing_runtime();
    let secret = format!("{:064x}", 2);
    let author = nostr::Keys::parse(&secret)
        .unwrap()
        .public_key()
        .to_string();
    let command = Phase1AddCommand::CreateUpdate(CreateUpdate::new("Other author").unwrap());
    let plan = command
        .authored_plan(1_700_000_000, author.clone())
        .unwrap();
    let event = signed_plan(&plan, &secret);
    super::coordinate_support::retain(&runtime, event.clone()).await;
    let card = card_id(AddCommandType::CreateUpdate, &plan).unwrap();
    let forged = Phase1RevisionTarget::new(
        AddCommandType::CreateUpdate,
        card,
        event.id().to_hex(),
        1,
        None,
        AUTHOR,
    )
    .unwrap();
    assert!(matches!(
        runtime.require_revision_source(&forged).await,
        Err(Phase1DraftError::InvalidRevision)
    ));
    let honest = Phase1RevisionTarget::new(
        AddCommandType::CreateUpdate,
        card,
        event.id().to_hex(),
        1,
        None,
        author,
    )
    .unwrap();
    assert!(matches!(
        runtime.require_revision_source(&honest).await,
        Err(Phase1DraftError::InvalidRevision)
    ));
    assert!(runtime.phase1_draft_heads(20).await.unwrap().is_empty());
}

#[tokio::test]
async fn retraction_authority_preserves_repeated_request_and_signed_plan_identity() {
    let runtime = signing_runtime();
    let target = original(&runtime).await;
    let saved = runtime
        .phase1_save_retraction_draft(
            [201; 16],
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
    let repeated = runtime
        .phase1_save_retraction_draft(
            [201; 16],
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
    assert_eq!(saved.draft(), repeated.draft());
    let plan = PlanWireV1::from_json(
        &Phase1DraftPayload::decode(saved.draft())
            .unwrap()
            .plan_wire_json,
    )
    .unwrap();
    runtime
        .require_publication_source(plan.plan(), Some(saved.draft()))
        .await
        .unwrap();
    assert!(matches!(
        runtime.require_publication_source(plan.plan(), None).await,
        Err(Phase1DraftError::InvalidRevision)
    ));
    // An old payload must not smuggle an additional target behind an honest card.
    let request = AuthoredNip09DeletionRequest::new(
        "Remove",
        vec![
            Nip09DeletionEventTarget::parse(&target.source_event_id, 1).unwrap(),
            Nip09DeletionEventTarget::parse("b".repeat(64), 1).unwrap(),
        ],
        vec![],
    )
    .unwrap();
    let extra = phase1_retraction_plan(&request, 1_700_000_001, AUTHOR).unwrap();
    let payload = Phase1DraftPayload::retraction(
        target.command_type,
        target.card_id,
        PlanWireV1::from_plan(&extra).to_json().unwrap(),
    )
    .unwrap();
    let head = AuthoredDraft::initial(
        AuthoredDraftId::new([205; 16]).unwrap(),
        *extra.author().as_bytes(),
        DRAFT_PAYLOAD_SCHEMA,
        payload.encode().unwrap(),
        AuthoredDraftStage::Draft,
        None,
        1_700_000_001_000,
    )
    .unwrap();
    assert_eq!(
        runtime
            .require_publication_source(&extra, Some(&head))
            .await
            .unwrap_err(),
        Phase1DraftError::InvalidRevision
    );
}
