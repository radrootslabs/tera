use super::*;
use crate::runtime::product_surface::{
    AddCommandType, CANONICAL_ADD_COMMAND_TYPES, COMPOSER_CONTENT_MAX_BYTES, COMPOSER_MEDIA_MAX,
    COMPOSER_TEXT_MAX_BYTES, ComposerFormInput, ComposerMediaInput, LocalNetworkId,
    Phase1DraftEventTiming,
};
use radroots_identity::PublicKey;
use serde_json::{Value, json};
use std::collections::BTreeSet;

pub(super) const AUTHOR: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
pub(super) const NOW: u64 = 1_800_000_000_000;

pub(super) fn scope() -> ComposerScope {
    ComposerScope::new(
        PublicKey::from_hex(AUTHOR).unwrap(),
        LocalNetworkId::new("nearby".into()).unwrap(),
    )
}

fn media() -> ComposerMediaInput {
    ComposerMediaInput {
        opaque_reference: "media:private-fixture".into(),
        sha256: "a".repeat(64),
        media_type: "image/png".into(),
        byte_size: 512,
        width: 4,
        height: 4,
        alt: "PRIVATE_ALT".into(),
        prepared_at_unix_s: NOW / 1000,
    }
}

pub(super) fn record(id: u8) -> ComposerStorageRecord {
    let mut form = ComposerFormInput::empty(AddCommandType::CreateFoodAvailability);
    form.content = "PRIVATE_EDIT\n\0 café  ".into();
    form.price_amount = Some("-".into());
    form.event_start_date = Some("2026-09-".into());
    form.media.push(media());
    ComposerStorageRecord::initial(
        ComposerId::new([id; 16]).unwrap(),
        scope(),
        ComposerEditSequence::new(7).unwrap(),
        ComposerPartialForm::new(form).unwrap(),
        NOW,
    )
    .unwrap()
}

pub(super) fn with_payload(id: u8, payload: Vec<u8>) -> AuthoredDraft {
    AuthoredDraft::initial(
        AuthoredDraftId::new([id; 16]).unwrap(),
        scope().author().into_bytes(),
        COMPOSER_PAYLOAD_SCHEMA,
        payload,
        AuthoredDraftStage::Draft,
        None,
        NOW,
    )
    .unwrap()
    .with_scope(ComposerStorageRecord::scope_digest(&scope()).unwrap())
    .unwrap()
}

fn wire() -> Value {
    serde_json::from_slice(record(1).stored().payload()).unwrap()
}

fn assert_error(stored: AuthoredDraft, error: ComposerStorageError) {
    assert_eq!(
        ComposerStorageRecord::decode(stored, &scope()).unwrap_err(),
        error
    );
}

#[test]
fn descriptor_pins_exact_wire_fields_bounds_families_and_scope_hash() {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/composer_schema_v1.json"
    ));
    assert_eq!(hex::encode(Sha256::digest(bytes)), COMPOSER_SCHEMA_SHA256);
    let descriptor: Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(descriptor["payload_schema"], COMPOSER_PAYLOAD_SCHEMA);
    assert_eq!(descriptor["schema_version"], COMPOSER_SCHEMA_VERSION);
    let encoded = wire();
    for (field, value) in [
        ("payload_fields", &encoded),
        ("scope_fields", &encoded["scope"]),
        ("form_fields", &encoded["form"]),
        ("media_fields", &encoded["form"]["media"][0]),
    ] {
        let expected: BTreeSet<_> = descriptor[field]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        let actual: BTreeSet<_> = value
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(actual, expected, "{field}");
    }
    assert_eq!(
        descriptor["creation_families"],
        json!(CANONICAL_ADD_COMMAND_TYPES)
    );
    assert_eq!(
        descriptor["event_timing"],
        json!([
            Phase1DraftEventTiming::AllDay,
            Phase1DraftEventTiming::Timed
        ])
    );
    assert_eq!(
        descriptor["bounds"],
        json!({
            "payload_bytes": COMPOSER_FORM_MAX_BYTES, "content_utf8_bytes": COMPOSER_CONTENT_MAX_BYTES,
            "partial_text_utf8_bytes": COMPOSER_TEXT_MAX_BYTES, "media_count": COMPOSER_MEDIA_MAX,
            "media_reference_bytes": 256, "media_type_utf8_bytes": 128,
            "media_alt_utf8_bytes": COMPOSER_TEXT_MAX_BYTES, "media_file_bytes": 10 * 1024 * 1024,
            "local_network_utf8_bytes": 256, "revision_max": ComposerRevision::MAX,
            "edit_sequence_max": u64::MAX, "timestamp_ms_max": i64::MAX as u64,
        })
    );
    assert_eq!(
        hex::encode(
            ComposerStorageRecord::scope_digest(&scope())
                .unwrap()
                .as_bytes()
        ),
        "40da79f35835c1cd90c8d8ddb9712da70a9727a8d3d07f44f40235a5ed300800"
    );
}

#[test]
fn exact_partial_payload_identity_and_history_round_trip_without_signing_authority() {
    let original = record(1);
    let restored = ComposerStorageRecord::decode(original.stored().clone(), &scope()).unwrap();
    assert_eq!(restored.draft(), original.draft());
    assert_eq!(restored.stored(), original.stored());
    assert_eq!(restored.stored().stage(), AuthoredDraftStage::Draft);
    assert!(restored.stored().operation_id().is_none());
    let mut payload = wire();
    payload["edit_sequence"] = json!(19);
    payload["form"]["price_amount"] = json!("1.");
    let next = original
        .stored()
        .successor(
            serde_json::to_vec(&payload).unwrap(),
            AuthoredDraftStage::Draft,
            None,
            NOW + 1,
        )
        .unwrap();
    let restored = ComposerStorageRecord::decode(next, &scope()).unwrap();
    assert_eq!(restored.draft().revision().get(), 2);
    assert_eq!(restored.draft().edit_sequence().get(), 19);
    assert_eq!(
        restored.draft().form().input().price_amount.as_deref(),
        Some("1.")
    );
    assert_eq!(
        original.draft().form().input().price_amount.as_deref(),
        Some("-")
    );
    assert_eq!(restored.stored().created_at_unix_ms(), NOW);
    assert_eq!(restored.stored().updated_at_unix_ms(), NOW + 1);
}

#[test]
fn future_version_or_checksum_is_rejected_before_interpreting_incompatible_body() {
    for (version, checksum) in [(2, COMPOSER_SCHEMA_SHA256), (1, "different-schema")] {
        let payload = json!({"schema_version": version, "schema_sha256": checksum,
            "scope": ["future"], "edit_sequence": "future", "form": ["future"]});
        assert_error(
            with_payload(1, serde_json::to_vec(&payload).unwrap()),
            ComposerStorageError::UnsupportedSchema,
        );
    }
}

#[test]
fn malformed_current_records_unknown_fields_and_duplicate_keys_fail_closed() {
    for bytes in [
        b"{".to_vec(),
        b"{}".to_vec(),
        vec![b' '; COMPOSER_FORM_MAX_BYTES + 1],
    ] {
        assert_error(with_payload(1, bytes), ComposerStorageError::CorruptRecord);
    }
    for pointer in ["", "/scope", "/form", "/form/media/0"] {
        let mut payload = wire();
        payload.pointer_mut(pointer).unwrap()["unexpected"] = json!(true);
        assert_error(
            with_payload(1, serde_json::to_vec(&payload).unwrap()),
            ComposerStorageError::CorruptRecord,
        );
    }
    let bytes = String::from_utf8(record(1).stored().payload().to_vec()).unwrap();
    for (find, replacement) in [
        (
            "\"schema_version\":1",
            "\"schema_version\":1,\"schema_version\":1",
        ),
        (
            "\"edit_sequence\":7",
            "\"edit_sequence\":7,\"edit_sequence\":7",
        ),
        ("\"width\":4", "\"width\":4,\"width\":4"),
    ] {
        assert!(bytes.contains(find));
        assert_error(
            with_payload(1, bytes.replace(find, replacement).into_bytes()),
            ComposerStorageError::CorruptRecord,
        );
    }
    for (pointer, value) in [
        ("/form", json!(null)),
        ("/edit_sequence", json!(0)),
        (
            "/form/content",
            json!("x".repeat(COMPOSER_CONTENT_MAX_BYTES + 1)),
        ),
        ("/scope/local_network", json!("other")),
    ] {
        let mut payload = wire();
        *payload.pointer_mut(pointer).unwrap() = value;
        assert_error(
            with_payload(1, serde_json::to_vec(&payload).unwrap()),
            ComposerStorageError::CorruptRecord,
        );
    }
}

#[test]
fn foreign_scope_and_schema_are_rejected_before_payload_inspection() {
    let draft = with_payload(1, b"not JSON".to_vec());
    let other = ComposerScope::new(
        scope().author(),
        LocalNetworkId::new("other".into()).unwrap(),
    );
    assert_eq!(
        ComposerStorageRecord::decode(draft.clone(), &other).unwrap_err(),
        ComposerStorageError::ScopeMismatch
    );
    for (field, value, error) in [
        (
            "author",
            json!([2; 32].to_vec()),
            ComposerStorageError::ScopeMismatch,
        ),
        ("scope", json!(null), ComposerStorageError::ScopeMismatch),
        (
            "scope",
            json!([3; 32].to_vec()),
            ComposerStorageError::ScopeMismatch,
        ),
        (
            "payload_schema",
            json!("fixture.noncomposer.v1"),
            ComposerStorageError::WrongSchema,
        ),
    ] {
        let mut envelope = serde_json::to_value(&draft).unwrap();
        envelope[field] = value;
        assert_error(serde_json::from_value(envelope).unwrap(), error);
    }
}

#[test]
fn storage_timestamps_revisions_and_nonediting_stages_are_checked() {
    let original = record(1);
    for time in [0, i64::MAX as u64 + 1, u64::MAX] {
        assert_eq!(
            ComposerStorageRecord::initial(
                original.draft().id(),
                scope(),
                ComposerEditSequence::INITIAL,
                original.draft().form().clone(),
                time
            )
            .unwrap_err(),
            ComposerStorageError::InvalidTimestamp
        );
    }
    for time in [1, i64::MAX as u64] {
        let value = ComposerStorageRecord::initial(
            original.draft().id(),
            scope(),
            ComposerEditSequence::INITIAL,
            original.draft().form().clone(),
            time,
        )
        .unwrap();
        assert!(ComposerStorageRecord::decode(value.into_stored(), &scope()).is_ok());
    }
    let preparing = original
        .stored()
        .successor(
            original.stored().payload().to_vec(),
            AuthoredDraftStage::MediaPreparing,
            None,
            NOW + 1,
        )
        .unwrap();
    assert_error(preparing, ComposerStorageError::CorruptRecord);
    for (field, value, error) in [
        (
            "revision",
            json!(i64::MAX as u64 + 1),
            ComposerStorageError::CorruptRecord,
        ),
        (
            "updated_at_unix_ms",
            json!(u64::MAX),
            ComposerStorageError::InvalidTimestamp,
        ),
    ] {
        let mut envelope = serde_json::to_value(original.stored()).unwrap();
        envelope[field] = value;
        assert_error(serde_json::from_value(envelope).unwrap(), error);
    }
}

#[test]
fn maximum_escaped_form_and_scope_fit_the_envelope_bound_and_debug_is_redacted() {
    let mut payload = wire();
    for (key, value) in payload["form"].as_object_mut().unwrap() {
        if key == "content" {
            *value = json!("\0".repeat(COMPOSER_CONTENT_MAX_BYTES));
        } else if !matches!(key.as_str(), "command_type" | "event_timing" | "media")
            && !key.ends_with("unix_s")
        {
            *value = json!("\0".repeat(COMPOSER_TEXT_MAX_BYTES));
        }
    }
    let mut reference = media();
    reference.alt = "\0".repeat(COMPOSER_TEXT_MAX_BYTES);
    reference.opaque_reference = "media:".to_owned() + &"a".repeat(250);
    payload["form"]["media"] = json!(vec![reference; COMPOSER_MEDIA_MAX]);
    let selected = ComposerScope::new(
        scope().author(),
        LocalNetworkId::new("é".repeat(128)).unwrap(),
    );
    let value = ComposerStorageRecord::initial(
        ComposerId::new([1; 16]).unwrap(),
        selected.clone(),
        ComposerEditSequence::new(u64::MAX).unwrap(),
        serde_json::from_value(payload["form"].clone()).unwrap(),
        NOW,
    )
    .unwrap();
    assert!(value.stored().payload().len() <= COMPOSER_FORM_MAX_BYTES);
    assert_eq!(
        ComposerStorageRecord::decode(value.stored().clone(), &selected)
            .unwrap()
            .draft(),
        value.draft()
    );
    let debug = format!("{:?}", record(1));
    assert!(!debug.contains("PRIVATE_"));
    assert!(!debug.contains("media:private-fixture"));
}
