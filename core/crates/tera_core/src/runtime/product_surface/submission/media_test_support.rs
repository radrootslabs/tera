use std::{sync::Arc, time::Duration};

use radroots_blossom::{BlobDescriptor, BlobUrl, MediaType, Sha256};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

use super::{
    SubmissionMediaRequest, SubmissionMediaResponse, SubmissionReservationRequest, test_support::*,
};
use crate::TeraRuntime;

pub(super) async fn configure(runtime: &TeraRuntime, origin: &str) {
    runtime
        .configure_blossom(
            radroots_sdk::transport::BlossomHostKind::Simulator,
            radroots_sdk::transport::BlossomEndpointAuthority::LoopbackDevelopment,
            origin.into(),
            vec![],
        )
        .unwrap();
}

pub(super) fn upload(
    request: &SubmissionReservationRequest,
    revision: u64,
) -> SubmissionMediaRequest {
    SubmissionMediaRequest::new(
        request.clone(),
        revision,
        photo().0.opaque_reference,
        photo().1,
    )
    .unwrap()
}

pub(super) fn response(origin: &str) -> SubmissionMediaResponse {
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
    SubmissionMediaResponse::new(
        200,
        Some("application/json".into()),
        None,
        serde_json::to_vec(&descriptor).unwrap(),
    )
    .unwrap()
}

pub(super) async fn blob_server(bytes: Arc<[u8]>) -> (String, tokio::task::JoinHandle<String>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let task = tokio::spawn(async move {
        tokio::time::timeout(Duration::from_secs(10),async move {
            let (mut stream,_) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            let mut byte = [0;1];
            while !request.ends_with(b"\r\n\r\n") {
                assert!(request.len()<16_384);
                stream.read_exact(&mut byte).await.unwrap(); request.push(byte[0]);
            }
            let request = String::from_utf8(request).unwrap();
            assert!(request.starts_with("GET /"));
            let header = format!("HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",bytes.len());
            stream.write_all(header.as_bytes()).await.unwrap();stream.write_all(&bytes).await.unwrap();stream.shutdown().await.unwrap();
            request
        }).await.expect("bounded blob retrieval")
    });
    (origin, task)
}
