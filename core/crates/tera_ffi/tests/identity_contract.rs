use radroots_storage::authored_draft::{AuthoredDraftId, AuthoredDraftRevision};
use tera_core::runtime::product_surface::{LocalNetwork, Phase1ExistingDraft, Phase1QueueIntent};
use tera_ffi::{FfiLocalNetworkRecord, MOBILE_FFI_SCHEMA_VERSION};

#[test]
fn stable_context_id_and_generation_keep_the_existing_wire_shape() {
    let context = LocalNetwork::new(
        "nearby".into(),
        "Nearby".into(),
        vec!["wss://relay.example".into()],
        None,
        vec![],
        u64::MAX,
    )
    .unwrap();
    let wire = serde_json::to_value(&context).unwrap();
    assert_eq!(wire["id"], "nearby");
    assert_eq!(wire["generation"], u64::MAX);
    assert_eq!(
        serde_json::from_value::<LocalNetwork>(wire).unwrap(),
        context
    );
    let dto = FfiLocalNetworkRecord::from(context);
    assert_eq!(dto.schema_version, MOBILE_FFI_SCHEMA_VERSION);
    assert_eq!(dto.id, "nearby");
    assert_eq!(dto.generation, u64::MAX);
}

#[test]
fn shared_draft_identity_and_revision_remain_validated_at_app_admission() {
    let id = AuthoredDraftId::new([7; 16]).unwrap();
    let encoded = serde_json::to_value(id).unwrap();
    assert_eq!(encoded, serde_json::to_value([7; 16]).unwrap());
    assert_eq!(
        serde_json::from_value::<AuthoredDraftId>(encoded).unwrap(),
        id
    );
    for revision in [1, i64::MAX as u64, (i64::MAX as u64) + 1, u64::MAX] {
        let typed = AuthoredDraftRevision::new(revision).unwrap();
        let wire = serde_json::to_string(&typed).unwrap();
        assert_eq!(wire, revision.to_string());
        assert_eq!(
            serde_json::from_str::<AuthoredDraftRevision>(&wire).unwrap(),
            typed
        );
        assert!(Phase1ExistingDraft::new(*id.as_bytes(), revision).is_ok());
        assert!(Phase1QueueIntent::new(*id.as_bytes(), revision).is_ok());
    }
    assert!(
        AuthoredDraftRevision::new(u64::MAX)
            .unwrap()
            .next()
            .is_err()
    );
    for (id, revision) in [([0; 16], 1), ([7; 16], 0)] {
        assert!(Phase1ExistingDraft::new(id, revision).is_err());
        assert!(Phase1QueueIntent::new(id, revision).is_err());
    }
    for wire in ["0", "-1", "18446744073709551616", "1.5"] {
        assert!(serde_json::from_str::<AuthoredDraftRevision>(wire).is_err());
    }
}
