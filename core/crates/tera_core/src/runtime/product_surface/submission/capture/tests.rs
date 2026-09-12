use super::super::test_support::*;
use super::*;
use crate::runtime::product_surface::{
    AddCommandType, ComposerDraft, ComposerEditSequence, ComposerFormInput, ComposerPartialForm,
    ComposerStorageRecord, Phase1DraftEventTiming,
};
use radroots_sdk::transport::BlossomConfig;

#[path = "runtime_tests.rs"]
mod runtime_tests;

fn reservation(input: ComposerFormInput) -> SubmissionReservationReceipt {
    let request = request();
    let source = ComposerStorageRecord::initial(
        request.composer_id(),
        request.scope().clone(),
        ComposerEditSequence::INITIAL,
        ComposerPartialForm::new(input).unwrap(),
        NOW,
    )
    .unwrap();
    SubmissionReservationReceipt {
        reservation_id: super::super::record::reservation_id(&request).unwrap(),
        captured: source.draft().clone(),
        source: radroots_storage::authored_draft_submission::AuthoredDraftSource::capture(
            source.stored(),
        )
        .unwrap(),
        request,
        reserved_at_unix_ms: NOW,
        replayed: false,
    }
}

#[test]
fn all_five_families_capture_shared_plans_and_both_calendar_profiles() {
    let mut cases = vec![
        input(AddCommandType::CreateUpdate),
        input(AddCommandType::CreateAsk),
    ];
    for timing in [
        Phase1DraftEventTiming::AllDay,
        Phase1DraftEventTiming::Timed,
    ] {
        let mut event = input(AddCommandType::CreateEvent);
        event.identifier = Some("harvest".into());
        event.title = Some("Harvest".into());
        event.event_timing = Some(timing);
        event.event_start_date = Some("2026-09-12".into());
        event.event_end_date = Some("2026-09-13".into());
        event.event_start_unix_s = Some(NOW / 1000);
        event.event_end_unix_s = Some(NOW / 1000 + 3600);
        event.event_timezone = Some("America/Vancouver".into());
        cases.push(event);
    }
    let mut food = input(AddCommandType::CreateFoodAvailability);
    food.identifier = Some("carrots".into());
    food.title = Some("Carrots".into());
    food.summary = Some("Fresh".into());
    food.location = Some("Victoria".into());
    food.price_amount = Some("4.5".into());
    food.currency = Some("CAD".into());
    food.unit = Some("bunch".into());
    food.quantity = Some("12".into());
    cases.push(food);
    cases.push(input(AddCommandType::CreatePhotoUpdate));
    let slot = blossom();
    for mut input in cases {
        let bytes = if input.command_type == AddCommandType::CreatePhotoUpdate {
            let (media, bytes) = photo();
            input.media.push(media);
            vec![bytes]
        } else {
            vec![]
        };
        let kind = input.command_type;
        let captured = CapturedSubmission::capture(
            reservation(input),
            policy("wss://relay.example"),
            Some(&slot),
            bytes,
        )
        .unwrap();
        assert_eq!(captured.command().command_type(), kind);
        assert_eq!(captured.plan().author().to_string(), AUTHOR);
        assert_eq!(captured.plan().created_at(), NOW / 1000);
        assert!(!format!("{captured:?}").contains("PRIVATE"));
    }
}

#[test]
fn captured_values_and_semantic_equality_survive_edits_and_replay_observation() {
    let mut live = input(AddCommandType::CreateUpdate);
    let original = reservation(live.clone());
    let captured = CapturedSubmission::capture(
        original.clone(),
        policy("wss://relay.example"),
        None,
        vec![],
    )
    .unwrap();
    live.content = "later content".into();
    let mut replay = original.clone();
    replay.replayed = true;
    let equal =
        CapturedSubmission::capture(replay, policy("wss://relay.example"), None, vec![]).unwrap();
    assert!(captured.same_request(&equal));
    assert_eq!(
        captured.reservation().captured().form().input().content,
        "PRIVATE harvest café"
    );
    let changed = CapturedSubmission::capture(
        reservation(live),
        policy("wss://relay.example"),
        None,
        vec![],
    )
    .unwrap();
    assert!(!captured.same_request(&changed));
    let changed_policy = CapturedSubmission::capture(
        original.clone(),
        policy("wss://other.example"),
        None,
        vec![],
    )
    .unwrap();
    assert!(!captured.same_request(&changed_policy));
    let mut other = original;
    other.request.scope = scope(OTHER, "nearby");
    other.captured = ComposerDraft::new(
        other.captured.id(),
        other.captured.revision(),
        other.request.scope.clone(),
        other.captured.edit_sequence(),
        other.captured.form().clone(),
    );
    let changed_author =
        CapturedSubmission::capture(other, policy("wss://relay.example"), None, vec![]).unwrap();
    assert!(!captured.same_request(&changed_author));
    assert_eq!(captured.plan().author().to_string(), AUTHOR);
}

#[test]
fn strict_incomplete_fields_fail_without_changing_the_captured_form() {
    for kind in [
        AddCommandType::CreateUpdate,
        AddCommandType::CreatePhotoUpdate,
        AddCommandType::CreateAsk,
        AddCommandType::CreateEvent,
        AddCommandType::CreateFoodAvailability,
    ] {
        let reservation = reservation(ComposerFormInput::empty(kind));
        let before = reservation.captured().clone();
        assert!(
            CapturedSubmission::capture(
                reservation.clone(),
                policy("wss://relay.example"),
                None,
                vec![]
            )
            .is_err()
        );
        assert_eq!(reservation.captured(), &before);
    }
}

#[test]
fn captured_media_requires_exact_bytes_dimensions_count_and_current_target() {
    let slot = blossom();
    let (media, bytes) = photo();
    let mut input = input(AddCommandType::CreatePhotoUpdate);
    input.media.push(media);
    let source = reservation(input.clone());
    assert!(
        CapturedSubmission::capture(
            source.clone(),
            policy("wss://relay.example"),
            Some(&slot),
            vec![]
        )
        .is_err()
    );
    assert!(
        CapturedSubmission::capture(
            source.clone(),
            policy("wss://relay.example"),
            None,
            vec![Arc::clone(&bytes)]
        )
        .is_err()
    );
    assert!(
        CapturedSubmission::capture(
            source.clone(),
            policy("wss://relay.example"),
            Some(&slot),
            vec![Arc::from(&b"wrong"[..])]
        )
        .is_err()
    );
    input.media[0].width = 3;
    assert!(
        CapturedSubmission::capture(
            reservation(input),
            policy("wss://relay.example"),
            Some(&slot),
            vec![Arc::clone(&bytes)]
        )
        .is_err()
    );
    let captured = CapturedSubmission::capture(
        source,
        policy("wss://relay.example"),
        Some(&slot),
        vec![bytes],
    )
    .unwrap();
    assert!(
        captured.media()[0]
            .url()
            .starts_with("http://127.0.0.1:3000/")
    );
    assert_eq!(
        captured.media()[0].stage(),
        crate::runtime::product_surface::Phase1MediaStage::Pending
    );
}

#[test]
fn media_policy_changes_conflict_even_when_blob_url_and_bytes_are_unchanged() {
    let slot = blossom();
    let (media, bytes) = photo();
    let mut input = input(AddCommandType::CreatePhotoUpdate);
    input.media.push(media);
    let source = reservation(input);
    let first = CapturedSubmission::capture(
        source.clone(),
        policy("wss://relay.example"),
        Some(&slot),
        vec![Arc::clone(&bytes)],
    )
    .unwrap();
    let original_policy = first.media_policy();
    slot.configure(
        BlossomConfig::from_profile(slot.profile().unwrap())
            .with_limits(1024 * 1024, 8192, 1)
            .unwrap(),
    )
    .unwrap();
    let changed = CapturedSubmission::capture(
        source.clone(),
        policy("wss://relay.example"),
        Some(&slot),
        vec![Arc::clone(&bytes)],
    )
    .unwrap();
    assert_eq!(first.media()[0].url(), changed.media()[0].url());
    assert!(!first.same_request(&changed));
    assert_ne!(first.media_policy(), changed.media_policy());
    assert!(matches!(
        media::prepare(
            &source.captured().form().input().media,
            Some(&slot),
            original_policy,
            vec![bytes]
        ),
        Err(SubmissionCaptureError::InvalidInput("media_policy_changed"))
    ));
}
