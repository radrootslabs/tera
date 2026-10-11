use super::super::tests::{context, signed, visible_admission};
use super::*;

async fn row(event: radroots_event::SignedEvent, selected: &LocalNetwork) -> Option<TodayRow> {
    let runtime = TeraRuntime::test_memory().unwrap();
    let storage = runtime.client.storage().unwrap();
    EventStore::admit(storage, visible_admission(event, 2_000_000_000_000))
        .await
        .unwrap();
    let visible = query_all_visible(storage).await.unwrap();
    assert_eq!(visible.len(), 1);
    reduce(selected, &visible[0]).unwrap()
}

#[tokio::test]
async fn rows_preserve_complete_profile_metadata_and_canonical_identity() {
    let event = signed(
        0,
        vec![],
        r#"{"name":"Synthetic farmer","website":"https://example.com","custom":{"flag":true}}"#,
        2_000_000_000,
    );
    let id = event.id().to_hex();
    let value = row(event, &context(None, 1)).await.unwrap();
    let document = value.clone().document().unwrap();
    assert!(TodayRow::from_document(&document).unwrap() == value);
    let TodayRow::Profile {
        event_id,
        summary,
        metadata,
    } = value
    else {
        panic!("profile row");
    };
    assert_eq!(event_id, id);
    assert_eq!(summary.website.as_deref(), Some("https://example.com"));
    assert_eq!(metadata["custom"]["flag"], true);
    assert_eq!(document.key(), format!("profile.{}", summary.author_pubkey));
}

#[tokio::test]
async fn row_keys_reject_a_payload_from_another_card_or_schema() {
    let value = row(
        signed(1, vec![], "Synthetic update", 2_000_000_000),
        &context(None, 1),
    )
    .await
    .unwrap();
    let document = value.document().unwrap();
    assert!(document.key().starts_with("card."));
    let wrong = ProjectionDocument::new("card.foreign".into(), document.value().to_vec()).unwrap();
    assert!(matches!(
        TodayRow::from_document(&wrong),
        Err(TodayError::CorruptProjection)
    ));
    let mut wire: serde_json::Value = serde_json::from_slice(document.value()).unwrap();
    wire["schemaVersion"] = serde_json::json!(ROW_SCHEMA + 1);
    let future = ProjectionDocument::new(document.key().into(), encode(&wire).unwrap()).unwrap();
    assert!(matches!(
        TodayRow::from_document(&future),
        Err(TodayError::CorruptProjection)
    ));
}

#[tokio::test]
async fn row_classification_retains_locality_exclusion() {
    let event = signed(
        1,
        vec![vec!["location", "Victoria"]],
        "Synthetic local update",
        2_000_000_000,
    );
    assert!(
        row(event.clone(), &context(Some("Toronto"), 1))
            .await
            .is_none()
    );
    let TodayRow::Card(value) = row(event, &context(Some("Victoria"), 1)).await.unwrap() else {
        panic!("card row");
    };
    assert_eq!(value.locality[0].value, "victoria");
    assert_eq!(
        value.card.context_rank,
        super::super::super::ContextRank::LocalityMatch
    );
    assert!(value.card.rank.is_none());
}

#[tokio::test]
async fn root_index_does_not_duplicate_profile_state_in_thread_rows() {
    let root = "ab".repeat(32);
    let event = signed(
        1,
        vec![vec!["e", &root, "", "root"]],
        "Synthetic reply",
        2_000_000_000,
    );
    let value = row(event, &context(None, 1)).await.unwrap();
    let TodayRow::Thread(entry) = &value else {
        panic!("thread row");
    };
    assert_eq!(entry.reference.root, root);
    assert!(entry.author_profile.is_none());
    let document = value.clone().document().unwrap();
    assert!(document.key().starts_with(&format!(
        "thread.{}.",
        hex::encode(Sha256::digest(root.as_bytes()))
    )));
    assert!(TodayRow::from_document(&document).unwrap() == value);
}
