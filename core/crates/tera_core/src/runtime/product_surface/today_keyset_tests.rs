//! Real-provider keyset controls are added alongside the inherited cohorts.
use super::super::CANONICAL_TODAY_CARD_TYPES;
use super::tests::{context, signed, visible_admission};
use super::*;
const NOW: u64 = 2_000_000_000;

#[tokio::test]
async fn ten_thousand_mixed_keyset_pages_match_frozen_order_across_sqlite_reopen() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut runtime = runtime(sqlite, &root).await;
        add(
            &runtime,
            signed(0, vec![], r#"{"name":"Synthetic mixed farmer"}"#, NOW),
        )
        .await;
        let photo_hash = format!("x {}", hex::encode(Sha256::digest([7_u8; 123])));
        for index in 0..10_000 {
            let id = format!("mixed{index}");
            let content = format!("Synthetic mixed card{index}");
            let url = format!("https://media.example/mixed{index}.jpg");
            let imeta_url = format!("url {url}");
            let event = match index % 5 {
                0 => signed(1, vec![], &content, NOW),
                1 => signed(1, vec![vec!["t", "RADROOTS-ASK"]], &content, NOW),
                2 => signed(
                    1,
                    vec![vec![
                        "imeta",
                        &imeta_url,
                        &photo_hash,
                        "m image/jpeg",
                        "dim 10x20",
                        "size 123",
                        "alt Synthetic field photo",
                    ]],
                    &format!("{content} {url}"),
                    NOW,
                ),
                3 => signed(
                    30402,
                    vec![
                        vec!["d", &id],
                        vec!["title", &content],
                        vec!["summary", "Synthetic harvest"],
                        vec!["published_at", "2000000000"],
                        vec!["location", "Victoria"],
                        vec!["price", "3", "CAD"],
                        vec!["radroots:price_unit", "lb"],
                        vec!["status", "active"],
                    ],
                    "available",
                    NOW,
                ),
                _ if index % 10 == 4 => signed(
                    31922,
                    vec![
                        vec!["d", &id],
                        vec!["title", &content],
                        vec!["start", "2033-05-18"],
                    ],
                    "",
                    NOW,
                ),
                _ => signed(
                    31923,
                    vec![
                        vec!["d", &id],
                        vec!["title", &content],
                        vec!["start", "2000000200"],
                        vec!["D", "23148"],
                    ],
                    "",
                    NOW,
                ),
            };
            add(&runtime, event).await;
        }
        let selected = context(None, 1);
        let mut built = false;
        for _ in 0..64 {
            match runtime
                .phase1_refresh_today(&selected, NOW, TodayProjectionUpdate::Rebuild)
                .await
            {
                Ok(_) => {
                    built = true;
                    break;
                }
                Err(TodayError::ProjectionRebuilding) => continue,
                result => panic!("Actual mixed bounded build: {result:?}"),
            }
        }
        assert!(
            built,
            "The original bounded build attempts must finish the fixed mixed source"
        );
        let calendar = super::super::ViewerCalendarContext::new(NOW, "UTC").unwrap();
        let state = load_state(
            runtime.client.storage().unwrap(),
            &selected,
            projection_generation().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        let reference = ranked_cards(&state, &selected, &calendar).unwrap();
        drop(state);
        assert_eq!(reference.len(), 10_000);
        for family in CANONICAL_TODAY_CARD_TYPES {
            assert_eq!(
                reference
                    .iter()
                    .filter(|item| item.card.card_type == family)
                    .count(),
                2_000,
                "Every mixed family is actually represented"
            );
        }
        let mut request = TodayPageRequest::first(20, NOW, "UTC");
        let mut actual = Vec::new();
        let mut reopened = false;
        let mut complete = false;
        for _ in 0..1_000 {
            let page = runtime.phase1_today_page(&selected, request).await.unwrap();
            assert!(page.items.len() <= 20);
            assert_eq!(page.calendar, calendar);
            actual.extend(page.items);
            let Some(cursor) = page.next_cursor else {
                complete = true;
                break;
            };
            request = TodayPageRequest::after(20, cursor);
            if sqlite && !reopened && actual.len() >= 5_000 {
                let expected = runtime
                    .phase1_today_page(&selected, request.clone())
                    .await
                    .unwrap();
                runtime.shutdown().await.unwrap();
                runtime = performance_tests::reopen_counted_sqlite(&root).await;
                assert_eq!(
                    runtime
                        .phase1_today_page(&selected, request.clone())
                        .await
                        .unwrap(),
                    expected
                );
                reopened = true;
            }
        }
        assert!(complete, "Bounded pages must reach actual EOF");
        assert_eq!(actual.len(), reference.len());
        assert_eq!(
            actual
                .iter()
                .map(|item| item.card.card_id)
                .collect::<std::collections::HashSet<_>>()
                .len(),
            actual.len(),
            "Complete mixed traversal must not duplicate cards"
        );
        if let Some((position, (actual, expected))) = actual
            .iter()
            .zip(reference.iter())
            .enumerate()
            .find(|(_, (a, b))| a != b)
        {
            assert_eq!(
                actual, expected,
                "First mixed ordering mismatch at{position}; sqlite={sqlite}"
            );
        }
        assert_eq!(reopened, sqlite);
        println!(
            "Mixed keyset traversal: sqlite={sqlite}, cards={}, every_family=2000, page_size=20, reopened={reopened}",
            actual.len()
        );
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn false_calendar_candidates_advance_bounded_cursor_without_false_eof_or_lost_cards() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let runtime = runtime(sqlite, &root).await;
        for index in 0..600 {
            let id = format!("past{index}");
            add(
                &runtime,
                signed(
                    31922,
                    vec![
                        vec!["d", &id],
                        vec!["title", "Synthetic past event"],
                        vec!["start", "2020-01-01"],
                    ],
                    "",
                    NOW,
                ),
            )
            .await;
        }
        let selected = context(None, 1);
        for _ in 0..64 {
            match runtime
                .phase1_refresh_today(&selected, NOW, TodayProjectionUpdate::Rebuild)
                .await
            {
                Ok(_) => break,
                Err(TodayError::ProjectionRebuilding) => continue,
                result => panic!("actual bounded build: {result:?}"),
            }
        }
        let first = runtime
            .phase1_today_page(&selected, TodayPageRequest::first(20, NOW, "UTC"))
            .await
            .unwrap();
        assert!(first.items.is_empty());
        assert!(
            first.next_cursor.is_some(),
            "Bounded rejected candidates must expose progress"
        );
        let state = load_state(
            runtime.client.storage().unwrap(),
            &selected,
            projection_generation().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        let reference = ranked_cards(&state, &selected, &first.calendar).unwrap();
        let mut cursor = first.next_cursor;
        let mut actual = Vec::new();
        let mut complete = false;
        for _ in 0..100 {
            let value = cursor.take().unwrap();
            let page = runtime
                .phase1_today_page(&selected, TodayPageRequest::after(20, value.clone()))
                .await
                .unwrap();
            assert_ne!(
                page.next_cursor.as_ref(),
                Some(&value),
                "Each successful partial result must advance"
            );
            actual.extend(page.items);
            cursor = page.next_cursor;
            if cursor.is_none() {
                complete = true;
                break;
            }
        }
        assert!(complete);
        assert_eq!(actual, reference);
        assert_eq!(actual.len(), 600);
        runtime.shutdown().await.unwrap();
    }
}

async fn runtime(sqlite: bool, root: &tempfile::TempDir) -> TeraRuntime {
    if sqlite {
        performance_tests::reopen_counted_sqlite(root).await
    } else {
        TeraRuntime::test_memory().unwrap()
    }
}

async fn add(runtime: &TeraRuntime, event: radroots_event::SignedEvent) {
    EventStore::admit(
        runtime.client.storage().unwrap(),
        visible_admission(event, NOW * 1000),
    )
    .await
    .unwrap();
}

async fn fixture(runtime: &TeraRuntime) {
    let photo_hash = format!("x {}", hex::encode(Sha256::digest([7_u8; 123])));
    add(
        runtime,
        signed(0, vec![], r#"{"name":"Synthetic farmer"}"#, NOW),
    )
    .await;
    for index in 0..75 {
        add(
            runtime,
            signed(1, vec![], &format!("Synthetic stable tie{index}"), NOW),
        )
        .await;
    }
    add(
        runtime,
        signed(1, vec![vec!["t", "RADROOTS-ASK"]], "Synthetic ask", NOW),
    )
    .await;
    add(
        runtime,
        signed(
            1,
            vec![vec![
                "imeta",
                "url https://media.example/photo.jpg",
                &photo_hash,
                "m image/jpeg",
                "dim 10x20",
                "size 123",
                "alt Field photo",
            ]],
            "Synthetic photo https://media.example/photo.jpg",
            NOW,
        ),
    )
    .await;
    for index in 0..3 {
        let id = format!("date{index}");
        add(
            runtime,
            signed(
                31922,
                vec![
                    vec!["d", &id],
                    vec!["title", "Synthetic date event"],
                    vec!["start", "2033-05-18"],
                ],
                "",
                NOW,
            ),
        )
        .await;
    }
    add(
        runtime,
        signed(
            31923,
            vec![
                vec!["d", "timed"],
                vec!["title", "Synthetic timed event"],
                vec!["start", "2000000200"],
                vec!["D", "23148"],
            ],
            "",
            NOW,
        ),
    )
    .await;
    add(
        runtime,
        signed(
            30402,
            vec![
                vec!["d", "carrots"],
                vec!["title", "Synthetic carrots"],
                vec!["summary", "Fresh"],
                vec!["published_at", "2000000005"],
                vec!["location", "Victoria"],
                vec!["price", "3", "CAD"],
                vec!["radroots:price_unit", "lb"],
                vec!["status", "active"],
            ],
            "available",
            NOW + 5,
        ),
    )
    .await;
    add(
        runtime,
        signed(
            1,
            vec![vec!["location", "Toronto"]],
            "Synthetic foreign locality",
            NOW,
        ),
    )
    .await;
}

#[tokio::test]
async fn keyset_memory_sqlite_reopen_matches_frozen_reference_and_stable_ties() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let runtime = runtime(sqlite, &root).await;
        fixture(&runtime).await;
        for locality in [None, Some("Victoria"), Some("Toronto")] {
            let selected = context(locality, 1);
            runtime
                .phase1_refresh_today(&selected, NOW, TodayProjectionUpdate::Rebuild)
                .await
                .unwrap();
            let state = load_state(
                runtime.client.storage().unwrap(),
                &selected,
                projection_generation().unwrap(),
            )
            .await
            .unwrap()
            .unwrap();
            for zone in ["UTC", "America/Vancouver", "Pacific/Auckland"] {
                let calendar = super::super::ViewerCalendarContext::new(NOW, zone).unwrap();
                let reference = ranked_cards(&state, &selected, &calendar).unwrap();
                if locality.is_none() {
                    assert!(
                        CANONICAL_TODAY_CARD_TYPES.iter().all(|family| reference
                            .iter()
                            .any(|item| item.card.card_type == *family))
                    );
                }
                let mut request = TodayPageRequest::first(7, NOW, zone);
                let mut actual = Vec::new();
                for _ in 0..100 {
                    let page = runtime
                        .phase1_today_page(&selected, request.clone())
                        .await
                        .unwrap();
                    assert_eq!(
                        runtime.phase1_today_page(&selected, request).await.unwrap(),
                        page
                    );
                    assert!(page.items.len() <= 7);
                    assert_eq!(page.calendar, calendar);
                    actual.extend(page.items);
                    let Some(cursor) = page.next_cursor else {
                        break;
                    };
                    assert!(cursor.len() <= 1384 && cursor.starts_with("rrtc4:"));
                    request = TodayPageRequest::after(7, cursor);
                }
                assert_eq!(actual, reference, "{sqlite}/{locality:?}/{zone}");
            }
        }
        let selected = context(None, 1);
        let first = runtime
            .phase1_today_page(&selected, TodayPageRequest::first(7, NOW, "UTC"))
            .await
            .unwrap();
        let cursor = first.next_cursor.unwrap();
        let expected = runtime
            .phase1_today_page(&selected, TodayPageRequest::after(7, cursor.clone()))
            .await
            .unwrap();
        runtime.shutdown().await.unwrap();
        if sqlite {
            let reopened = performance_tests::reopen_counted_sqlite(&root).await;
            assert_eq!(
                reopened
                    .phase1_today_page(&selected, TodayPageRequest::after(7, cursor))
                    .await
                    .unwrap(),
                expected
            );
            reopened.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn dense_thread_preview_and_detail_preserve_every_reply_and_current_profiles() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let runtime = runtime(sqlite, &root).await;
        add(
            &runtime,
            signed(0, vec![], r#"{"name":"Synthetic reply author"}"#, NOW),
        )
        .await;
        let event = signed(1, vec![], "Synthetic thread root", NOW);
        let identity = event.id().to_hex();
        add(&runtime, event).await;
        for index in 0..130 {
            add(
                &runtime,
                signed(
                    1,
                    vec![vec!["e", &identity, "", "root"]],
                    &format!("Synthetic reply{index}"),
                    NOW,
                ),
            )
            .await;
        }
        let selected = context(None, 1);
        let first = runtime
            .phase1_today_page(&selected, TodayPageRequest::first(20, NOW, "UTC"))
            .await
            .unwrap();
        assert_eq!(first.items.len(), 1);
        assert!(first.next_cursor.is_none());
        let card = &first.items[0];
        assert_eq!(card.thread.len(), 8);
        let mut actual = card.thread.clone();
        let mut next = card.thread_next_cursor.clone();
        while let Some(cursor) = next {
            assert!(cursor.len() <= 1384);
            assert!(
                runtime
                    .phase1_today_page(&selected, TodayPageRequest::after(7, cursor.clone()))
                    .await
                    .is_err()
            );
            let page = runtime
                .phase1_today_thread_page(&selected, 7, &cursor)
                .await
                .unwrap();
            assert_eq!(
                runtime
                    .phase1_today_thread_page(&selected, 7, &cursor)
                    .await
                    .unwrap(),
                page
            );
            assert_eq!(page.card_id, card.card.card_id);
            actual.extend(page.items);
            next = page.next_cursor;
        }
        let state = load_state(
            runtime.client.storage().unwrap(),
            &selected,
            projection_generation().unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(actual, state.thread);
        assert_eq!(actual.len(), 130);
        assert!(actual.iter().all(
            |entry| entry.author_profile.as_ref().unwrap().name.as_deref()
                == Some("Synthetic reply author")
        ));
        let mut keys: Vec<_> = actual.iter().map(|entry| &entry.event_id).collect();
        keys.dedup();
        assert_eq!(keys.len(), 130);
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn keyset_cursor_checks_membership_full_local_digest_and_source_epoch() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let runtime = runtime(sqlite, &root).await;
        for body in ["alpha", "bravo", "charlie"] {
            add(&runtime, signed(1, vec![], body, NOW)).await;
        }
        let selected = context(None, 1);
        let first = runtime
            .phase1_today_page(&selected, TodayPageRequest::first(1, NOW, "UTC"))
            .await
            .unwrap();
        let cursor = first.next_cursor.unwrap();
        let decoded = page_cursor::decode(&cursor).unwrap();
        let legacy =
            TodayCursor::encode(&decoded.scope, TodayCursorPosition { rank: decoded.rank })
                .unwrap();
        assert!(matches!(
            runtime
                .phase1_today_page(
                    &selected,
                    TodayPageRequest::after(1, legacy.as_str().into())
                )
                .await,
            Err(TodayError::Cursor(CursorError::Version))
        ));
        let mut missing = decoded.rank;
        missing.card_id = CardId::parse(&"ab".repeat(32)).unwrap();
        let forged = page_cursor::encode(
            &decoded.scope,
            missing,
            decoded.fingerprint,
            None,
            decoded.seen,
        )
        .unwrap();
        assert!(matches!(
            runtime
                .phase1_today_page(&selected, TodayPageRequest::after(1, forged))
                .await,
            Err(TodayError::CursorPositionMissing)
        ));
        let mut bytes = cursor.clone().into_bytes();
        let final_index = bytes.len() - 2;
        bytes[final_index] = if bytes[final_index] == b'a' {
            b'b'
        } else {
            b'a'
        };
        assert!(
            runtime
                .phase1_today_page(
                    &selected,
                    TodayPageRequest::after(1, String::from_utf8(bytes).unwrap())
                )
                .await
                .is_err()
        );
        let mut local = sidecar::load(runtime.client.storage().unwrap(), &selected)
            .await
            .unwrap()
            .unwrap();
        local.overlays.insert(
            first.items[0].card.card_id.to_hex(),
            sidecar::OverlayEvidence {
                source_event: first.items[0].card.source_event_id.clone(),
                overlay: LocalAuthorOverlay {
                    operation_id: "Synthetic operation".into(),
                    state: "pending".into(),
                    source_draft_id: None,
                },
            },
        );
        let guard = runtime.today_projection_lock.lock().await;
        sidecar::save(&runtime, &selected, &local).await.unwrap();
        drop(guard);
        assert!(matches!(
            runtime
                .phase1_today_page(&selected, TodayPageRequest::after(1, cursor))
                .await,
            Err(TodayError::Cursor(CursorError::Stale))
        ));
        let fresh = runtime
            .phase1_today_page(&selected, TodayPageRequest::first(1, NOW, "UTC"))
            .await
            .unwrap();
        add(&runtime, signed(1, vec![], "new source", NOW + 1)).await;
        assert!(matches!(
            runtime
                .phase1_today_page(
                    &selected,
                    TodayPageRequest::after(1, fresh.next_cursor.unwrap())
                )
                .await,
            Err(TodayError::Cursor(CursorError::Stale))
        ));
        runtime.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn actual_provider_payload_preflight_accepts_exact_logical_maximum_and_refuses_one_more() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let runtime = runtime(sqlite, &root).await;
        let storage = runtime.client.storage().unwrap();
        let id = ProjectionId::parse("tera.test.page-allocation.v1").unwrap();
        let generation = ProjectionGeneration::new([17; 32]).unwrap();
        let maximum = (16 * 1024 * 1024 - 2 * 1024 - 4096) / 16;
        for (key, bytes) in [("exact", maximum), ("plus-one", maximum + 1)] {
            ProjectionStore::put_projection_document(
                storage,
                id.clone(),
                generation,
                ProjectionDocument::new(key.into(), vec![19; bytes]).unwrap(),
            )
            .await
            .unwrap();
            let mut budget = page_budget::Budget::default();
            let result = budget
                .document(storage, id.clone(), generation, key.into(), None)
                .await;
            if key == "exact" {
                assert_eq!(result.unwrap().unwrap().value().len(), maximum);
                assert!(budget.charge(1, 0).is_err());
            } else {
                assert!(matches!(
                    result,
                    Err(TodayError::Storage(
                        radroots_storage::Error::SpaceInsufficient
                    ))
                ));
            }
        }
        runtime.shutdown().await.unwrap();
    }
}
