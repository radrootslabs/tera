use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::io::AsyncWriteExt;

async fn pressure_server(listener: TcpListener, oversized: bool, consumed: Arc<AtomicUsize>) {
    let (stream, _) = listener.accept().await.unwrap();
    let mut socket = accept_async(stream).await.unwrap();
    while let Some(message) = socket.next().await {
        let Ok(Message::Text(text)) = message else {
            continue;
        };
        let values: Value = serde_json::from_str(&text).unwrap();
        if values[0] != "REQ" {
            continue;
        }
        if oversized {
            // Deliver the actual complete unmasked text-frame header with a
            // declared payload one byte above the existing 512 KiB ceiling.
            // The client must reject before requiring/allocating that payload.
            // Avoid claiming a complete payload send after a racing close.
            let mut header = vec![0x81, 127];
            header.extend_from_slice(&(512_u64 * 1024 + 1).to_be_bytes());
            socket.get_mut().write_all(&header).await.unwrap();
            socket.get_mut().flush().await.unwrap();
            consumed.store(1, Ordering::SeqCst);
            let _ = socket.next().await;
            return;
        }
        let malformed = serde_json::to_string(&(
            "EVENT",
            &values[1],
            serde_json::json!({"invalid_event": "x".repeat(384 * 1024)}),
        ))
        .unwrap();
        assert!(malformed.len() < 512 * 1024);
        // More than the existing aggregate 8 MiB wire inventory. Invalid
        // events consume work even though no application event is admitted.
        for sequence in 0_u32..24 {
            if socket
                .send(Message::Text(malformed.clone().into()))
                .await
                .is_err()
            {
                return;
            }
            let marker = sequence.to_be_bytes().to_vec();
            if socket
                .send(Message::Ping(marker.clone().into()))
                .await
                .is_err()
            {
                return;
            }
            loop {
                match socket.next().await {
                    Some(Ok(Message::Pong(value))) if value.as_ref() == marker.as_slice() => {
                        consumed.fetch_add(1, Ordering::SeqCst);
                        break;
                    }
                    Some(Ok(Message::Close(_))) | Some(Err(_)) | None => return,
                    _ => {
                        if socket.flush().await.is_err() {
                            return;
                        }
                    }
                }
            }
        }
        let _ = socket
            .send(Message::Text(
                serde_json::to_string(&("EOSE", &values[1])).unwrap().into(),
            ))
            .await;
        return;
    }
}

async fn pressure_preserves_cached_projection(oversized: bool) {
    let (listener, url) = listener().await;
    let consumed = Arc::new(AtomicUsize::new(0));
    let server = tokio::spawn(pressure_server(listener, oversized, consumed.clone()));
    let runtime = live_runtime(std::slice::from_ref(&url), 1);
    let selected = selected(vec![url]);
    ingest(
        &runtime,
        &selected,
        signed(1, vec![], "cached under hostile wire", 2_000_000_000),
        2_000_000_100,
    )
    .await;
    let receipt = tokio::time::timeout(
        Duration::from_secs(5),
        runtime.phase1_sync_today(&selected, 2_000_000_200, TodayProjectionUpdate::Incremental),
    )
    .await
    .unwrap()
    .unwrap();
    assert_ne!(receipt.relay_state, TodayRelaySyncState::Complete);
    assert_eq!(receipt.events_observed, 0);
    assert!(receipt.pages_fetched <= TODAY_SYNC_MAX_PAGES);
    assert_eq!(receipt.projection.visible_cards, 1);
    if oversized {
        assert_eq!(consumed.load(Ordering::SeqCst), 1);
    } else {
        assert!(consumed.load(Ordering::SeqCst) >= 21);
        assert_eq!(
            receipt.targets[0].final_state,
            Some(TodayTargetSyncState::Partial)
        );
    }
    let page = runtime
        .phase1_today_page(&selected, TodayPageRequest::first(10, 2_000_000_201, "UTC"))
        .await
        .unwrap();
    assert_eq!(page.items.len(), 1);
    // Stop only the bounded test fixture; production cleanup is performed by
    // the shared request owner and existing cancellation tests qualify it.
    server.abort();
    let outcome = tokio::time::timeout(Duration::from_secs(5), server)
        .await
        .unwrap();
    assert!(outcome.is_ok() || outcome.unwrap_err().is_cancelled());
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn malformed_raw_ingress_budget_cannot_turn_eose_into_complete_today() {
    pressure_preserves_cached_projection(false).await;
}

#[tokio::test]
async fn oversized_raw_frame_cannot_replace_or_poison_cached_today() {
    pressure_preserves_cached_projection(true).await;
}
