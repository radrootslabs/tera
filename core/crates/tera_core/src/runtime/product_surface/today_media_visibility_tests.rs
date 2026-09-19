use super::super::tests::{context, ingest, keys, signed, visible_admission};
use super::*;
use crate::runtime::{
    builder::RuntimeBuilder,
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};
use radroots_blossom::Sha256 as BlobHash;
use radroots_sdk::transport::{
    BlossomConfig, BlossomEndpointAuthority, BlossomHostKind, BlossomProfile,
};
use radroots_transport::source::ObservedEvent;
use std::{sync::Arc, time::Duration};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

const NOW: u64 = 2_000_000_000;

struct Fixture {
    _root: tempfile::TempDir,
    runtime: Arc<TeraRuntime>,
    selected: LocalNetwork,
    fingerprint: [u8; 32],
    source: String,
    bytes: Vec<u8>,
    listener: TcpListener,
}

async fn fixture() -> Fixture {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend_from_slice(&2_u32.to_be_bytes());
    bytes.extend_from_slice(&3_u32.to_be_bytes());
    let url = format!("{origin}/{}.png", BlobHash::digest(&bytes).to_hex());
    let root = tempfile::tempdir().unwrap();
    let public_key = keys().public_key().to_string();
    let store = MobileUserStoreConfig::from_encoded(
        root.path(),
        &public_key,
        &"86".repeat(32),
        NOW * 1_000,
        ProtectedDataAvailability::Available,
    )
    .unwrap();
    std::fs::create_dir_all(store.owner_directory()).unwrap();
    let blossom = BlossomConfig::from_profile(
        BlossomProfile::new(
            BlossomHostKind::Simulator,
            BlossomEndpointAuthority::LoopbackDevelopment,
            &origin,
            std::iter::empty::<&str>(),
        )
        .unwrap(),
    )
    .with_network_policy(
        Duration::from_millis(250),
        Duration::from_secs(3),
        1,
        Duration::from_millis(1),
    )
    .unwrap();
    let runtime = Arc::new(
        RuntimeBuilder::new(store)
            .blossom_config(blossom)
            .build()
            .await
            .unwrap(),
    );
    let selected = context(None, 1);
    let profile = signed(
        0,
        vec![],
        &format!(r#"{{"name":"Media fixture","picture":"{url}"}}"#),
        NOW,
    );
    let source = profile.id().to_hex();
    ingest(&runtime, &selected, profile, NOW + 1).await;
    let picture = runtime
        .phase1_me(&selected, &public_key, NOW + 2, "UTC")
        .await
        .unwrap()
        .profile
        .unwrap()
        .picture
        .unwrap();
    Fixture {
        _root: root,
        runtime,
        selected,
        fingerprint: *picture.structural().fingerprint(),
        source,
        bytes,
        listener,
    }
}

async fn retrieve(
    runtime: &TeraRuntime,
    selected: &LocalNetwork,
    fingerprint: [u8; 32],
) -> Result<Phase1LocalMediaArtifact, TodayError> {
    runtime
        .phase1_retrieve_media(
            selected,
            fingerprint,
            [86; 16],
            Phase1MediaCachePolicy::new(1_024, 8).unwrap(),
            BlossomCancellation::default(),
        )
        .await
}

async fn serve(
    listener: TcpListener,
    bytes: Vec<u8>,
    started: oneshot::Sender<()>,
    resume: oneshot::Receiver<()>,
) {
    let (mut stream, _) = listener.accept().await.unwrap();
    let mut request = Vec::new();
    while !request.windows(4).any(|part| part == b"\r\n\r\n") {
        let mut chunk = [0; 1_024];
        let count = stream.read(&mut chunk).await.unwrap();
        assert_ne!(count, 0);
        request.extend_from_slice(&chunk[..count]);
        assert!(request.len() <= 16_384);
    }
    let request = String::from_utf8(request).unwrap();
    assert!(request.starts_with("GET /"));
    assert!(!request.to_lowercase().contains("authorization:"));
    started.send(()).unwrap();
    resume.await.unwrap();
    let header = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: image/png\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        bytes.len()
    );
    stream.write_all(header.as_bytes()).await.unwrap();
    stream.write_all(&bytes).await.unwrap();
    stream.shutdown().await.unwrap();
}

#[tokio::test]
async fn wrong_query_scope_and_unadmitted_reference_cannot_start_transport() {
    let f = fixture().await;
    let wrong = context(Some("another locality"), 1);
    for (selected, fingerprint) in [(&wrong, f.fingerprint), (&f.selected, [1; 32])] {
        assert!(matches!(
            retrieve(&f.runtime, selected, fingerprint).await,
            Err(TodayError::InvalidRequest)
        ));
    }
    assert!(
        tokio::time::timeout(Duration::from_millis(30), f.listener.accept())
            .await
            .is_err()
    );
    let (started, entered) = oneshot::channel();
    let (resume, wait) = oneshot::channel();
    let server = tokio::spawn(serve(f.listener, f.bytes.clone(), started, wait));
    resume.send(()).unwrap();
    let artifact = retrieve(&f.runtime, &f.selected, f.fingerprint)
        .await
        .unwrap();
    entered.await.unwrap();
    server.await.unwrap();
    assert_eq!(artifact.bytes(), f.bytes);
    assert!(matches!(
        f.runtime
            .phase1_verified_media_artifact(&wrong, artifact.artifact_id(), (NOW + 3) * 1_000)
            .await,
        Err(TodayError::InvalidRequest)
    ));
    let deletion = signed(5, vec![vec!["e", &f.source]], "", NOW + 4);
    EventStore::admit(
        f.runtime.client.storage().unwrap(),
        visible_admission(deletion, (NOW + 4) * 1_000),
    )
    .await
    .unwrap();
    assert!(matches!(
        f.runtime
            .phase1_verified_media_artifact(&f.selected, artifact.artifact_id(), (NOW + 5) * 1_000)
            .await,
        Err(TodayError::InvalidRequest)
    ));
    assert!(
        artifact.local_path().exists(),
        "visibility revocation does not authorize shared blob deletion"
    );
    f.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn same_raw_count_visibility_change_revokes_cached_retrieval_authority() {
    let f = fixture().await;
    let deletion = signed(5, vec![vec!["e", &f.source]], "", NOW + 4);
    let admission = visible_admission(deletion, (NOW + 4) * 1_000);
    let storage = f.runtime.client.storage().unwrap();
    EventStore::admit(
        storage,
        EventAdmission::raw(ObservedEvent::new(
            admission.event().clone(),
            admission.provenance().clone(),
        )),
    )
    .await
    .unwrap();
    f.runtime
        .phase1_refresh_today(&f.selected, NOW + 5, TodayProjectionUpdate::Incremental)
        .await
        .unwrap();
    let before = EventStore::status(storage).await.unwrap().raw_events();
    EventStore::admit(storage, admission).await.unwrap();
    assert_eq!(
        EventStore::status(storage).await.unwrap().raw_events(),
        before
    );
    assert!(matches!(
        retrieve(&f.runtime, &f.selected, f.fingerprint).await,
        Err(TodayError::InvalidRequest)
    ));
    assert!(
        tokio::time::timeout(Duration::from_millis(30), f.listener.accept())
            .await
            .is_err()
    );
    f.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn retraction_during_network_wait_cannot_publish_verified_bytes() {
    let f = fixture().await;
    let (started, entered) = oneshot::channel();
    let (resume, wait) = oneshot::channel();
    let server = tokio::spawn(serve(f.listener, f.bytes, started, wait));
    let runtime = f.runtime.clone();
    let selected = f.selected.clone();
    let fingerprint = f.fingerprint;
    let pending = tokio::spawn(async move { retrieve(&runtime, &selected, fingerprint).await });
    tokio::time::timeout(Duration::from_secs(2), entered)
        .await
        .unwrap()
        .unwrap();
    let deletion = signed(5, vec![vec!["e", &f.source]], "", NOW + 4);
    ingest(&f.runtime, &f.selected, deletion, NOW + 5).await;
    resume.send(()).unwrap();
    assert!(matches!(
        pending.await.unwrap(),
        Err(TodayError::InvalidRequest)
    ));
    server.await.unwrap();
    assert_eq!(
        f.runtime
            .phase1_media_cache_status(&f.selected)
            .await
            .unwrap()
            .artifacts,
        0
    );
    f.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn arbitrary_embedded_url_cannot_bypass_the_verified_transport() {
    let f = fixture().await;
    let url = format!("http://{}/arbitrary", f.listener.local_addr().unwrap());
    ingest(
        &f.runtime,
        &f.selected,
        signed(0, vec![], &format!(r#"{{"picture":"{url}"}}"#), NOW + 10),
        NOW + 11,
    )
    .await;
    let picture = f
        .runtime
        .phase1_me(
            &f.selected,
            &keys().public_key().to_string(),
            NOW + 12,
            "UTC",
        )
        .await
        .unwrap()
        .profile
        .unwrap()
        .picture
        .unwrap();
    assert!(
        retrieve(&f.runtime, &f.selected, *picture.structural().fingerprint())
            .await
            .is_err()
    );
    assert!(
        tokio::time::timeout(Duration::from_millis(30), f.listener.accept())
            .await
            .is_err()
    );
    assert_eq!(
        f.runtime
            .phase1_media_cache_status(&f.selected)
            .await
            .unwrap()
            .artifacts,
        0
    );
    f.runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn visible_reference_does_not_turn_wrong_response_bytes_into_an_artifact() {
    let f = fixture().await;
    let mut wrong = f.bytes;
    wrong[23] ^= 1;
    let (started, entered) = oneshot::channel();
    let (resume, wait) = oneshot::channel();
    let server = tokio::spawn(serve(f.listener, wrong, started, wait));
    resume.send(()).unwrap();
    assert!(
        retrieve(&f.runtime, &f.selected, f.fingerprint)
            .await
            .is_err()
    );
    entered.await.unwrap();
    server.await.unwrap();
    assert_eq!(
        f.runtime
            .phase1_media_cache_status(&f.selected)
            .await
            .unwrap()
            .artifacts,
        0
    );
    f.runtime.shutdown().await.unwrap();
}
