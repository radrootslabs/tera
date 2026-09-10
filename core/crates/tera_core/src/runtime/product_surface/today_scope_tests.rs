use super::tests::{context, ingest, keys, signed};
use super::*;

const NOW: u64 = 2_000_000_000;

#[tokio::test]
async fn arrivals_and_authorized_deletion_produce_complete_new_keysets() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let selected = context(None, 1);
    let removed = signed(1, vec![], "remove-me", NOW);
    let removed_id = removed.id().to_hex();
    ingest(&runtime, &selected, removed, NOW).await;
    for body in ["alpha", "bravo"] {
        ingest(&runtime, &selected, signed(1, vec![], body, NOW), NOW).await;
    }
    let first = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(1, NOW + 10, "UTC"))
        .await
        .unwrap();
    ingest(
        &runtime,
        &selected,
        signed(1, vec![], "late-equal-time", NOW),
        NOW + 1,
    )
    .await;
    ingest(
        &runtime,
        &selected,
        signed(5, vec![vec!["e", &removed_id]], "", NOW + 2),
        NOW + 2,
    )
    .await;
    let fresh = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(1, NOW + 10, "UTC"))
        .await
        .unwrap();
    assert_ne!(fresh.next_cursor, first.next_cursor);
    let mut items = fresh.items;
    let mut cursor = fresh.next_cursor;
    while let Some(value) = cursor {
        let request = TodayPageRequest::after(1, value);
        let page = runtime
            .phase1_today_page(&selected, request.clone())
            .await
            .unwrap();
        assert_eq!(
            runtime.phase1_today_page(&selected, request).await.unwrap(),
            page
        );
        items.extend(page.items);
        assert!(items.len() <= 3);
        cursor = page.next_cursor;
    }
    assert_eq!(items.len(), 3);
    assert!(
        items
            .iter()
            .all(|item| item.card.source_event_id != removed_id)
    );
    let contents = items
        .iter()
        .map(|item| item.card.content.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(
        contents,
        ["alpha", "bravo", "late-equal-time"].into_iter().collect()
    );
}

#[tokio::test]
async fn same_identity_and_generation_do_not_authorize_another_query() {
    let runtime = TeraRuntime::test_memory().expect("runtime");
    let first = context(Some("first"), 1);
    let second = context(Some("second"), 1);
    for locality in ["first", "second"] {
        for body in ["alpha", "bravo"] {
            ingest(
                &runtime,
                &first,
                signed(1, vec![vec!["g", locality]], body, NOW),
                NOW,
            )
            .await;
        }
    }
    let page = runtime
        .phase1_today_page(&first, TodayPageRequest::first(1, NOW, "UTC"))
        .await
        .unwrap();
    let cursor = page.next_cursor.expect("second matching item");
    let result = runtime
        .phase1_today_page(&second, TodayPageRequest::after(1, cursor.clone()))
        .await;
    assert!(
        matches!(
            result,
            Err(TodayError::Cursor(CursorError::ContextMismatch))
        ),
        "{result:?}"
    );
    let original = runtime
        .phase1_today_page(&first, TodayPageRequest::after(1, cursor.clone()))
        .await
        .unwrap();
    let other = runtime
        .phase1_today_page(&second, TodayPageRequest::first(1, NOW, "UTC"))
        .await
        .unwrap();
    assert_eq!(other.items.len(), 1);
    assert!(
        other.next_cursor.is_some(),
        "both second-locality records must be projected"
    );
    let other_rest = runtime
        .phase1_today_page(
            &second,
            TodayPageRequest::after(1, other.next_cursor.clone().unwrap()),
        )
        .await
        .unwrap();
    assert_eq!(other_rest.items.len(), 1);
    assert!(other_rest.next_cursor.is_none());
    let second_ids = [
        other.items[0].card.source_event_id.clone(),
        other_rest.items[0].card.source_event_id.clone(),
    ]
    .into_iter()
    .collect::<std::collections::BTreeSet<_>>();
    let expected_ids = ["alpha", "bravo"]
        .map(|body| {
            signed(1, vec![vec!["g", "second"]], body, NOW)
                .id()
                .to_hex()
        })
        .into_iter()
        .collect();
    assert_eq!(second_ids, expected_ids);
    assert_ne!(other.next_cursor.as_ref(), Some(&cursor));
    let scope = TodayCursor::scope(&cursor).unwrap();
    let position = TodayCursor::decode(&cursor, &scope).unwrap();
    let retained = load_snapshot(
        runtime.client.storage().unwrap(),
        projection_id().unwrap(),
        projection_generation().unwrap(),
        &scope,
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        page_from_snapshot(retained, scope, Some(position.rank), 1).unwrap(),
        original
    );
    assert!(matches!(
        runtime
            .phase1_today_page(&first, TodayPageRequest::after(1, cursor.clone()))
            .await,
        Err(TodayError::Cursor(CursorError::Stale))
    ));
    for changed in [
        LocalNetwork {
            label: "Changed".into(),
            ..first.clone()
        },
        LocalNetwork {
            relay_urls: vec!["wss://another.example".into()],
            ..first.clone()
        },
        LocalNetwork {
            followed_authors: vec![keys().public_key().to_string()],
            ..first.clone()
        },
    ] {
        assert!(matches!(
            runtime
                .phase1_today_page(&changed, TodayPageRequest::after(1, cursor.clone()))
                .await,
            Err(TodayError::Cursor(CursorError::ContextMismatch))
        ));
    }
}

#[tokio::test]
async fn tied_keysets_are_exact_repeatable_and_independent_of_ingest_order() {
    let selected = context(Some("first"), 1);
    let mut expected = None;
    for reverse in [false, true] {
        let runtime = TeraRuntime::test_memory().unwrap();
        let mut events = (0..9)
            .map(|index| {
                signed(
                    1,
                    if index < 6 {
                        vec![vec!["g", "first"]]
                    } else {
                        vec![]
                    },
                    &format!("post-{index}"),
                    NOW,
                )
            })
            .collect::<Vec<_>>();
        if reverse {
            events.reverse();
        }
        for event in events {
            ingest(&runtime, &selected, event, NOW).await;
        }
        let all = runtime
            .phase1_today_page(&selected, TodayPageRequest::first(100, NOW, "UTC"))
            .await
            .unwrap();
        let ids = all
            .items
            .iter()
            .map(|item| item.card.card_id)
            .collect::<Vec<_>>();
        assert_eq!(ids.len(), 9);
        assert_eq!(
            ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
            9
        );
        assert!(
            all.items
                .windows(2)
                .all(|pair| pair[0].card.rank < pair[1].card.rank)
        );
        if let Some(expected) = &expected {
            assert_eq!(&ids, expected);
        } else {
            expected = Some(ids.clone());
        }
        for limit in [1, 2, 4, 8, 9, 100] {
            let mut request = TodayPageRequest::first(limit, NOW, "UTC");
            let mut observed = Vec::new();
            loop {
                let page = runtime
                    .phase1_today_page(&selected, request.clone())
                    .await
                    .unwrap();
                assert_eq!(
                    runtime
                        .phase1_today_page(&selected, request.clone())
                        .await
                        .unwrap(),
                    page
                );
                observed.extend(page.items.iter().map(|item| item.card.card_id));
                assert!(observed.len() <= ids.len());
                let Some(cursor) = page.next_cursor else {
                    break;
                };
                request = TodayPageRequest::after(limit, cursor);
            }
            assert_eq!(observed, ids);
        }
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn authenticated_accounts_and_retired_stores_cannot_share_cursor_authority() {
    use crate::runtime::{
        builder::RuntimeBuilder,
        store::{MobileUserStoreConfig, ProtectedDataAvailability},
    };
    let root = tempfile::tempdir().unwrap();
    let selected = context(None, 1);
    let first_owner = keys().public_key().to_string();
    let second_owner = nostr::Keys::parse(&format!("{:064x}", 2))
        .unwrap()
        .public_key()
        .to_string();
    let mut first_cursor: Option<String> = None;
    for (index, owner, generation) in [
        (0, &first_owner, "31"),
        (1, &second_owner, "31"),
        (2, &first_owner, "32"),
    ] {
        let store = MobileUserStoreConfig::from_encoded(
            root.path().join(index.to_string()),
            owner,
            &generation.repeat(32),
            NOW * 1000,
            ProtectedDataAvailability::Available,
        )
        .unwrap();
        std::fs::create_dir_all(store.owner_directory()).unwrap();
        let runtime = RuntimeBuilder::new(store).build().await.unwrap();
        for body in ["alpha", "bravo"] {
            ingest(&runtime, &selected, signed(1, vec![], body, NOW), NOW).await;
        }
        let page = runtime
            .phase1_today_page(&selected, TodayPageRequest::first(1, NOW, "UTC"))
            .await
            .unwrap();
        let own_cursor = page.next_cursor.unwrap();
        assert_eq!(
            runtime
                .phase1_today_page(&selected, TodayPageRequest::after(1, own_cursor.clone()))
                .await
                .unwrap()
                .items
                .len(),
            1
        );
        if let Some(cursor) = &first_cursor {
            let result = runtime
                .phase1_today_page(&selected, TodayPageRequest::after(1, cursor.clone()))
                .await;
            assert!(matches!(
                (index, result),
                (1, Err(TodayError::Cursor(CursorError::ContextMismatch)))
                    | (2, Err(TodayError::Cursor(CursorError::Stale)))
            ));
        } else {
            first_cursor = Some(own_cursor);
        }
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn corrupt_snapshot_order_and_legacy_unbound_cache_fail_closed() {
    let runtime = TeraRuntime::test_memory().unwrap();
    let selected = context(None, 1);
    for body in ["alpha", "bravo", "charlie"] {
        ingest(&runtime, &selected, signed(1, vec![], body, NOW), NOW).await;
    }
    let state = load_state(
        runtime.client.storage().unwrap(),
        &selected,
        projection_generation().unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    let snapshot = frozen_snapshot(
        &state,
        &selected,
        &crate::runtime::product_surface::ViewerCalendarContext::new(NOW, "UTC").unwrap(),
        paging_scope::query_scope(&selected, None).unwrap(),
    )
    .unwrap();
    assert!(paging_scope::validate_order(&snapshot.items).is_ok());
    for index in 0..6 {
        let mut invalid = snapshot.items.clone();
        match index {
            0 => invalid.swap(0, 1),
            1 => invalid.insert(1, invalid[0].clone()),
            2 => invalid[0].card.rank = None,
            3 => invalid[0].card.rank.as_mut().unwrap().schema_version += 1,
            4 => invalid[0].card.rank.as_mut().unwrap().algorithm_version += 1,
            _ => invalid[0].card.rank.as_mut().unwrap().card_id = invalid[1].card.card_id,
        }
        assert!(matches!(
            paging_scope::validate_order(&invalid),
            Err(TodayError::CorruptProjection)
        ));
    }
    let mut legacy = serde_json::to_value(&snapshot).unwrap();
    legacy["schemaVersion"] = 1.into();
    legacy.as_object_mut().unwrap().remove("queryScope");
    let bytes = serde_json::to_vec(&legacy).unwrap();
    assert!(matches!(
        decode_snapshot(&bytes),
        Err(TodayError::Cursor(CursorError::Stale))
    ));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&bytes).unwrap(),
        legacy
    );
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(100, NOW, "UTC"))
        .await
        .unwrap();
    assert_eq!(page.items, snapshot.items);

    // Genuine pre-binding projection bytes still verify with their old hash.
    // Scope repair retains local author overlays instead of discarding the cache.
    let mut unbound = state.clone();
    unbound.query_scope = None;
    let overlay = LocalAuthorOverlay {
        operation_id: "retained-operation".into(),
        state: "delivered".into(),
    };
    let overlaid_id = unbound.cards[0].card.card_id;
    unbound
        .overlays
        .insert(overlaid_id.to_hex(), overlay.clone());
    unbound.content_generation = content_generation(&unbound).unwrap();
    let old_bytes = encode(&unbound).unwrap();
    assert!(
        !std::str::from_utf8(&old_bytes)
            .unwrap()
            .contains("queryScope")
    );
    assert_eq!(decode_state(&old_bytes).unwrap(), unbound);
    store_state(
        runtime.client.storage().unwrap(),
        &selected,
        projection_generation().unwrap(),
        &unbound,
    )
    .await
    .unwrap();
    let repaired = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(100, NOW, "UTC"))
        .await
        .unwrap();
    assert_eq!(repaired.items.len(), snapshot.items.len());
    assert_eq!(
        repaired
            .items
            .iter()
            .find(|item| item.card.card_id == overlaid_id)
            .unwrap()
            .local_overlay
            .as_ref(),
        Some(&overlay)
    );
    let rebound = load_state(
        runtime.client.storage().unwrap(),
        &selected,
        projection_generation().unwrap(),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(
        rebound.query_scope,
        Some(paging_scope::query_scope(&selected, None).unwrap())
    );
    assert_eq!(rebound.overlays, unbound.overlays);
    assert_eq!(rebound.media_cache, unbound.media_cache);
}
