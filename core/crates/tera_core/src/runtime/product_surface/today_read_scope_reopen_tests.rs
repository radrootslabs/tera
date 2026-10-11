use super::*;

fn other_author(event: &SignedEvent) -> SignedEvent {
    use nostr::secp256k1::{Keypair, Message, SECP256K1};
    let keys = nostr::Keys::parse(&"02".repeat(32)).unwrap();
    let mut wire = event.wire().clone();
    wire.pubkey = keys.public_key().to_string();
    let id = wire.computed_event_id().unwrap();
    wire.id = id.to_hex();
    wire.sig = SECP256K1
        .sign_schnorr_no_aux_rand(
            &Message::from_digest(*id.as_bytes()),
            &Keypair::from_secret_key(SECP256K1, keys.secret_key()),
        )
        .to_string();
    let raw = serde_json::to_string(&wire).unwrap();
    SignedEvent::from_wire_verified_id(wire, raw).unwrap()
}

#[tokio::test]
async fn sqlite_invalid_signature_and_wrong_author_deletion_cannot_hide_current_reads() {
    for invalid in [false, true] {
        for operation in 0..4 {
            let root = tempfile::tempdir().unwrap();
            let runtime = reopen_counted_sqlite(&root).await;
            let selected = context(None, 1);
            let event = signed(1, vec![], "scope retained", NOW);
            let expected = vec![event.id().to_hex()];
            ingest(&runtime, &selected, event, NOW).await;
            let ids = runtime
                .phase1_today_page(&selected, TodayPageRequest::first(100, NOW, "UTC"))
                .await
                .unwrap()
                .items
                .into_iter()
                .map(|card| card.card.card_id.to_hex())
                .collect::<Vec<_>>();
            let deletion = signed(5, vec![vec!["e", &expected[0]]], "", NOW + 1);
            let admission = if invalid {
                let mut wire = deletion.wire().clone();
                wire.sig = "0".repeat(128);
                let event = radroots_event_codec::decode::signed_event(
                    &serde_json::to_string(&wire).unwrap(),
                )
                .unwrap();
                assert!(verify_nip01_event(event.envelope().clone()).is_err());
                EventAdmission::raw(observed(event))
            } else {
                let event = other_author(&deletion);
                assert!(verify_nip01_event(event.envelope().clone()).is_ok());
                visible_admission(event, NOW * 1_000)
            };
            let receipt = EventStore::admit(runtime.client.storage().unwrap(), admission)
                .await
                .unwrap();
            runtime.shutdown().await.unwrap();
            let runtime = reopen_counted_sqlite(&root).await;
            assert_eq!(
                read_ids(&runtime, &selected, operation, &ids)
                    .await
                    .unwrap(),
                expected
            );
            let retained = EventStore::query_raw(
                runtime.client.storage().unwrap(),
                EventQuery::for_ids(
                    EventQueryBounds::first(1).unwrap(),
                    vec![*receipt.event_id()],
                )
                .unwrap(),
            )
            .await
            .unwrap();
            assert_eq!(retained.items()[0].position(), receipt.position());
            runtime.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn sqlite_thread_cursor_refuses_unprojected_deletion_and_reopens_current_detail() {
    let root = tempfile::tempdir().unwrap();
    let runtime = reopen_counted_sqlite(&root).await;
    let selected = context(None, 1);
    let event = signed(1, vec![], "scope root", NOW);
    let identity = event.id().to_hex();
    ingest(&runtime, &selected, event, NOW).await;
    ingest(
        &runtime,
        &selected,
        signed(0, vec![], r#"{"name":"Current thread author"}"#, NOW),
        NOW,
    )
    .await;
    let mut reply_ids = Vec::new();
    for index in 0..10 {
        let reply = signed(
            1,
            vec![vec!["e", &identity, "", "root"]],
            &format!("scope reply {index}"),
            NOW,
        );
        reply_ids.push(reply.id().to_hex());
        ingest(&runtime, &selected, reply, NOW).await;
    }
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(20, NOW, "UTC"))
        .await
        .unwrap();
    let cursor = page.items[0].thread_next_cursor.clone().unwrap();
    let removed = page.items[0].thread[0].event_id.clone();
    EventStore::admit(
        runtime.client.storage().unwrap(),
        visible_admission(
            signed(5, vec![vec!["e", &removed]], "", NOW + 1),
            NOW * 1_000,
        ),
    )
    .await
    .unwrap();
    runtime.shutdown().await.unwrap();
    let runtime = reopen_counted_sqlite(&root).await;
    assert!(matches!(
        runtime
            .phase1_today_thread_page(&selected, 10, &cursor)
            .await,
        Err(TodayError::Cursor(CursorError::Stale))
    ));
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(20, NOW + 10, "UTC"))
        .await
        .unwrap();
    let card = &page.items[0];
    let mut replies = card.thread.clone();
    let mut cursor = card.thread_next_cursor.clone();
    while let Some(value) = cursor {
        let next = runtime
            .phase1_today_thread_page(&selected, 10, &value)
            .await
            .unwrap();
        replies.extend(next.items);
        cursor = next.next_cursor;
    }
    let mut actual = replies
        .iter()
        .map(|reply| reply.event_id.clone())
        .collect::<Vec<_>>();
    let mut expected = reply_ids
        .into_iter()
        .filter(|id| id != &removed)
        .collect::<Vec<_>>();
    actual.sort();
    expected.sort();
    assert_eq!(actual, expected);
    assert!(replies.iter().all(|reply| {
        reply
            .author_profile
            .as_ref()
            .and_then(|profile| profile.name.as_deref())
            == Some("Current thread author")
    }));
    runtime.shutdown().await.unwrap();
}
