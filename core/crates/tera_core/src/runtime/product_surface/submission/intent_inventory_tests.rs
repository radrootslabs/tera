use super::*;
use crate::runtime::product_surface::{
    media_gc::InventoryBudget,
    submission::{reference_inventory, test_support::*, transaction_test_support::capture},
};

#[tokio::test]
async fn media_inventory_retains_pending_uploading_and_failed_intent_sources() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let store = client.storage().unwrap();
    let captured = capture(store, &request(), true).await;
    let original = IntentPayload::capture(&captured).unwrap();
    let mut stored = original.intent().clone();
    for stage in [
        Phase1MediaStage::Pending,
        Phase1MediaStage::Uploading,
        Phase1MediaStage::Failed,
    ] {
        if stage != Phase1MediaStage::Pending {
            let mut payload = decode(&stored).unwrap();
            payload.media[0]
                .transition_requested(
                    stage,
                    (stage == Phase1MediaStage::Failed).then(|| "upload_unknown".into()),
                )
                .unwrap();
            stored = stored
                .successor(
                    serde_json::to_vec(&payload).unwrap(),
                    payload.stage(),
                    None,
                    NOW + 1,
                )
                .unwrap();
        }
        let hashes = reference_inventory::media_references(
            stored.clone(),
            request().scope().author().into_bytes(),
            store,
            &mut InventoryBudget::default(),
        )
        .await
        .unwrap();
        assert_eq!(hashes, [photo().0.sha256]);
    }
    client.close().await.unwrap();
}

#[tokio::test]
async fn media_inventory_rejects_missing_reservation_changed_media_and_noncanonical_payload() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let store = client.storage().unwrap();
    let captured = capture(store, &request(), true).await;
    let original = IntentPayload::capture(&captured).unwrap();
    for mutation in 0..3 {
        let mut payload = decode(original.intent()).unwrap();
        match mutation {
            0 => payload.reservation_id = AuthoredDraftId::new([9; 16]).unwrap(),
            1 => payload.media.clear(),
            _ => {}
        }
        let mut bytes = serde_json::to_vec(&payload).unwrap();
        if mutation == 2 {
            bytes.push(b' ');
        }
        let stored = original
            .intent()
            .successor(bytes, AuthoredDraftStage::MediaPreparing, None, NOW + 1)
            .unwrap();
        assert!(
            reference_inventory::media_references(
                stored,
                request().scope().author().into_bytes(),
                store,
                &mut InventoryBudget::default()
            )
            .await
            .is_err()
        );
    }
    client.close().await.unwrap();
}
