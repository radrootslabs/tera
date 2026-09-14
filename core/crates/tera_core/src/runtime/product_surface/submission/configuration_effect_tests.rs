use futures_util::{SinkExt, StreamExt};
use std::{sync::Arc, time::Duration};
use tokio::{net::TcpListener, sync::Notify};
use tokio_tungstenite::{accept_async, tungstenite::Message};

use super::{operation_test_support::*, test_support::*, *};
use crate::runtime::product_surface::{Phase1OutboxState, PublicationDeliveryState};

#[tokio::test]
async fn configuration_removal_retains_issued_socket_and_late_ok_across_restart() {
    for (sqlite, change) in [false, true]
        .into_iter()
        .flat_map(|sqlite| ["relay", "read_only", "identity"].map(|change| (sqlite, change)))
    {
        let root = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let entered = Arc::new(Notify::new());
        let resume = Arc::new(Notify::new());
        let server = {
            let entered = entered.clone();
            let resume = resume.clone();
            tokio::spawn(async move {
                tokio::time::timeout(Duration::from_secs(15), async move {
                    let (stream, _) = listener.accept().await.unwrap();
                    let mut socket = accept_async(stream).await.unwrap();
                    while let Some(message) = socket.next().await {
                        if let Message::Text(text) = message.unwrap() {
                            let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                            if value[0] == "EVENT" {
                                entered.notify_one();
                                resume.notified().await;
                                socket
                                    .send(Message::Text(
                                        serde_json::json!(["OK", value[1]["id"], true, ""])
                                            .to_string()
                                            .into(),
                                    ))
                                    .await
                                    .unwrap();
                                return value[1].clone();
                            }
                        }
                    }
                    panic!("expected original EVENT");
                })
                .await
                .unwrap()
            })
        };
        let signer = CountingSigner::new();
        let runtime = runtime(sqlite.then_some(root.path()), signer.clone(), &url).await;
        let request = request();
        prepare(&runtime, &request, false).await;
        let task = {
            let runtime = runtime.clone();
            let request = request.clone();
            tokio::spawn(async move { runtime.submission_advance(&request, 1).await })
        };
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            restrict_settings(&runtime, &url, change),
        )
        .await
        .unwrap()
        .unwrap();
        runtime
            .configure_simulator_relays(vec![url.clone()])
            .await
            .unwrap();
        let stopped = runtime.submission_operation_status(&request).await.unwrap();
        assert_eq!(
            stopped.delivery_evidence().state,
            PublicationDeliveryState::Unknown
        );
        assert!(stopped.delivery_evidence().unresolved_claims);
        assert_eq!(stopped.state(), Phase1OutboxState::Cancelled);
        assert_eq!(
            runtime.submission_request_stop(&request).await.unwrap(),
            stopped
        );
        resume.notify_one();
        let _result = task.await.unwrap();
        let sent = server.await.unwrap();
        assert_eq!(sent["pubkey"], AUTHOR);
        let late = runtime.submission_operation_status(&request).await.unwrap();
        assert_eq!(
            late.delivery_evidence().state,
            PublicationDeliveryState::Accepted
        );
        assert_eq!(late.state(), Phase1OutboxState::Complete);
        assert_eq!(late.delivery_evidence().retained_facts, 1);
        assert_eq!(
            late.delivery_evidence().stop_requested_at_unix_ms,
            stopped.delivery_evidence().stop_requested_at_unix_ms
        );
        assert_eq!(
            sent["id"],
            hex::encode(
                late.push()
                    .artifact()
                    .signed()
                    .unwrap()
                    .event()
                    .id()
                    .as_bytes()
            )
        );
        assert_eq!(signer.count(), 1);
        runtime.shutdown().await.unwrap();
        drop(runtime);
        if sqlite {
            let reopened =
                self::runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19998").await;
            assert_eq!(
                reopened
                    .submission_operation_status(&request)
                    .await
                    .unwrap(),
                late
            );
            assert_eq!(
                reopened.submission_request_stop(&request).await.unwrap(),
                late
            );
            assert_eq!(
                reopened
                    .submission_advance(&request, late.intent().revision().get())
                    .await
                    .unwrap_err(),
                SubmissionOperationError::Stopped
            );
            reopened.shutdown().await.unwrap();
        }
    }
}

async fn restrict_settings(
    runtime: &crate::TeraRuntime,
    url: &str,
    change: &str,
) -> Result<(), crate::runtime::product_surface::SettingsError> {
    use crate::runtime::product_surface::{
        IdentityLockState, IdentityRecord, IdentityState, MobileNetworkEnvironment,
        RelayAccessPreference, RelayEndpointPreference, RelayPreferences, ReplaceMobileSettings,
    };
    let settings = runtime.phase1_settings().await?;
    let revision = settings.revision();
    let changed = if change == "identity" {
        settings.with_identity(
            IdentityState::new(
                vec![IdentityRecord::new("replacement", OTHER).unwrap()],
                Some("replacement".into()),
                IdentityLockState::Locked,
                None,
            )
            .unwrap(),
        )
    } else {
        let (url, access) = if change == "read_only" {
            (url, RelayAccessPreference::ReadOnly)
        } else {
            ("ws://127.0.0.1:19998", RelayAccessPreference::ReadWrite)
        };
        settings
            .with_blossom(crate::runtime::product_surface::BlossomPreferences::simulator_default())
            .with_relays(
                RelayPreferences::new(
                    MobileNetworkEnvironment::Simulator,
                    vec![
                        RelayEndpointPreference::new(
                            MobileNetworkEnvironment::Simulator,
                            url,
                            access,
                        )
                        .unwrap(),
                    ],
                )
                .unwrap(),
            )
    };
    runtime
        .phase1_replace_settings(ReplaceMobileSettings::new(revision, changed)?)
        .await?;
    Ok(())
}

#[tokio::test]
async fn configuration_change_during_upload_authorization_retains_stop_without_issuing_job() {
    use super::media_test_support::{configure, upload};
    use std::sync::atomic::Ordering;
    let signer = CountingSigner::new();
    signer.pause.store(true, Ordering::SeqCst);
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    configure(&runtime, "http://127.0.0.1:3000").await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let before = runtime.submission_operation_status(&request).await.unwrap();
    let task = {
        let runtime = runtime.clone();
        let request = request.clone();
        tokio::spawn(async move {
            runtime
                .submission_prepare_native_upload(upload(&request, 1))
                .await
        })
    };
    tokio::time::timeout(Duration::from_secs(5), signer.entered.notified())
        .await
        .unwrap();
    tokio::time::timeout(
        Duration::from_secs(2),
        configure(&runtime, "http://127.0.0.1:3001"),
    )
    .await
    .unwrap();
    configure(&runtime, "http://127.0.0.1:3000").await;
    signer.resume.notify_one();
    assert!(matches!(
        task.await.unwrap(),
        Err(SubmissionOperationError::Stopped)
    ));
    let stopped = runtime.submission_operation_status(&request).await.unwrap();
    assert_eq!(stopped.intent(), before.intent());
    assert_eq!(stopped.media(), before.media());
    assert_eq!(stopped.captured(), before.captured());
    assert!(
        stopped
            .delivery_evidence()
            .stop_requested_at_unix_ms
            .is_some()
    );
    assert_eq!(signer.count(), 1);
    assert!(matches!(
        runtime
            .submission_prepare_native_upload(upload(&request, 1))
            .await,
        Err(SubmissionOperationError::Stopped)
    ));
    runtime.shutdown().await.unwrap();
}
