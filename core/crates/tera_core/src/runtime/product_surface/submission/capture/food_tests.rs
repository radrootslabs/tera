use super::{reservation, *};
use crate::runtime::product_surface::{
    CardLifecycleState, ContextAdmission, ContextRank, LocalNetworkAdmission,
    ProductEventClassification, classify_admitted_event,
};
use nostr::secp256k1::{Keypair, Message, SECP256K1};
use radroots_event::{Event, envelope::EventEnvelopeParts, wire::compute_canonical_nip01_event_id};
use radroots_event_codec::{
    admission::{RadrootsAdmittedEvent, admit_verified_event},
    authoring::AuthoredEventPlan,
    verify::verify_nip01_event,
};

fn food() -> ComposerFormInput {
    let mut value = input(AddCommandType::CreateFoodAvailability);
    value.identifier = Some("carrots".into());
    value.title = Some("Carrots".into());
    value.summary = Some("Fresh".into());
    value.location = Some("Victoria".into());
    value.price_amount = Some("12345678901234567890.12345678".into());
    value.currency = Some("ZZZ".into()); // Shape is not a finance registry assertion.
    value.unit = Some("bunch".into());
    value.quantity = Some("0.000000000000000000000000001".into());
    value.food_status = Some("sold".into());
    value
}

fn capture(value: ComposerFormInput) -> Result<CapturedSubmission, SubmissionCaptureError> {
    CapturedSubmission::capture(
        reservation(value),
        policy("wss://relay.example"),
        None,
        vec![],
    )
}

fn signed(
    plan: &AuthoredEventPlan,
    tags: Vec<Vec<String>>,
) -> radroots_event_codec::verify::RadrootsSignatureVerifiedEvent {
    let keys = nostr::Keys::parse(&format!("{:064x}", 1)).unwrap();
    let author = plan.author().to_hex();
    let id = compute_canonical_nip01_event_id(
        &author,
        plan.created_at(),
        plan.body().kind(),
        &tags,
        plan.body().content(),
    )
    .unwrap();
    let signature = SECP256K1.sign_schnorr_no_aux_rand(
        &Message::from_digest(*id.as_bytes()),
        &Keypair::from_secret_key(SECP256K1, keys.secret_key()),
    );
    verify_nip01_event(
        Event::new(EventEnvelopeParts {
            id: id.to_hex(),
            author,
            created_at: plan.created_at(),
            kind: plan.body().kind(),
            tags,
            content: plan.body().content().into(),
            sig: signature.to_string(),
        })
        .unwrap(),
    )
    .unwrap()
}

#[test]
fn food_exact_capture_shared_admission_and_sold_projection_round_trip() {
    let input = food();
    let captured = capture(input.clone()).unwrap();
    assert_eq!(captured.reservation().captured().form().input(), &input);
    let plan = captured.plan();
    let admitted = admit_verified_event(signed(plan, plan.body().tags().to_vec())).unwrap();
    let RadrootsAdmittedEvent::FoodAvailability(event) = &admitted else {
        panic!("food contract required")
    };
    assert_eq!(
        event.projection().price().amount(),
        input.price_amount.as_deref().unwrap()
    );
    assert_eq!(event.projection().price().currency().as_str(), "ZZZ");
    assert_eq!(
        event.projection().quantity().unwrap().amount(),
        input.quantity.as_deref().unwrap()
    );
    let ProductEventClassification::Card(card) = classify_admitted_event(
        &admitted,
        LocalNetworkAdmission::Included(ContextAdmission {
            rank: ContextRank::MissingLocalityFallback,
            reason: "locality_missing_fallback",
        }),
    ) else {
        panic!("food card required")
    };
    assert_eq!(card.price_amount, input.price_amount);
    assert_eq!(card.quantity, input.quantity);
    assert_eq!(card.price_currency.as_deref(), Some("ZZZ"));
    assert_eq!(card.price_unit.as_deref(), Some("bunch"));
    assert_eq!(card.food_status.as_deref(), Some("sold"));
    assert_eq!(card.lifecycle, CardLifecycleState::Sold);
}

#[test]
fn food_invalid_decimals_currency_unit_status_and_zero_quantity_fail_shared_capture() {
    for amount in [
        "",
        "01",
        "1.0",
        "1,5",
        "1e2",
        "-1",
        "+1",
        ".5",
        "1.",
        "12345678901234567890123456789",
    ] {
        let mut value = food();
        value.price_amount = Some(amount.into());
        assert!(capture(value).is_err(), "{amount}");
    }
    for (field, invalid) in [
        ("quantity", "0"),
        ("quantity", "1.00"),
        ("currency", "cad"),
        ("currency", "EURO"),
        ("unit", "crate"),
        ("status", "reserved"),
    ] {
        let mut value = food();
        match field {
            "quantity" => value.quantity = Some(invalid.into()),
            "currency" => value.currency = Some(invalid.into()),
            "unit" => value.unit = Some(invalid.into()),
            "status" => value.food_status = Some(invalid.into()),
            _ => unreachable!(),
        }
        assert!(capture(value).is_err(), "{field}={invalid}");
    }
    let mut free = food();
    free.price_amount = Some("0".into());
    free.quantity = None;
    free.food_status = None;
    let captured = capture(free).unwrap();
    let admitted = admit_verified_event(signed(
        captured.plan(),
        captured.plan().body().tags().to_vec(),
    ))
    .unwrap();
    let RadrootsAdmittedEvent::FoodAvailability(event) = admitted else {
        panic!("food required")
    };
    assert_eq!(event.projection().price().amount(), "0");
    assert!(event.projection().quantity().is_none());
    assert_eq!(event.projection().status().as_str(), "active");
}

#[test]
fn food_signed_unit_mismatch_is_rejected_before_typed_projection() {
    let captured = capture(food()).unwrap();
    let mut tags = captured.plan().body().tags().to_vec();
    let quantity = tags
        .iter_mut()
        .find(|tag| tag.first().map(String::as_str) == Some("radroots:quantity"))
        .unwrap();
    assert_eq!(quantity[2], "bunch");
    quantity[2] = "bag".into();
    assert!(admit_verified_event(signed(captured.plan(), tags)).is_err());
}

#[tokio::test]
async fn food_identical_announcements_keep_distinct_intent_and_coordinate_identity() {
    use super::super::super::repository::SubmissionRepository;
    use crate::runtime::product_surface::{
        ComposerId, ComposerRevision, SubmissionCommandId, SubmissionReservationRequest,
    };
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let store = client.storage().unwrap();
    let repo = SubmissionRepository { store };
    let mut receipts = Vec::new();
    let mut event_ids = Vec::new();
    for number in [1, 2] {
        let request = SubmissionReservationRequest::new(
            SubmissionCommandId::new([number; 16]).unwrap(),
            scope(AUTHOR, "nearby"),
            ComposerId::new([number; 16]).unwrap(),
            ComposerRevision::INITIAL,
        );
        let mut value = food();
        value.identifier = Some(format!("announcement-{number}"));
        let source = ComposerStorageRecord::initial(
            request.composer_id(),
            request.scope().clone(),
            ComposerEditSequence::INITIAL,
            ComposerPartialForm::new(value).unwrap(),
            NOW,
        )
        .unwrap();
        store
            .append_authored_draft(source.into_stored(), None)
            .await
            .unwrap();
        let reservation = repo.reserve(&request, Some(NOW)).await.unwrap();
        let captured =
            CapturedSubmission::capture(reservation, policy("wss://relay.example"), None, vec![])
                .unwrap();
        event_ids.push(captured.plan().expected_event_id().to_hex());
        let receipt = repo.commit(&captured).await.unwrap();
        let replay = repo.commit(&captured).await.unwrap();
        assert_eq!(replay.operation_id(), receipt.operation_id());
        assert!(replay.is_replay());
        receipts.push(receipt);
    }
    assert_ne!(receipts[0].intent_id(), receipts[1].intent_id());
    assert_ne!(receipts[0].operation_id(), receipts[1].operation_id());
    assert_ne!(event_ids[0], event_ids[1]);
}
