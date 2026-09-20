use super::*;

fn association() -> RecoveryAssociation {
    RecoveryAssociation::new(
        PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
            .unwrap(),
        AuthoredDraftId::new([1; 16]).unwrap(),
        SigningOperationId::new([2; 16]).unwrap(),
        BlobUrl::parse(&format!("https://media.example/{}", "03".repeat(32))).unwrap(),
        MediaType::parse("image/png").unwrap(),
        4096,
    )
    .unwrap()
}

fn parent(completion: RustCompletion) -> RecoveryParent {
    RecoveryParent::Known {
        association: Box::new(association()),
        completion,
    }
}

fn native(state: NativeRecoveryState) -> NativeRecoveryEvidence {
    NativeRecoveryEvidence::Known {
        association: Box::new(association()),
        state,
    }
}

#[test]
fn structurally_valid_text_is_bounded_before_retention_and_comparison() {
    let prefix = format!("https://media.example/{}.", "03".repeat(32));
    let url =
        |length| BlobUrl::parse(&format!("{prefix}{}", "a".repeat(length - prefix.len()))).unwrap();
    let media_type = |length| {
        let prefix = "image/png; x=";
        let value =
            MediaType::parse(&format!("{prefix}{}", "a".repeat(length - prefix.len()))).unwrap();
        assert_eq!(value.as_str().len(), length);
        value
    };
    let create = |url, media_type| {
        let a = association();
        RecoveryAssociation::new(a.author, a.parent, a.attempt, url, media_type, a.byte_size)
    };
    assert!(
        create(
            url(RECOVERY_URL_MAX_BYTES),
            media_type(RECOVERY_MEDIA_TYPE_MAX_BYTES)
        )
        .is_ok()
    );
    assert!(
        create(
            url(RECOVERY_URL_MAX_BYTES + 1),
            media_type(RECOVERY_MEDIA_TYPE_MAX_BYTES)
        )
        .is_err()
    );
    assert!(
        create(
            url(RECOVERY_URL_MAX_BYTES),
            media_type(RECOVERY_MEDIA_TYPE_MAX_BYTES + 1)
        )
        .is_err()
    );
}

#[test]
fn both_commit_windows_and_every_native_state_have_explicit_repeatable_actions() {
    use NativeRecoveryState as N;
    use RecoveryDecision as D;
    use RustCompletion as R;
    let cases = [
        (R::Pending, N::ReceiptAvailable, D::Complete),
        (R::Verified, N::ReceiptAvailable, D::Settle),
        (R::Verified, N::Settled, D::Reconciled),
        (
            R::Pending,
            N::Settled,
            D::Quarantine(RecoveryRepair::SettlementWithoutCompletion),
        ),
        (R::Pending, N::Active, D::Pause(RecoveryPause::NativeActive)),
        (R::Verified, N::Active, D::QueryNative),
        (
            R::Pending,
            N::DefinitivelyInactive,
            D::Pause(RecoveryPause::RetryAuthorityRequired),
        ),
        (R::Verified, N::DefinitivelyInactive, D::QueryNative),
    ];
    for (rust, state, expected) in cases {
        let parent = parent(rust);
        let native = native(state);
        let original = (parent.clone(), native.clone());
        for _ in 0..3 {
            assert_eq!(reconcile(&parent, &native), expected);
            assert_eq!((&parent, &native), (&original.0, &original.1));
        }
    }
}

#[test]
fn page_absence_requires_exact_lookup_and_unavailable_storage_is_never_corruption() {
    use RecoveryDecision as D;
    let cases = [
        (RecoveryParent::Unqueried, D::LookupParent),
        (
            RecoveryParent::Missing,
            D::Quarantine(RecoveryRepair::ParentMissing),
        ),
        (
            RecoveryParent::InvalidRecord,
            D::Quarantine(RecoveryRepair::InvalidParent),
        ),
        (
            RecoveryParent::AssociationUnconfirmed,
            D::Quarantine(RecoveryRepair::AssociationUnconfirmed),
        ),
        (
            RecoveryParent::ProtectedDataUnavailable,
            D::Pause(RecoveryPause::ProtectedData),
        ),
        (
            RecoveryParent::StorageUnavailable,
            D::Pause(RecoveryPause::StorageUnavailable),
        ),
    ];
    for (parent, expected) in cases {
        for native in [
            NativeRecoveryEvidence::Unknown,
            NativeRecoveryEvidence::Conflicting,
            native(NativeRecoveryState::ReceiptAvailable),
            native(NativeRecoveryState::Settled),
        ] {
            assert_eq!(reconcile(&parent, &native), expected);
        }
    }
    for completion in [RustCompletion::Pending, RustCompletion::Verified] {
        assert_eq!(
            reconcile(&parent(completion), &NativeRecoveryEvidence::Unknown),
            D::QueryNative
        );
        assert_eq!(
            reconcile(&parent(completion), &NativeRecoveryEvidence::Conflicting),
            D::Quarantine(RecoveryRepair::ConflictingEvidence)
        );
    }
}

#[test]
fn every_frozen_identity_component_must_match_before_complete_or_settle() {
    let original = association();
    let mut mismatches = Vec::new();
    let mut value = original.clone();
    value.author =
        PublicKey::from_hex("c6047f9441ed7d6d3045406e95c07cd85c778e4b8cef3ca7abac09b95c709ee5")
            .unwrap();
    mismatches.push(value);
    let mut value = original.clone();
    value.parent = AuthoredDraftId::new([4; 16]).unwrap();
    mismatches.push(value);
    let mut value = original.clone();
    value.attempt = SigningOperationId::new([5; 16]).unwrap();
    mismatches.push(value);
    let mut value = original.clone();
    value.canonical_url =
        BlobUrl::parse(&format!("https://media.example/{}", "06".repeat(32))).unwrap();
    mismatches.push(value);
    let mut value = original.clone();
    value.canonical_url =
        BlobUrl::parse(&format!("https://other.example/{}", "03".repeat(32))).unwrap();
    mismatches.push(value);
    let mut value = original.clone();
    value.media_type = MediaType::parse("image/jpeg").unwrap();
    mismatches.push(value);
    let mut value = original.clone();
    value.byte_size += 1;
    mismatches.push(value);
    for association in mismatches {
        for completion in [RustCompletion::Pending, RustCompletion::Verified] {
            for state in [
                NativeRecoveryState::ReceiptAvailable,
                NativeRecoveryState::Settled,
            ] {
                let native = NativeRecoveryEvidence::Known {
                    association: Box::new(association.clone()),
                    state,
                };
                assert_eq!(
                    reconcile(&parent(completion), &native),
                    RecoveryDecision::Quarantine(RecoveryRepair::AssociationMismatch)
                );
            }
        }
    }
    assert!(
        RecoveryAssociation::new(
            original.author,
            original.parent,
            original.attempt,
            original.canonical_url,
            original.media_type,
            0
        )
        .is_err()
    );
}
