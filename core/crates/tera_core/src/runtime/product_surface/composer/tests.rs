use super::*;
use crate::runtime::product_surface::{
    AddCommandType, CANONICAL_ADD_COMMAND_TYPES, CreateAsk, CreatePhotoUpdate, CreateUpdate,
    LocalNetworkId, Phase1DraftEventTiming,
};
use radroots_identity::PublicKey;

fn scope() -> ComposerScope {
    ComposerScope::new(
        PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .unwrap(),
        LocalNetworkId::new("nearby".into()).unwrap(),
    )
}

fn media() -> ComposerMediaInput {
    ComposerMediaInput {
        opaque_reference: "media:editing-fixture".into(),
        sha256: "a".repeat(64),
        media_type: "image/png".into(),
        byte_size: 512,
        width: 4,
        height: 4,
        alt: String::new(),
        prepared_at_unix_s: 1_800_000_000,
    }
}

#[test]
fn all_five_families_accept_empty_and_incomplete_local_editing() {
    for family in CANONICAL_ADD_COMMAND_TYPES {
        let input = ComposerFormInput::empty(family);
        let empty = ComposerPartialForm::new(input.clone()).unwrap();
        assert_eq!(empty.input(), &input);
        let mut partial = input;
        partial.price_amount = Some("-".into());
        partial.quantity = Some("1.".into());
        partial.currency = Some("C".into());
        partial.event_start_date = Some("2026-09-".into());
        partial.event_end_date = Some("2".into());
        partial.event_timezone = Some("America/".into());
        partial.media.push(media());
        let form = ComposerPartialForm::new(partial.clone()).unwrap();
        assert_eq!(form.input(), &partial);
        assert_eq!(
            ComposerPartialForm::from_json(&form.to_json().unwrap()).unwrap(),
            form
        );
    }
}

#[test]
fn raw_editing_bytes_and_inactive_fields_survive_round_trip_exactly() {
    let mut input = ComposerFormInput::empty(AddCommandType::CreateEvent);
    input.content = "  café\n\tunfinished\r\n".into();
    input.summary = Some("\n".into());
    input.price_amount = Some("01,20.".into());
    input.quantity = Some(" 2 ".into());
    input.event_timing = Some(Phase1DraftEventTiming::AllDay);
    input.event_start_date = Some("2026-02-3".into());
    input.event_start_unix_s = Some(0);
    input.event_end_unix_s = Some(u64::MAX);
    input.food_published_at_unix_s = Some(u64::MAX);
    input.media.push(media());
    input.media[0].alt = "  unfinishe\n".into();
    let form = ComposerPartialForm::new(input.clone()).unwrap();
    let encoded = form.to_json().unwrap();
    let restored = ComposerPartialForm::from_json(&encoded).unwrap();
    assert_eq!(restored.input(), &input);
    assert_eq!(restored.to_json().unwrap(), encoded);
}

#[test]
fn exact_maxima_and_worst_case_json_escaping_stay_within_the_aggregate_bound() {
    let mut input = ComposerFormInput::empty(AddCommandType::CreatePhotoUpdate);
    input.content = "\0".repeat(COMPOSER_CONTENT_MAX_BYTES);
    for field in [
        &mut input.identifier,
        &mut input.title,
        &mut input.summary,
        &mut input.location,
        &mut input.event_start_date,
        &mut input.event_end_date,
        &mut input.event_timezone,
        &mut input.price_amount,
        &mut input.currency,
        &mut input.unit,
        &mut input.quantity,
        &mut input.food_status,
    ] {
        *field = Some("\0".repeat(COMPOSER_TEXT_MAX_BYTES));
    }
    let mut reference = media();
    reference.opaque_reference = "media:".to_owned() + &"a".repeat(250);
    reference.alt = "\0".repeat(COMPOSER_TEXT_MAX_BYTES);
    input.media = vec![reference; COMPOSER_MEDIA_MAX];
    let form = ComposerPartialForm::new(input).unwrap();
    let bytes = form.to_json().unwrap();
    assert!(bytes.len() <= COMPOSER_FORM_MAX_BYTES);
    assert_eq!(ComposerPartialForm::from_json(&bytes).unwrap(), form);
}

#[test]
fn utf8_byte_limits_and_excess_media_reject_before_retaining_a_form() {
    let mut input = ComposerFormInput::empty(AddCommandType::CreateUpdate);
    input.content = "é".repeat(COMPOSER_CONTENT_MAX_BYTES / 2) + "a";
    assert!(ComposerPartialForm::new(input.clone()).is_ok());
    input.content.push('a');
    assert_eq!(
        ComposerPartialForm::new(input),
        Err(ComposerError::InvalidForm)
    );
    let mut input = ComposerFormInput::empty(AddCommandType::CreateFoodAvailability);
    input.quantity = Some("é".repeat(COMPOSER_TEXT_MAX_BYTES / 2));
    assert!(ComposerPartialForm::new(input.clone()).is_ok());
    input.quantity.as_mut().unwrap().push('a');
    assert!(ComposerPartialForm::new(input).is_err());
    let mut input = ComposerFormInput::empty(AddCommandType::CreatePhotoUpdate);
    input.media = vec![media(); COMPOSER_MEDIA_MAX + 1];
    assert!(ComposerPartialForm::new(input).is_err());
}

#[test]
fn invalid_media_metadata_is_not_confused_with_missing_alt_text() {
    let mut input = ComposerFormInput::empty(AddCommandType::CreatePhotoUpdate);
    input.media.push(media());
    assert!(ComposerPartialForm::new(input.clone()).is_ok());
    let changes: &[fn(&mut ComposerMediaInput)] = &[
        |m| m.opaque_reference = "file:/private/image.png".into(),
        |m| m.opaque_reference = "media:".into(),
        |m| m.opaque_reference = "media:".to_owned() + &"a".repeat(251),
        |m| m.opaque_reference = "media:UPPER".into(),
        |m| m.sha256 = "g".repeat(64),
        |m| m.media_type = "invalid media type".into(),
        |m| m.media_type = "a".repeat(129),
        |m| m.byte_size = 0,
        |m| m.byte_size = 10 * 1024 * 1024 + 1,
        |m| m.width = 0,
        |m| m.height = u32::MAX,
        |m| m.alt = "é".repeat(513),
        |m| m.prepared_at_unix_s = 0,
        |m| m.prepared_at_unix_s = u64::MAX,
    ];
    for change in changes {
        let mut invalid = input.clone();
        change(&mut invalid.media[0]);
        assert_eq!(
            ComposerPartialForm::new(invalid),
            Err(ComposerError::InvalidForm)
        );
    }
}

#[test]
fn wire_decoding_cannot_bypass_bounds_or_add_unknown_fields() {
    let mut input = ComposerFormInput::empty(AddCommandType::CreateUpdate);
    input.content = "x".repeat(COMPOSER_CONTENT_MAX_BYTES + 1);
    let invalid = serde_json::to_vec(&input).unwrap();
    assert!(serde_json::from_slice::<ComposerPartialForm>(&invalid).is_err());
    assert!(ComposerPartialForm::from_json(&invalid).is_err());
    assert!(ComposerPartialForm::from_json(&[]).is_err());
    assert!(ComposerPartialForm::from_json(b"{").is_err());
    assert!(ComposerPartialForm::from_json(&vec![b' '; COMPOSER_FORM_MAX_BYTES + 1]).is_err());
    let mut input = ComposerFormInput::empty(AddCommandType::CreatePhotoUpdate);
    input.media.push(media());
    let value = serde_json::to_value(input).unwrap();
    for field in ["schema_version", "signing_key", "operation_id"] {
        let mut unknown = value.clone();
        unknown[field] = serde_json::json!(1);
        assert!(serde_json::from_value::<ComposerPartialForm>(unknown).is_err());
    }
    let mut nested = value;
    nested["media"][0]["authorization_header"] = serde_json::json!("fixture");
    assert!(serde_json::from_value::<ComposerPartialForm>(nested).is_err());
}

#[test]
fn persistent_identity_and_scope_use_validated_scalars_without_generation() {
    let id = ComposerId::new([1; 16]).unwrap();
    assert_eq!(id.as_bytes(), &[1; 16]);
    assert!(ComposerId::new([0; 16]).is_err());
    assert!(serde_json::from_value::<ComposerId>(serde_json::json!([0; 16].to_vec())).is_err());
    assert_eq!(
        serde_json::from_value::<ComposerId>(serde_json::to_value(id).unwrap()).unwrap(),
        id
    );
    let selected = scope();
    let wire = serde_json::to_value(&selected).unwrap();
    assert_eq!(
        serde_json::from_value::<ComposerScope>(wire.clone()).unwrap(),
        selected
    );
    assert_eq!(selected.local_network().as_str(), "nearby");
    assert_ne!(
        selected,
        ComposerScope::new(
            selected.author(),
            LocalNetworkId::new("other".into()).unwrap()
        )
    );
    for (field, value) in [
        ("author", serde_json::json!("00".repeat(32))),
        ("local_network", serde_json::json!(" nearby")),
        ("generation", serde_json::json!(2)),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(serde_json::from_value::<ComposerScope>(invalid).is_err());
    }
}

#[test]
fn revision_and_edit_sequence_are_distinct_checked_monotonic_values() {
    assert_eq!(ComposerRevision::INITIAL.next().unwrap().get(), 2);
    assert_eq!(ComposerEditSequence::INITIAL.next().unwrap().get(), 2);
    for value in [0, ComposerRevision::MAX + 1, u64::MAX] {
        assert!(ComposerRevision::new(value).is_err());
        assert!(serde_json::from_value::<ComposerRevision>(serde_json::json!(value)).is_err());
    }
    assert!(
        ComposerRevision::new(ComposerRevision::MAX)
            .unwrap()
            .next()
            .is_err()
    );
    assert!(ComposerEditSequence::new(0).is_err());
    assert!(serde_json::from_str::<ComposerEditSequence>("0").is_err());
    let max_edit = ComposerEditSequence::new(u64::MAX).unwrap();
    assert!(max_edit.next().is_err());
    assert_eq!(
        serde_json::from_value::<ComposerEditSequence>(serde_json::to_value(max_edit).unwrap())
            .unwrap(),
        max_edit
    );
    let form =
        ComposerPartialForm::new(ComposerFormInput::empty(AddCommandType::CreateAsk)).unwrap();
    let draft = ComposerDraft::new(
        ComposerId::new([1; 16]).unwrap(),
        ComposerRevision::INITIAL,
        scope(),
        max_edit,
        form.clone(),
    );
    assert_eq!(draft.revision(), ComposerRevision::INITIAL);
    assert_eq!(draft.edit_sequence(), max_edit);
    assert_eq!(draft.form(), &form);
    assert_eq!(draft.scope(), &scope());
    assert_eq!(draft.id().as_bytes(), &[1; 16]);
}

#[test]
fn incomplete_editing_does_not_weaken_strict_publication_constructors() {
    use radroots_event::{
        calendar::{AuthoredCalendarTimeEvent, CalendarDate},
        food::availability::{FoodCurrency, FoodPrice, FoodUnit},
    };
    let mut input = ComposerFormInput::empty(AddCommandType::CreateFoodAvailability);
    input.price_amount = Some("-".into());
    input.event_start_date = Some("2026-09-".into());
    let form = ComposerPartialForm::new(input).unwrap();
    assert!(CreateUpdate::new(&form.input().content).is_err());
    assert!(CreateAsk::new(&form.input().content, vec![]).is_err());
    assert!(CreatePhotoUpdate::new("unfinished photo", vec![]).is_err());
    assert!(CalendarDate::parse(form.input().event_start_date.as_deref().unwrap()).is_err());
    assert!(AuthoredCalendarTimeEvent::new("", "", 0).is_err());
    assert!(
        FoodPrice::new(
            form.input().price_amount.as_deref().unwrap(),
            FoodCurrency::parse("CAD").unwrap(),
            FoodUnit::Pound
        )
        .is_err()
    );
}

#[test]
fn debug_and_errors_do_not_expose_editing_content_or_local_references() {
    let mut input = ComposerFormInput::empty(AddCommandType::CreatePhotoUpdate);
    input.content = "PRIVATE_EDITING_MARKER".into();
    input.media.push(media());
    input.media[0].alt = "PRIVATE_ALT_MARKER".into();
    let form = ComposerPartialForm::new(input).unwrap();
    let draft = ComposerDraft::new(
        ComposerId::new([1; 16]).unwrap(),
        ComposerRevision::INITIAL,
        scope(),
        ComposerEditSequence::INITIAL,
        form.clone(),
    );
    for debug in [
        format!("{form:?}"),
        format!("{draft:?}"),
        format!("{:?}", form.input().media[0]),
    ] {
        assert!(!debug.contains("PRIVATE_"));
        assert!(!debug.contains("media:editing-fixture"));
    }
    assert_eq!(
        ComposerError::InvalidForm.to_string(),
        "composer form exceeds its bounds or has invalid metadata"
    );
}
