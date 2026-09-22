use super::*;
use crate::runtime::product_surface::{
    AddCommandType, ComposerEditSequence, ComposerFormInput, ComposerId, ComposerPartialForm,
    ComposerScope, LocalNetworkId,
};
use crate::runtime::{
    builder::RuntimeBuilder,
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};
use radroots_storage::authored_draft::{AuthoredDraft, AuthoredDraftId, AuthoredDraftStage};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

const AUTHOR: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

#[derive(Default)]
pub(super) struct Host {
    candidate: Mutex<Option<Vec<u8>>>,
    completed: Mutex<Option<Vec<u8>>>,
    fail_publication: AtomicBool,
}

impl BackupHost for Host {
    fn load_candidate(
        &self,
        _: BackupRequest,
    ) -> BoxFuture<'_, Result<Option<Vec<u8>>, BackupError>> {
        Box::pin(async { Ok(self.candidate.lock().unwrap().clone()) })
    }
    fn retain_media(
        &self,
        _: BackupRequest,
        media: Vec<BackupMediaRequirement>,
    ) -> BoxFuture<'_, Result<Vec<BackupMediaLease>, BackupError>> {
        Box::pin(async move {
            if media.is_empty() {
                Ok(vec![])
            } else {
                Err(BackupError::MediaUnavailable)
            }
        })
    }
    fn persist_candidate(
        &self,
        manifest: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), BackupError>> {
        Box::pin(async move {
            *self.candidate.lock().unwrap() = Some(manifest.encode()?);
            Ok(())
        })
    }
    fn publish_complete(
        &self,
        manifest: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), BackupError>> {
        Box::pin(async move {
            if self.fail_publication.load(Ordering::SeqCst) {
                return Err(BackupError::PublicationIncomplete);
            }
            *self.completed.lock().unwrap() = Some(manifest.encode()?);
            Ok(())
        })
    }
}

pub(super) async fn fixture() -> (tempfile::TempDir, Arc<TeraRuntime>, BackupRequest) {
    let root = tempfile::tempdir().unwrap();
    let config = MobileUserStoreConfig::from_encoded(
        root.path(),
        AUTHOR,
        &hex::encode([7; 32]),
        100,
        ProtectedDataAvailability::Available,
    )
    .unwrap()
    .with_local_backups();
    std::fs::create_dir_all(config.owner_directory()).unwrap();
    std::fs::create_dir_all(config.backup_directory()).unwrap();
    let author = config.public_key().into_bytes();
    let runtime = RuntimeBuilder::new(config).build().await.unwrap();
    let request = BackupRequest::new([1; 16], author, [7; 32], 200, 128 * 1024 * 1024).unwrap();
    (root, Arc::new(runtime), request)
}

#[tokio::test]
async fn owner_wal_capture_and_lost_completion_reconcile_the_original_bundle() {
    let (root, runtime, request) = fixture().await;
    let id = ComposerId::new([3; 16]).unwrap();
    let scope = ComposerScope::new(
        radroots_identity::PublicKey::from_hex(AUTHOR).unwrap(),
        LocalNetworkId::new("backup_fixture".into()).unwrap(),
    );
    let mut input = ComposerFormInput::empty(AddCommandType::CreateFoodAvailability);
    input.content = "latest committed WAL content".into();
    runtime
        .composer_create(
            &scope,
            id,
            ComposerEditSequence::INITIAL,
            ComposerPartialForm::new(input).unwrap(),
        )
        .await
        .unwrap();
    let draft_id = AuthoredDraftId::new(*id.as_bytes()).unwrap();
    let expected = runtime
        .client
        .storage()
        .unwrap()
        .authored_draft_head(draft_id)
        .await
        .unwrap()
        .unwrap();
    assert!(
        std::fs::metadata(
            root.path()
                .join("radroots/users")
                .join(AUTHOR)
                .join("runtime.sqlite-wal")
        )
        .unwrap()
        .len()
            > 0
    );
    let host = Host::default();
    host.fail_publication.store(true, Ordering::SeqCst);
    assert_eq!(
        runtime
            .capture_application_backup(request.clone(), &host)
            .await
            .unwrap_err(),
        BackupError::PublicationIncomplete
    );
    assert!(host.completed.lock().unwrap().is_none());
    let first = host.candidate.lock().unwrap().clone().unwrap();
    host.fail_publication.store(false, Ordering::SeqCst);
    let manifest = runtime
        .capture_application_backup(request.clone(), &host)
        .await
        .unwrap();
    assert_eq!(manifest.encode().unwrap(), first);
    assert_eq!(host.completed.lock().unwrap().as_ref(), Some(&first));
    assert_eq!(manifest.owner().members().len(), 2);
    assert!(
        manifest
            .owner()
            .members()
            .iter()
            .all(|v| v.byte_length() > 0)
    );
    let replay = runtime
        .capture_application_backup(request, &host)
        .await
        .unwrap();
    assert_eq!(manifest, replay);
    // Independently verify physical members against the owner receipt. Opening
    // a VACUUM snapshot as a live store would impose the live WAL policy. The
    // owning Lib fixture separately verifies the captured WAL row via SQLx.
    let backup_root = root.path().join("backups").join(AUTHOR).join("sqlite");
    let bundles: Vec<_> = std::fs::read_dir(backup_root)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    assert_eq!(bundles.len(), 1);
    use sha2::{Digest, Sha256};
    for member in manifest.owner().members() {
        let bytes = std::fs::read(bundles[0].join(member.relative_path())).unwrap();
        assert_eq!(bytes.len() as u64, member.byte_length());
        assert_eq!(Sha256::digest(bytes).as_slice(), member.sha256().as_bytes());
    }
    assert_eq!(
        runtime
            .client
            .storage()
            .unwrap()
            .authored_draft_head(draft_id)
            .await
            .unwrap(),
        Some(expected)
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn identity_generation_capacity_and_unknown_inventory_refuse_without_completion() {
    let (_root, runtime, request) = fixture().await;
    let host = Host::default();
    let foreign = BackupRequest::new(
        request.id(),
        radroots_identity::PublicKey::from_hex(
            "c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5",
        )
        .unwrap()
        .into_bytes(),
        request.generation(),
        request.requested_at_ms(),
        request.maximum_bytes(),
    )
    .unwrap();
    assert_eq!(
        runtime
            .capture_application_backup(foreign, &host)
            .await
            .unwrap_err(),
        BackupError::IdentityMismatch
    );
    let wrong_generation = BackupRequest::new(
        request.id(),
        request.author(),
        [8; 32],
        200,
        request.maximum_bytes(),
    )
    .unwrap();
    assert_eq!(
        runtime
            .capture_application_backup(wrong_generation, &host)
            .await
            .unwrap_err(),
        BackupError::GenerationMismatch
    );
    let tiny = BackupRequest::new([2; 16], request.author(), request.generation(), 200, 1).unwrap();
    assert_eq!(
        runtime
            .capture_application_backup(tiny, &host)
            .await
            .unwrap_err(),
        BackupError::CapacityExceeded
    );
    let unknown = AuthoredDraft::initial(
        AuthoredDraftId::new([9; 16]).unwrap(),
        request.author(),
        "fixture.unknown.v1",
        b"retained".to_vec(),
        AuthoredDraftStage::Draft,
        None,
        100,
    )
    .unwrap();
    runtime
        .client
        .storage()
        .unwrap()
        .append_authored_draft(unknown.clone(), None)
        .await
        .unwrap();
    assert_eq!(
        runtime
            .capture_application_backup(request, &host)
            .await
            .unwrap_err(),
        BackupError::MediaUnavailable
    );
    assert!(host.candidate.lock().unwrap().is_none());
    assert!(host.completed.lock().unwrap().is_none());
    assert_eq!(
        runtime
            .client
            .storage()
            .unwrap()
            .authored_draft_head(unknown.draft_id())
            .await
            .unwrap(),
        Some(unknown)
    );
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn maintenance_refuses_active_work_and_drains_close_without_leaking_admission() {
    let (_root, runtime, request) = fixture().await;
    let host = Host::default();
    let command = runtime.lifecycle.enter().unwrap();
    assert_eq!(
        runtime
            .capture_application_backup(request.clone(), &host)
            .await
            .unwrap_err(),
        BackupError::Busy
    );
    assert!(host.candidate.lock().unwrap().is_none());
    drop(command);
    let maintenance = runtime.lifecycle.maintenance().unwrap();
    assert!(runtime.sdk_storage_status().await.is_err());
    let closing = runtime.shutdown();
    futures_util::pin_mut!(closing);
    assert!(futures_util::poll!(&mut closing).is_pending());
    drop(maintenance);
    closing.await.unwrap();
    assert_eq!(
        runtime
            .capture_application_backup(request, &host)
            .await
            .unwrap_err(),
        BackupError::Unavailable
    );
}
