use super::super::{
    repository::SubmissionRepository, test_support::*, transaction_test_support::capture,
};
use super::*;
use radroots_storage::authored_atomic::AuthoredAtomicOutcome;

fn with_payload(original: &PrepareFromDraft, bytes: Vec<u8>, schema: &str) -> PrepareFromDraft {
    let old = original.intent();
    let digest = Sha256::digest(&bytes).into();
    let draft = AuthoredDraft::reconstruct(
        old.draft_id(),
        old.revision(),
        *old.author(),
        schema,
        bytes,
        digest,
        old.stage(),
        old.operation_id(),
        old.created_at_unix_ms(),
        old.updated_at_unix_ms(),
    )
    .unwrap()
    .with_scope(old.scope().unwrap())
    .unwrap();
    let constructor = if old.operation_id().is_some() {
        PrepareFromDraft::new
    } else {
        PrepareFromDraft::new_waiting
    };
    constructor(
        original.command_id(),
        original.source().clone(),
        draft,
        original.preparation().clone(),
    )
    .unwrap()
}

#[tokio::test]
async fn descriptor_ids_and_original_source_are_stable_bounded_and_account_scoped() {
    assert_eq!(
        hex::encode(Sha256::digest(include_bytes!(
            "../../../../../../fixtures/submission_intent_schema_v1.json"
        ))),
        SUBMISSION_INTENT_SCHEMA_SHA256
    );
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let store = client.storage().unwrap();
    let request = request();
    let captured = capture(store, &request, false).await;
    let original = IntentPayload::capture(&captured).unwrap();
    assert_eq!(original.source(), &captured.reservation().source);
    assert_ne!(
        original.source().draft_id(),
        captured.reservation().reservation_id()
    );
    IntentPayload::validate_committed(&original, captured.reservation()).unwrap();
    let mut different = request.clone();
    different.scope = scope(OTHER, "nearby");
    assert_ne!(intent_id(&request).unwrap(), intent_id(&different).unwrap());
    assert_ne!(
        operation_id(&request).unwrap(),
        operation_id(&different).unwrap()
    );
    assert_ne!(commit_id(&request), commit_id(&different));
    different.scope = scope(AUTHOR, "other");
    assert_eq!(intent_id(&request).unwrap(), intent_id(&different).unwrap());
    assert_eq!(
        operation_id(&request).unwrap(),
        operation_id(&different).unwrap()
    );
    let mut bytes = original.intent().payload().to_vec();
    bytes.resize(SUBMISSION_INTENT_MAX_BYTES, b' ');
    let at_limit = with_payload(&original, bytes.clone(), SUBMISSION_INTENT_PAYLOAD_SCHEMA);
    IntentPayload::validate_committed(&at_limit, captured.reservation()).unwrap();
    bytes.push(b' ');
    let digest = Sha256::digest(&bytes).into();
    let old = original.intent();
    assert!(
        AuthoredDraft::reconstruct(
            old.draft_id(),
            old.revision(),
            *old.author(),
            old.payload_schema(),
            bytes,
            digest,
            old.stage(),
            old.operation_id(),
            old.created_at_unix_ms(),
            old.updated_at_unix_ms()
        )
        .is_err()
    );
}

#[tokio::test]
async fn malformed_and_self_inconsistent_intents_fail_before_any_success_receipt() {
    for media in [false, true] {
        let client = radroots_sdk::ClientBuilder::memory_default()
            .build()
            .unwrap();
        let store = client.storage().unwrap();
        let captured = capture(store, &request(), media).await;
        let original = IntentPayload::capture(&captured).unwrap();
        let base: serde_json::Value = serde_json::from_slice(original.intent().payload()).unwrap();
        let mut variants = Vec::new();
        for (field, value) in [
            ("schema_version", serde_json::json!(2)),
            ("schema_sha256", serde_json::json!("wrong")),
            ("unknown", serde_json::json!(true)),
            ("command_id", serde_json::json!([9; 16].to_vec())),
            ("reservation_id", serde_json::json!([9; 16].to_vec())),
            ("command_type", serde_json::json!("create_event")),
            ("plan_wire_json", serde_json::json!([123u8, 125].to_vec())),
        ] {
            let mut changed = base.clone();
            changed[field] = value;
            variants.push(serde_json::to_vec(&changed).unwrap());
        }
        let text = String::from_utf8(original.intent().payload().to_vec()).unwrap();
        variants.push(format!("{{\"schema_version\":1,{}", &text[1..]).into_bytes());
        variants.push(b"{truncated".to_vec());
        let mut changed = base.clone();
        changed["policy"]["delivery_deadline_unix_ms"] = serde_json::json!(NOW + 9999);
        variants.push(serde_json::to_vec(&changed).unwrap());
        let mut changed = base.clone();
        changed["policy"]["unknown"] = serde_json::json!(true);
        variants.push(serde_json::to_vec(&changed).unwrap());
        if media {
            let mut changed = base.clone();
            changed["media"][0]["unknown"] = serde_json::json!(true);
            variants.push(serde_json::to_vec(&changed).unwrap());
            let mut changed = base.clone();
            changed["media"] = serde_json::json!([]);
            variants.push(serde_json::to_vec(&changed).unwrap());
            let mut changed = base.clone();
            changed["media_policy"] = serde_json::Value::Null;
            variants.push(serde_json::to_vec(&changed).unwrap());
            let mut changed = base.clone();
            changed["media"][0]["byte_size"] = serde_json::json!(1);
            variants.push(serde_json::to_vec(&changed).unwrap());
        }
        for bytes in variants {
            let changed = with_payload(&original, bytes, SUBMISSION_INTENT_PAYLOAD_SCHEMA);
            assert!(IntentPayload::validate_committed(&changed, captured.reservation()).is_err());
        }
        assert_eq!(
            IntentPayload::validate_committed(
                &with_payload(
                    &original,
                    original.intent().payload().to_vec(),
                    "future.schema"
                ),
                captured.reservation()
            ),
            Err(E::UnsupportedSchema)
        );
    }
}

#[tokio::test]
async fn original_receipt_stays_valid_after_waiting_intent_head_advances() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let store = client.storage().unwrap();
    let request = request();
    let captured = capture(store, &request, true).await;
    let repo = SubmissionRepository { store };
    let receipt = repo.commit(&captured).await.unwrap();
    let head = store
        .authored_draft_head(receipt.intent_id())
        .await
        .unwrap()
        .unwrap();
    let next = head
        .successor(
            head.payload().to_vec(),
            AuthoredDraftStage::MediaUploading,
            None,
            NOW + 1,
        )
        .unwrap();
    store
        .append_authored_draft(next, Some(head.revision()))
        .await
        .unwrap();
    let replay = repo.recover(&request).await.unwrap().unwrap();
    assert_eq!(receipt.intent_id(), replay.intent_id());
    assert_eq!(receipt.operation_id(), replay.operation_id());
    assert!(replay.is_replay());
    let stored = store
        .authored_receipt(commit_id(&request))
        .await
        .unwrap()
        .unwrap();
    let AuthoredAtomicOutcome::Submitted(original) = stored.outcome() else {
        panic!("wrong receipt")
    };
    assert_eq!(
        original.intent().stage(),
        AuthoredDraftStage::MediaPreparing
    );
    assert_eq!(original.intent().revision(), AuthoredDraftRevision::INITIAL);
}
