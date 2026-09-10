use super::tests::{context, ingest, signed};
use super::*;

const NOW: u64 = 2_000_000_100;

#[tokio::test]
async fn reconciliation_retains_loaded_identity_and_removes_author_deleted_content() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let context = context(None, 1);
    let first = signed(1, vec![], "first", NOW - 3);
    let removed = signed(1, vec![], "removed", NOW - 2);
    ingest(&runtime, &context, first, NOW).await;
    ingest(&runtime, &context, removed.clone(), NOW).await;
    let original = runtime
        .phase1_today_page(&context, TodayPageRequest::first(100, NOW))
        .await
        .unwrap();
    let ids = original
        .items
        .iter()
        .map(|c| c.card.card_id.to_hex())
        .collect::<Vec<_>>();
    let unchanged = runtime
        .phase1_today_reconcile(&context, NOW, &ids, Some(original.projection_generation))
        .await
        .unwrap();
    assert_eq!(unchanged.items, original.items);
    ingest(
        &runtime,
        &context,
        signed(1, vec![], "new arrival", NOW - 1),
        NOW,
    )
    .await;
    ingest(
        &runtime,
        &context,
        signed(
            5,
            vec![vec!["e", &removed.id().to_hex()], vec!["k", "1"]],
            "",
            NOW,
        ),
        NOW,
    )
    .await;
    assert!(matches!(
        runtime
            .phase1_today_reconcile(&context, NOW, &ids, Some(original.projection_generation))
            .await,
        Err(TodayError::Cursor(CursorError::Stale))
    ));
    let current = runtime
        .phase1_today_reconcile(&context, NOW, &ids, None)
        .await
        .unwrap();
    assert_ne!(
        current.projection_generation,
        original.projection_generation
    );
    assert_eq!(current.items.len(), 1);
    assert_eq!(current.items[0].card.content, "first");
    assert!(
        current
            .items
            .iter()
            .all(|c| ids.contains(&c.card.card_id.to_hex()))
    );
}

#[tokio::test]
async fn reconciliation_applies_full_context_and_rejects_unbounded_or_invalid_ids() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let context = context(None, 1);
    ingest(&runtime, &context, signed(1, vec![], "visible", NOW), NOW).await;
    let page = runtime
        .phase1_today_page(&context, TodayPageRequest::first(1, NOW))
        .await
        .unwrap();
    let id = page.items[0].card.card_id.to_hex();
    for ids in [
        vec![id.clone(); 101],
        vec![id.clone(), id.clone()],
        vec!["a".repeat(65)],
        vec!["G".repeat(64)],
    ] {
        assert!(matches!(
            runtime
                .phase1_today_reconcile(&context, NOW, &ids, None)
                .await,
            Err(TodayError::InvalidRequest)
        ));
    }
    assert!(matches!(
        runtime.phase1_today_reconcile(&context, 0, &[], None).await,
        Err(TodayError::InvalidRequest)
    ));
    let empty = runtime
        .phase1_today_reconcile(&context, NOW, &[], None)
        .await
        .unwrap();
    assert!(empty.items.is_empty());
    assert_eq!(empty.projection_generation, page.projection_generation);
    let mut changed = context.clone();
    changed.locality = Some("changed".into());
    assert!(matches!(
        runtime
            .phase1_today_reconcile(&changed, NOW, &[id], None)
            .await,
        Err(TodayError::Cursor(CursorError::Stale))
    ));
}
