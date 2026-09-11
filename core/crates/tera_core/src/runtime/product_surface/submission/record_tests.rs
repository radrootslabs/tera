use super::*;
use crate::runtime::product_surface::submission::test_support::*;

fn altered(stored: &AuthoredDraft, edit: impl FnOnce(&mut serde_json::Value)) -> AuthoredDraft {
    let mut payload: serde_json::Value = serde_json::from_slice(stored.payload()).unwrap();
    edit(&mut payload);
    AuthoredDraft::initial(
        stored.draft_id(),
        *stored.author(),
        stored.payload_schema(),
        serde_json::to_vec(&payload).unwrap(),
        stored.stage(),
        None,
        stored.created_at_unix_ms(),
    )
    .unwrap()
    .with_scope(stored.scope().unwrap())
    .unwrap()
}

#[test]
fn command_ids_are_validated_distinct_and_independent_of_content() {
    assert_eq!(SubmissionCommandId::new([0; 16]), Err(E::InvalidCommandId));
    assert!(
        serde_json::from_str::<SubmissionCommandId>(&serde_json::to_string(&[0; 16]).unwrap())
            .is_err()
    );
    let left = SubmissionCommandId::generate().unwrap();
    let right = SubmissionCommandId::generate().unwrap();
    assert_ne!(left, right);
    assert_eq!(
        serde_json::from_str::<SubmissionCommandId>(&serde_json::to_string(&left).unwrap())
            .unwrap(),
        left
    );
    let request = request();
    let mut changed = request.clone();
    changed.scope = scope(AUTHOR, "changed");
    assert_eq!(
        reservation_id(&request).unwrap(),
        reservation_id(&changed).unwrap()
    );
    changed.scope = scope(OTHER, "nearby");
    assert_ne!(
        reservation_id(&request).unwrap(),
        reservation_id(&changed).unwrap()
    );
    changed = request.clone();
    changed.command_id = right;
    assert_ne!(
        reservation_id(&request).unwrap(),
        reservation_id(&changed).unwrap()
    );
}

#[test]
fn descriptor_and_envelope_bind_only_exact_source_metadata() {
    let bytes = include_bytes!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../fixtures/submission_reservation_schema_v1.json"
    ));
    assert_eq!(
        hex::encode(Sha256::digest(bytes)),
        SUBMISSION_RESERVATION_SCHEMA_SHA256
    );
    let descriptor: serde_json::Value = serde_json::from_slice(bytes).unwrap();
    assert_eq!(
        descriptor["bounds"]["payload_bytes"],
        SUBMISSION_RESERVATION_MAX_BYTES
    );
    let request = request();
    let source = source();
    let stored = initial(&request, &source, Some(NOW + 1)).unwrap();
    assert_eq!(stored.revision(), AuthoredDraftRevision::INITIAL);
    assert_eq!(stored.stage(), AuthoredDraftStage::Draft);
    assert!(stored.operation_id().is_none());
    assert!(!String::from_utf8_lossy(stored.payload()).contains("PRIVATE"));
    assert!(
        decode(&stored, &request)
            .unwrap()
            .source
            .matches(source.stored())
    );
    for time in [None, Some(0), Some(NOW - 1), Some(u64::MAX)] {
        assert_eq!(
            initial(&request, &source, time).unwrap_err(),
            E::ClockUnavailable
        );
    }
}

#[test]
fn unknown_fields_versions_scope_and_metadata_corruption_fail_closed() {
    let request = request();
    let stored = initial(&request, &source(), Some(NOW)).unwrap();
    for path in ["", "/scope", "/source"] {
        let changed = altered(&stored, |value| {
            value.pointer_mut(path).unwrap()["unknown"] = true.into();
        });
        assert!(matches!(decode(&changed, &request), Err(E::CorruptRecord)));
    }
    for (key, value) in [
        ("schema_version", 2.into()),
        ("schema_sha256", "unknown".into()),
    ] {
        let changed = altered(&stored, |wire| wire[key] = value);
        assert!(matches!(
            decode(&changed, &request),
            Err(E::UnsupportedSchema)
        ));
    }
    let changed = altered(&stored, |wire| {
        wire["source"]["payload_schema"] = "other.v1".into()
    });
    assert!(matches!(decode(&changed, &request), Err(E::CorruptRecord)));
    let successor = stored
        .successor(
            stored.payload().to_vec(),
            AuthoredDraftStage::Draft,
            None,
            NOW + 1,
        )
        .unwrap();
    assert!(matches!(
        decode(&successor, &request),
        Err(E::CorruptRecord)
    ));
    let oversized = AuthoredDraft::initial(
        stored.draft_id(),
        *stored.author(),
        stored.payload_schema(),
        vec![b' '; SUBMISSION_RESERVATION_MAX_BYTES + 1],
        stored.stage(),
        None,
        NOW,
    )
    .unwrap();
    assert!(matches!(
        decode(&oversized, &request),
        Err(E::CorruptRecord)
    ));
    let other_scope = AuthoredDraft::initial(
        stored.draft_id(),
        *stored.author(),
        stored.payload_schema(),
        stored.payload().to_vec(),
        stored.stage(),
        None,
        NOW,
    )
    .unwrap()
    .with_scope(ComposerStorageRecord::scope_digest(&scope(AUTHOR, "other")).unwrap())
    .unwrap();
    assert!(matches!(
        decode(&other_scope, &request),
        Err(E::CorruptRecord)
    ));
}
