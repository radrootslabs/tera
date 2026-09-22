use super::tests::fixture;
use super::*;
use crate::runtime::product_surface::{
    AddCommandType, ComposerEditSequence, ComposerFormInput, ComposerId, ComposerMediaInput,
    ComposerPartialForm, ComposerScope, LocalNetworkId,
};
use std::sync::{
    Mutex,
    atomic::{AtomicU8, Ordering},
};

#[derive(Default)]
struct FaultHost {
    candidate: Mutex<Option<Vec<u8>>>,
    // 1: missing, 2: wrong size, 3: wrong digest, 4: oversized, 5: suspend after candidate.
    fault: AtomicU8,
}

impl BackupHost for FaultHost {
    fn load_candidate(
        &self,
        _: BackupRequest,
    ) -> BoxFuture<'_, Result<Option<Vec<u8>>, BackupError>> {
        Box::pin(async { Ok(self.candidate.lock().unwrap().clone()) })
    }
    fn retain_media(
        &self,
        request: BackupRequest,
        media: Vec<BackupMediaRequirement>,
    ) -> BoxFuture<'_, Result<Vec<BackupMediaLease>, BackupError>> {
        Box::pin(async move {
            let fault = self.fault.load(Ordering::SeqCst);
            if fault == 1 {
                return Err(BackupError::MediaUnavailable);
            }
            Ok(media
                .into_iter()
                .map(|media| {
                    let hash = if fault == 3 {
                        "b".repeat(64)
                    } else {
                        media.sha256
                    };
                    BackupMediaLease {
                        identifier: request.lease_identifier(&hash),
                        sha256: hash,
                        byte_length: match fault {
                            2 => media.byte_length + 1,
                            4 => BACKUP_MEDIA_MAX_BYTES + 1,
                            _ => media.byte_length,
                        },
                    }
                })
                .collect())
        })
    }
    fn persist_candidate(
        &self,
        manifest: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), BackupError>> {
        Box::pin(async move {
            *self.candidate.lock().unwrap() = Some(manifest.encode()?);
            if self.fault.load(Ordering::SeqCst) == 5 {
                std::future::pending::<()>().await;
            }
            Ok(())
        })
    }
    fn publish_complete(
        &self,
        _: ApplicationBackupManifest,
    ) -> BoxFuture<'_, Result<(), BackupError>> {
        Box::pin(async { Ok(()) })
    }
}

async fn create(
    runtime: &crate::TeraRuntime,
    request: &BackupRequest,
    id: u8,
    media: bool,
) -> Result<(), crate::runtime::product_surface::ComposerPersistenceError> {
    let scope = ComposerScope::new(
        radroots_identity::PublicKey::from_hex(&hex::encode(request.author())).unwrap(),
        LocalNetworkId::new("backup_fixture".into()).unwrap(),
    );
    let mut input = ComposerFormInput::empty(AddCommandType::CreateFoodAvailability);
    if media {
        input.media.push(ComposerMediaInput {
            opaque_reference: "media:backup_fixture".into(),
            sha256: "a".repeat(64),
            media_type: "image/png".into(),
            byte_size: 512,
            width: 4,
            height: 4,
            alt: String::new(),
            prepared_at_unix_s: 1_800_000_000,
        });
    }
    runtime
        .composer_create(
            &scope,
            ComposerId::new([id; 16]).unwrap(),
            ComposerEditSequence::INITIAL,
            ComposerPartialForm::new(input).unwrap(),
        )
        .await
        .map(|_| ())
}

#[tokio::test]
async fn missing_wrong_size_wrong_hash_and_oversize_media_cannot_create_candidate() {
    let (_root, runtime, request) = fixture().await;
    create(&runtime, &request, 3, true).await.unwrap();
    let host = FaultHost::default();
    for fault in 1..=4 {
        host.fault.store(fault, Ordering::SeqCst);
        let expected = if fault == 4 {
            BackupError::CapacityExceeded
        } else {
            BackupError::MediaUnavailable
        };
        assert_eq!(
            runtime
                .capture_application_backup(request.clone(), &host)
                .await
                .unwrap_err(),
            expected
        );
        assert!(host.candidate.lock().unwrap().is_none());
    }
    host.fault.store(0, Ordering::SeqCst);
    let manifest = runtime
        .capture_application_backup(request, &host)
        .await
        .unwrap();
    assert_eq!(manifest.media().len(), 1);
    assert_eq!(manifest.media()[0].byte_length, 512);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn mutation_is_fenced_and_cancelled_candidate_reconciles_without_recapture() {
    let (_root, runtime, request) = fixture().await;
    create(&runtime, &request, 3, false).await.unwrap();
    let host = FaultHost::default();
    host.fault.store(5, Ordering::SeqCst);
    let mut capture = Box::pin(runtime.capture_application_backup(request.clone(), &host));
    loop {
        assert!(futures_util::poll!(&mut capture).is_pending());
        if host.candidate.lock().unwrap().is_some() {
            break;
        }
        tokio::task::yield_now().await;
    }
    assert!(create(&runtime, &request, 4, false).await.is_err());
    let candidate = host.candidate.lock().unwrap().clone().unwrap();
    drop(capture);
    create(&runtime, &request, 4, false).await.unwrap();
    host.fault.store(0, Ordering::SeqCst);
    let completed = runtime
        .capture_application_backup(request, &host)
        .await
        .unwrap();
    // A later live mutation cannot silently change an already bound candidate.
    assert_eq!(completed.encode().unwrap(), candidate);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn tampered_database_or_malformed_candidate_never_reports_success() {
    let (root, runtime, request) = fixture().await;
    let host = FaultHost::default();
    let manifest = runtime
        .capture_application_backup(request.clone(), &host)
        .await
        .unwrap();
    let original = manifest.encode().unwrap();
    for bytes in [b"{}".to_vec(), vec![0; BACKUP_MANIFEST_MAX_BYTES + 1]] {
        *host.candidate.lock().unwrap() = Some(bytes);
        assert!(
            runtime
                .capture_application_backup(request.clone(), &host)
                .await
                .is_err()
        );
    }
    for (key, value, expected) in [
        (
            "version",
            serde_json::json!(2),
            BackupError::UnsupportedFormat,
        ),
        (
            "generation",
            serde_json::json!(vec![8; 32]),
            BackupError::GenerationMismatch,
        ),
        (
            "maximum_bytes",
            serde_json::json!(request.maximum_bytes() + 1),
            BackupError::Conflict,
        ),
    ] {
        let mut value_json: serde_json::Value = serde_json::from_slice(&original).unwrap();
        value_json["request"][key] = value;
        *host.candidate.lock().unwrap() = Some(serde_json::to_vec(&value_json).unwrap());
        assert_eq!(
            runtime
                .capture_application_backup(request.clone(), &host)
                .await
                .unwrap_err(),
            expected
        );
    }
    *host.candidate.lock().unwrap() = Some(original);
    let bundle = std::fs::read_dir(
        root.path()
            .join("backups")
            .join(hex::encode(request.author()))
            .join("sqlite"),
    )
    .unwrap()
    .next()
    .unwrap()
    .unwrap()
    .path();
    let member = &manifest.owner().members()[0];
    let path = bundle.join(member.relative_path());
    let mut bytes = std::fs::read(&path).unwrap();
    bytes[100] ^= 1;
    std::fs::write(&path, &bytes).unwrap();
    assert_eq!(
        runtime
            .capture_application_backup(request, &host)
            .await
            .unwrap_err(),
        BackupError::VerificationFailed
    );
    assert_eq!(std::fs::read(path).unwrap(), bytes);
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn unsupported_owner_never_fabricates_an_empty_backup() {
    let mut runtime = crate::TeraRuntime::test_memory().unwrap();
    let (_root, owner, request) = fixture().await;
    // Admit the account so the assertion reaches the actual unsupported owner,
    // rather than succeeding through an earlier identity refusal.
    runtime.store_public_key =
        Some(radroots_identity::PublicKey::from_hex(&hex::encode(request.author())).unwrap());
    let host = FaultHost::default();
    assert_eq!(
        runtime
            .capture_application_backup(request, &host)
            .await
            .unwrap_err(),
        BackupError::Unavailable
    );
    assert!(host.candidate.lock().unwrap().is_none());
    runtime.shutdown().await.unwrap();
    owner.shutdown().await.unwrap();
}
