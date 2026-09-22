use super::*;
use crate::runtime::backup::{ApplicationBackupManifest, BackupRequest};
use radroots_storage::backup::{
    BackupFormatVersion, BackupId, BackupManifest, BackupMember, BackupMemberKind,
    BackupSecretPolicy, MemberDigest,
};

fn backup() -> BackupRequest {
    let author = radroots_identity::PublicKey::from_hex(
        "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
    )
    .unwrap();
    BackupRequest::new([1; 16], author.into_bytes(), [7; 32], 100, 4096).unwrap()
}

fn manifest(request: &BackupRequest, digest: u8) -> ApplicationBackupManifest {
    let owner = BackupManifest::new(
        BackupFormatVersion::V1,
        BackupId::new(request.id()).unwrap(),
        request.requested_at_ms(),
        BackupSecretPolicy::IncludeProtectedStorage,
        vec![
            BackupMember::new(
                "runtime/events.sqlite3",
                BackupMemberKind::Runtime,
                100,
                MemberDigest::new([digest; 32]),
            )
            .unwrap(),
            BackupMember::new(
                "private/protected.sqlite3",
                BackupMemberKind::Protected,
                100,
                MemberDigest::new([digest; 32]),
            )
            .unwrap(),
        ],
    )
    .unwrap();
    ApplicationBackupManifest::decode(
        &serde_json::to_vec(&serde_json::json!({
            "request": request, "owner": owner, "media": []
        }))
        .unwrap(),
        request,
    )
    .unwrap()
}

#[test]
fn durable_guard_binds_exact_backup_and_manifest_without_delivery_authority() {
    let backup = backup();
    let original = manifest(&backup, 2);
    let request = RestoreRequest::new([3; 16], backup.clone(), 200).unwrap();
    let guard = ApplicationRestoreGuard::new(request.clone(), &original).unwrap();
    assert_eq!(guard.request(), &request);
    assert_eq!(guard.request().attempt_id(), [3; 16]);
    assert_eq!(guard.request().requested_at_ms(), 200);
    assert_eq!(guard.request().backup(), &backup);
    assert_eq!(
        ApplicationRestoreGuard::decode(&guard.encode().unwrap()).unwrap(),
        guard
    );
    guard.verify_manifest(&original).unwrap();
    assert_eq!(
        guard.verify_manifest(&manifest(&backup, 4)),
        Err(RestoreError::VerificationFailed)
    );
    let other = RestoreRequest::new([4; 16], backup.clone(), 200).unwrap();
    assert_ne!(
        ApplicationRestoreGuard::new(other, &original).unwrap(),
        guard
    );
    let foreign_generation = BackupRequest::new(
        backup.id(),
        backup.author(),
        [8; 32],
        backup.requested_at_ms(),
        backup.maximum_bytes(),
    )
    .unwrap();
    assert_eq!(
        guard.verify_manifest(&manifest(&foreign_generation, 2)),
        Err(RestoreError::GenerationMismatch)
    );
    assert!(!format!("{guard:?} {request:?}").contains(&hex::encode(backup.author())));
}

#[test]
fn invalid_future_and_noncanonical_guards_cannot_enter_recovery() {
    let backup = backup();
    for (id, at) in [([0; 16], 200), ([3; 16], 99), ([3; 16], u64::MAX)] {
        assert_eq!(
            RestoreRequest::new(id, backup.clone(), at),
            Err(RestoreError::InvalidRequest)
        );
    }
    let guard = ApplicationRestoreGuard::new(
        RestoreRequest::new([3; 16], backup.clone(), 200).unwrap(),
        &manifest(&backup, 2),
    )
    .unwrap();
    let mut bytes = guard.encode().unwrap();
    bytes.push(b' ');
    assert_eq!(
        ApplicationRestoreGuard::decode(&bytes),
        Err(RestoreError::VerificationFailed)
    );
    let mut future = serde_json::to_value(&guard).unwrap();
    future["request"]["version"] = serde_json::json!(2);
    assert_eq!(
        ApplicationRestoreGuard::decode(&serde_json::to_vec(&future).unwrap()),
        Err(RestoreError::UnsupportedFormat)
    );
    assert_eq!(
        ApplicationRestoreGuard::decode(&vec![b' '; RESTORE_GUARD_MAX_BYTES + 1]),
        Err(RestoreError::CapacityExceeded)
    );
    for bytes in [b"{}".as_slice(), b"null", b"/private/arbitrary/database"] {
        assert_eq!(
            ApplicationRestoreGuard::decode(bytes),
            Err(RestoreError::VerificationFailed)
        );
    }
}
