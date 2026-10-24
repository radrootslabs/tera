use super::super::performance_tests::resume_bounded_work;
use super::super::tests::{context, signed, visible_admission};
use super::*;
const NOW: u64 = 2_000_000_000;

#[tokio::test]
async fn row_generations_equal_canonical_reference_for_five_families_dense_threads_and_calendar_locality()
 {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let runtime = if sqlite {
            super::super::performance_tests::reopen_counted_sqlite(&root).await
        } else {
            TeraRuntime::test_memory().unwrap()
        };
        let mut events = vec![
            signed(
                0,
                vec![],
                r#"{"name":"Synthetic farm","website":"https://example.com","custom":{"enabled":true}}"#,
                NOW,
            ),
            signed(1, vec![], "Synthetic ordinary update", NOW),
            signed(
                1,
                vec![vec![
                    "imeta",
                    "url https://media.example/photo.jpg",
                    "m image/jpeg",
                    "dim 10x20",
                    "size 123",
                    "alt Field photo",
                ]],
                "Synthetic photo https://media.example/photo.jpg",
                NOW + 1,
            ),
            signed(1, vec![vec!["t", "RADROOTS-ASK"]], "Synthetic ask", NOW + 2),
            signed(
                31922,
                vec![
                    vec!["d", "date"],
                    vec!["title", "Synthetic date event"],
                    vec!["start", "2033-05-18"],
                ],
                "",
                NOW + 3,
            ),
            signed(
                31923,
                vec![
                    vec!["d", "timed"],
                    vec!["title", "Synthetic timed event"],
                    vec!["start", "2000000200"],
                    vec!["D", "23148"],
                ],
                "",
                NOW + 4,
            ),
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
            signed(
                1,
                vec![vec!["location", "Toronto"]],
                "Synthetic excluded locality",
                NOW + 6,
            ),
        ];
        let thread_root = events[1].id().to_hex();
        for index in 0..130 {
            events.push(signed(
                1,
                vec![vec!["e", &thread_root, "", "root"]],
                &format!("Synthetic dense reply{index}"),
                NOW + 10,
            ));
        }
        let storage = runtime.client.storage().unwrap();
        for event in events {
            EventStore::admit(storage, visible_admission(event, NOW * 1_000))
                .await
                .unwrap();
        }
        let canonical = EventStore::rebuild_visibility(storage).await.unwrap();
        for locality in [None, Some("Victoria"), Some("Toronto")] {
            let selected = context(locality, 1);
            resume_bounded_work(
                || {
                    runtime.phase1_refresh_today(
                        &selected,
                        NOW + 100,
                        TodayProjectionUpdate::Rebuild,
                    )
                },
                |result| matches!(result, Err(TodayError::ProjectionRebuilding)),
            )
            .await
            .unwrap();
            let state = load_state(storage, &selected, projection_generation().unwrap())
                .await
                .unwrap()
                .unwrap();
            let status = EventStore::status(storage).await.unwrap();
            let visible = query_all_visible(storage).await.unwrap();
            assert_eq!(visible.len(), canonical.visible_event_ids().len());
            let reference = project_state(
                &selected,
                status.generation().as_bytes(),
                status.raw_events(),
                visible,
                BTreeMap::new(),
            )
            .unwrap();
            assert_eq!(state.cards, reference.cards);
            assert_eq!(state.profiles, reference.profiles);
            assert_eq!(state.thread, reference.thread);
            assert_eq!(
                state.quarantined_source_ids,
                reference.quarantined_source_ids
            );
            assert_eq!(state.thread.len(), 130);
            assert!(
                state
                    .thread
                    .iter()
                    .all(|entry| entry.reference.root == thread_root)
            );
            for zone in ["UTC", "America/Vancouver", "Pacific/Auckland"] {
                let calendar =
                    super::super::super::ViewerCalendarContext::new(NOW + 100, zone).unwrap();
                assert_eq!(
                    ranked_cards(&state, &selected, &calendar).unwrap(),
                    ranked_cards(&reference, &selected, &calendar).unwrap()
                );
            }
        }
        runtime.shutdown().await.unwrap();
    }
}
