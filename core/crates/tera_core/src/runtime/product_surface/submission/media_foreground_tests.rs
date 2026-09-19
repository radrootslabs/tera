use std::{sync::atomic::Ordering, time::Duration};

use radroots_blossom::{BlobDescriptor, BlobUrl, MediaType, Sha256};
use radroots_sdk::transport::{
    BlossomConfig, BlossomEndpointAuthority, BlossomHostKind, BlossomProfile,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};

use super::{media_test_support::*, operation_test_support::*, test_support::*, *};
use crate::runtime::product_surface::Phase1MediaStage;

#[tokio::test]
async fn foreground_upload_preserves_frozen_parent_and_verifies_exact_bytes_after_reopen() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let runtime = runtime(
            sqlite.then_some(root.path()),
            signer.clone(),
            "ws://127.0.0.1:19999",
        )
        .await;
        let (origin, server) = foreground_server().await;
        configure(&runtime, &origin).await;
        let request = request();
        prepare(&runtime, &request, true).await;
        let original = runtime.submission_operation_status(&request).await.unwrap();
        let ready = runtime
            .submission_upload_media(upload(&request, 1))
            .await
            .unwrap();
        assert_eq!(server.await.unwrap(), ["PUT", "GET"]);
        assert_eq!(ready.media()[0].stage(), Phase1MediaStage::Verified);
        assert_eq!(ready.captured(), original.captured());
        assert_eq!(
            ready.receipt().operation_id(),
            original.receipt().operation_id()
        );
        assert_eq!(ready.push(), original.push());
        assert_eq!(signer.count(), 1);
        assert_eq!(*signer.kinds.lock().unwrap(), [24242]);
        assert_redacted(&ready);
        runtime.shutdown().await.unwrap();
        drop(runtime);
        if sqlite {
            let config = BlossomConfig::from_profile(
                BlossomProfile::new(
                    BlossomHostKind::Simulator,
                    BlossomEndpointAuthority::LoopbackDevelopment,
                    &origin,
                    std::iter::empty::<&str>(),
                )
                .unwrap(),
            );
            let reopened = runtime_with_blossom(
                Some(root.path()),
                signer.clone(),
                "ws://127.0.0.1:19999",
                config,
            )
            .await;
            assert_eq!(
                reopened
                    .submission_operation_status(&request)
                    .await
                    .unwrap(),
                ready
            );
            reopened.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn foreground_stop_during_authorization_cannot_start_transport() {
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    let request = request();
    prepare(&runtime, &request, true).await;
    signer.pause.store(true, Ordering::SeqCst);
    let task = {
        let runtime = runtime.clone();
        let input = upload(&request, 1);
        tokio::spawn(async move { runtime.submission_upload_media(input).await })
    };
    tokio::time::timeout(Duration::from_secs(5), signer.entered.notified())
        .await
        .unwrap();
    let stopped = runtime.submission_request_stop(&request).await.unwrap();
    signer.resume.notify_one();
    assert_eq!(
        task.await.unwrap().err().unwrap(),
        SubmissionOperationError::Stopped
    );
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        stopped
    );
    assert_eq!(signer.count(), 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn foreground_does_not_replace_an_unreconciled_native_attempt() {
    let signer = CountingSigner::new();
    let runtime = runtime(None, signer.clone(), "ws://127.0.0.1:19999").await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let (uploading, _) = runtime
        .submission_prepare_native_upload(upload(&request, 1))
        .await
        .unwrap();
    assert_eq!(
        runtime
            .submission_upload_media(upload(&request, uploading.intent().revision().get()))
            .await
            .err()
            .unwrap(),
        SubmissionOperationError::InvalidMedia
    );
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        uploading
    );
    assert_eq!(signer.count(), 1);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn offline_foreground_preserves_parent_without_hidden_replacement_or_publication() {
    // Reserve an endpoint without a listener so another process cannot acquire it.
    // A bounded fixture timeout covers platforms that drop rather than refuse SYNs.
    let unavailable = tokio::net::TcpSocket::new_v4().unwrap();
    unavailable.bind("127.0.0.1:0".parse().unwrap()).unwrap();
    let origin = format!("http://{}", unavailable.local_addr().unwrap());
    let signer = CountingSigner::new();
    let config = BlossomConfig::from_profile(
        BlossomProfile::new(
            BlossomHostKind::Simulator,
            BlossomEndpointAuthority::LoopbackDevelopment,
            &origin,
            std::iter::empty::<&str>(),
        )
        .unwrap(),
    )
    .with_network_policy(
        Duration::from_millis(100),
        Duration::from_secs(1),
        1,
        Duration::from_millis(1),
    )
    .unwrap();
    let runtime = runtime_with_blossom(None, signer.clone(), "ws://127.0.0.1:19999", config).await;
    let request = request();
    prepare(&runtime, &request, true).await;
    let original = runtime.submission_operation_status(&request).await.unwrap();
    assert!(
        runtime
            .submission_upload_media(upload(&request, 1))
            .await
            .is_err()
    );
    let paused = runtime.submission_operation_status(&request).await.unwrap();
    assert_eq!(paused.captured(), original.captured());
    assert_eq!(paused.push(), original.push());
    assert_eq!(paused.receipt(), original.receipt());
    assert_eq!(paused.media()[0].stage(), Phase1MediaStage::Failed);
    assert!(paused.media()[0].orphan().is_some());
    assert_eq!(*signer.kinds.lock().unwrap(), [24242]);
    let revision = paused.intent().revision().get();
    assert!(
        runtime
            .submission_upload_media(upload(&request, revision))
            .await
            .is_err()
    );
    assert!(
        runtime
            .submission_prepare_native_upload(upload(&request, revision))
            .await
            .is_err()
    );
    assert_eq!(signer.count(), 1);
    assert_eq!(
        runtime.submission_operation_status(&request).await.unwrap(),
        paused
    );
    runtime.shutdown().await.unwrap();
}

async fn foreground_server() -> (String, tokio::task::JoinHandle<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let (photo, bytes) = photo();
    let sha = Sha256::digest(&bytes);
    let descriptor = BlobDescriptor::new(
        BlobUrl::parse(&format!("{origin}/{sha}.png")).unwrap(),
        sha,
        photo.byte_size,
        MediaType::parse("image/png").unwrap(),
        NOW / 1000,
    )
    .unwrap();
    let task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(10), async move {
            let mut methods = Vec::new();
            for method in ["PUT", "GET"] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let header = read_header(&mut stream).await;
                assert!(header.starts_with(&format!("{method} /")));
                if method == "PUT" {
                    assert!(
                        header
                            .to_ascii_lowercase()
                            .contains("authorization: nostr ")
                    );
                    let length = header
                        .lines()
                        .find_map(|line| {
                            line.to_ascii_lowercase()
                                .strip_prefix("content-length:")
                                .map(|value| value.trim().parse::<usize>().unwrap())
                        })
                        .unwrap();
                    assert_eq!(length, bytes.len());
                    let mut uploaded = vec![0; length];
                    stream.read_exact(&mut uploaded).await.unwrap();
                    assert_eq!(uploaded, bytes.as_ref());
                    respond(
                        &mut stream,
                        "application/json",
                        &serde_json::to_vec(&descriptor).unwrap(),
                    )
                    .await;
                } else {
                    assert!(header.starts_with(&format!("GET /{sha}.png ")));
                    respond(&mut stream, "image/png", &bytes).await;
                }
                methods.push(method.to_owned());
            }
            methods
        })
        .await
        .unwrap()
    });
    (origin, task)
}

async fn read_header(stream: &mut TcpStream) -> String {
    let mut request = Vec::new();
    let mut byte = [0; 1];
    while !request.ends_with(b"\r\n\r\n") {
        assert!(request.len() < 16_384);
        stream.read_exact(&mut byte).await.unwrap();
        request.push(byte[0]);
    }
    String::from_utf8(request).unwrap()
}

async fn respond(stream: &mut TcpStream, media_type: &str, body: &[u8]) {
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {media_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(header.as_bytes()).await.unwrap();
    stream.write_all(body).await.unwrap();
    stream.shutdown().await.unwrap();
}
