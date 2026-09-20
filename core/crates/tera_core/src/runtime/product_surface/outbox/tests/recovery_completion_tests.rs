use super::*;
use crate::runtime::product_surface::SubmissionMediaResponse;
use crate::runtime::product_surface::recovery_completion::*;

#[tokio::test]
async fn legacy_recovery_completion_binds_saved_attempt_and_replays_without_current_configuration()
{
    use tokio::{
        io::{AsyncReadExt, AsyncWriteExt},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend_from_slice(&2u32.to_be_bytes());
    bytes.extend_from_slice(&2u32.to_be_bytes());
    let bytes: Arc<[u8]> = bytes.into();
    let hash = BlossomSha256::digest(&bytes);
    let kind = MediaType::parse("image/png").unwrap();
    let url = format!("{origin}/{hash}.png");
    let descriptor = BlobDescriptor::new(
        BlobUrl::parse(&url).unwrap(),
        hash,
        bytes.len() as u64,
        kind.clone(),
        1_784_347_100,
    )
    .unwrap();
    let checked = descriptor
        .clone()
        .approve_reference()
        .unwrap()
        .verify_bytes(&bytes, &kind)
        .unwrap();
    let media = Phase1MediaPrerequisite::new("media:legacy", &checked).unwrap();
    let image = AuthoredPostImage::new(
        AuthoredImage::try_from(checked).unwrap(),
        PostImageDimensions::new(2, 2).unwrap(),
        "Harvest",
    )
    .unwrap();
    let content = format!("Harvest photo {url}");
    let command =
        Phase1AddCommand::CreatePhotoUpdate(CreatePhotoUpdate::new(&content, vec![image]).unwrap());
    let form = Phase1DraftFormSnapshot {
        command_type: AddCommandType::CreatePhotoUpdate,
        content,
        media: vec![Phase1DraftMediaSnapshot {
            opaque_reference: "media:legacy".into(),
            url,
            sha256: hash.to_hex(),
            media_type: "image/png".into(),
            byte_size: bytes.len() as u64,
            width: 2,
            height: 2,
            alt: "Harvest".into(),
            prepared_at_unix_s: 1_784_347_100,
        }],
        ..update_form()
    };
    let runtime = signing_runtime();
    runtime
        .configure_blossom(
            radroots_sdk::transport::BlossomHostKind::Simulator,
            radroots_sdk::transport::BlossomEndpointAuthority::LoopbackDevelopment,
            origin.clone(),
            vec![],
        )
        .await
        .unwrap();
    runtime
        .phase1_save_draft_with_form(
            [91; 16],
            command,
            1_784_347_200,
            vec![media],
            form,
            None,
            30,
        )
        .await
        .unwrap();
    let (uploading, job) = runtime
        .phase1_prepare_native_upload(
            Phase1UploadIntent::new([91; 16], 1, bytes.clone(), kind.clone(), 2, 2).unwrap(),
        )
        .await
        .unwrap();
    let native = || {
        RecoveryNativeReceipt::new(
            RecoveryNativeIdentity::new(
                [91; 16],
                uploading.draft().revision().get(),
                job.operation_id(),
                job.upload_url().into(),
            )
            .unwrap(),
            RecoveryNativeMedia::new(hash, kind.clone(), bytes.len() as u64).unwrap(),
            SubmissionMediaResponse::new(
                200,
                Some("application/json".into()),
                None,
                serde_json::to_vec(&descriptor).unwrap(),
            )
            .unwrap(),
        )
    };
    let source =
        || RecoveryMedia::new("media:legacy".into(), bytes.clone(), kind.clone(), 2, 2).unwrap();
    let remote = bytes.clone();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = vec![];
        let mut byte = [0];
        while !request.ends_with(b"\r\n\r\n") {
            assert!(request.len() < 16_384);
            stream.read_exact(&mut byte).await.unwrap();
            request.push(byte[0]);
        }
        assert!(request.starts_with(b"GET /"));
        let header = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            remote.len()
        );
        stream.write_all(header.as_bytes()).await.unwrap();
        stream.write_all(&remote).await.unwrap();
    });
    let proof = runtime
        .recover_native_upload(native(), source())
        .await
        .unwrap();
    server.await.unwrap();
    let committed = runtime.phase1_draft_status([91; 16]).await.unwrap();
    runtime
        .configure_blossom(
            radroots_sdk::transport::BlossomHostKind::Simulator,
            radroots_sdk::transport::BlossomEndpointAuthority::LoopbackDevelopment,
            "http://127.0.0.1:3001".into(),
            vec![],
        )
        .await
        .unwrap();
    assert_eq!(
        runtime
            .recover_native_upload(native(), source())
            .await
            .unwrap(),
        proof
    );
    assert_eq!(
        runtime.phase1_draft_status([91; 16]).await.unwrap(),
        committed
    );
    runtime.shutdown().await.unwrap();
}
