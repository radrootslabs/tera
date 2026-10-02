use super::*;
use crate::runtime::product_surface::submission::{
    fault_store::{Fault, FaultStore},
    operation_test_support::{CountingSigner, runtime},
    test_support::*,
    transaction_test_support::capture_with_policy,
};
use std::sync::atomic::Ordering;

use super::tests::assert_records;

// Expected states are selected before each effect. These faults intercept the
// real storage trait only in cfg(test); the SQLite owner performs the commit.
#[tokio::test]
async fn sqlite_commit_faults_reopen_exact_intent_without_false_acknowledgement() {
    for media in [false, true] {
        for fault in [
            Fault::BeforeCommit,
            Fault::LostCallback,
            Fault::WrongReceipt,
            Fault::CapacityBefore,
            Fault::CapacityAfter,
        ] {
            let installed = !matches!(fault, Fault::BeforeCommit | Fault::CapacityBefore);
            let expected = match &fault {
                Fault::WrongReceipt => E::InvalidReceipt,
                Fault::CapacityBefore | Fault::CapacityAfter => {
                    E::Storage(Error::SpaceInsufficient)
                }
                _ => E::Storage(Error::BackendUnavailable),
            };
            let root = tempfile::tempdir().unwrap();
            let signer = CountingSigner::new();
            let owner = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
            let request = request();
            let captured = capture_with_policy(
                owner.client.storage().unwrap(),
                &request,
                media,
                owner.active_queue_policy(NOW).unwrap(),
            )
            .await;
            {
                let inner = owner.client.storage().unwrap();
                let mut store = FaultStore::new(inner, Fault::None);
                store.atomic_fault = fault;
                let repository = SubmissionRepository { store: &store };
                assert_eq!(repository.commit(&captured).await.unwrap_err(), expected);
                assert_eq!(store.commits.load(Ordering::SeqCst), 1);
                assert_records(inner, &captured, installed).await;
                assert_eq!(
                    repository.recover(&request).await.unwrap().is_some(),
                    installed
                );
            }
            owner.shutdown().await.unwrap();
            drop(owner);
            let reopened = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
            {
                let inner = reopened.client.storage().unwrap();
                let repository = SubmissionRepository { store: inner };
                assert_records(inner, &captured, installed).await;
                let recovered = repository.recover(&request).await.unwrap();
                assert_eq!(recovered.is_some(), installed);
                if let Some(recovered) = recovered {
                    assert_eq!(
                        recovered.operation_id(),
                        intent::operation_id(&request).unwrap()
                    );
                    assert_eq!(recovered.intent_id(), intent::intent_id(&request).unwrap());
                    assert!(recovered.is_replay());
                }
                let retry = repository.commit(&captured).await.unwrap();
                assert_eq!(retry.is_replay(), installed);
                assert_eq!(
                    retry.operation_id(),
                    intent::operation_id(&request).unwrap()
                );
                assert_eq!(retry.intent_id(), intent::intent_id(&request).unwrap());
                assert_records(inner, &captured, true).await;
                let replay = repository.commit(&captured).await.unwrap();
                assert!(replay.is_replay());
                assert_eq!(retry.committed_at_unix_ms(), replay.committed_at_unix_ms());
                assert_eq!(retry.operation_id(), replay.operation_id());
                assert_records(inner, &captured, true).await;
            }
            assert_eq!(signer.count(), 0);
            assert_eq!(signer.statuses.load(Ordering::SeqCst), 0);
            reopened.shutdown().await.unwrap();
        }
    }
}

#[tokio::test]
async fn sqlite_racing_commits_reopen_one_frozen_operation_without_publication() {
    for media in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let signer = CountingSigner::new();
        let owner = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
        let request = request();
        let captured = capture_with_policy(
            owner.client.storage().unwrap(),
            &request,
            media,
            owner.active_queue_policy(NOW).unwrap(),
        )
        .await;
        let receipt = {
            let inner = owner.client.storage().unwrap();
            let mut store = FaultStore::new(inner, Fault::None);
            store.atomic_fault = Fault::Race(tokio::sync::Barrier::new(2));
            let repository = SubmissionRepository { store: &store };
            let (a, b) = tokio::join!(repository.commit(&captured), repository.commit(&captured));
            let a = a.unwrap();
            let b = b.unwrap();
            assert_ne!(a.is_replay(), b.is_replay());
            assert_eq!(a.operation_id(), b.operation_id());
            assert_eq!(a.intent_id(), b.intent_id());
            assert_eq!(a.committed_at_unix_ms(), b.committed_at_unix_ms());
            assert_eq!(store.commits.load(Ordering::SeqCst), 2);
            assert_records(inner, &captured, true).await;
            a
        };
        owner.shutdown().await.unwrap();
        drop(owner);
        let reopened = runtime(Some(root.path()), signer.clone(), "ws://127.0.0.1:19999").await;
        let recovered = reopened
            .submission_recover(&request)
            .await
            .unwrap()
            .unwrap();
        assert!(recovered.is_replay());
        assert_eq!(recovered.operation_id(), receipt.operation_id());
        assert_eq!(recovered.intent_id(), receipt.intent_id());
        assert_eq!(
            recovered.committed_at_unix_ms(),
            receipt.committed_at_unix_ms()
        );
        assert_records(reopened.client.storage().unwrap(), &captured, true).await;
        assert_eq!(signer.count(), 0);
        assert_eq!(signer.statuses.load(Ordering::SeqCst), 0);
        reopened.shutdown().await.unwrap();
    }
}
