use super::{
    tests::{AUTHOR, AppendFault, FaultStore, form, scope},
    *,
};
use crate::runtime::{
    builder::RuntimeBuilder,
    store::{MobileUserStoreConfig, ProtectedDataAvailability},
};
use std::sync::atomic::{AtomicUsize, Ordering};

#[tokio::test]
async fn sqlite_composer_faults_reopen_partial_work_and_refuse_stale_replacement() {
    const NOW: u64 = 1_800_000_000_000;
    for fault in [
        AppendFault::BeforeCommit,
        AppendFault::LostCallback,
        AppendFault::CapacityBefore,
        AppendFault::CapacityAfter,
        AppendFault::WrongReceipt,
    ] {
        let installed = matches!(
            fault,
            AppendFault::LostCallback | AppendFault::CapacityAfter
        );
        let expected_error = match fault {
            AppendFault::CapacityBefore | AppendFault::CapacityAfter => {
                ComposerPersistenceError::Storage(Error::SpaceInsufficient)
            }
            AppendFault::WrongReceipt => ComposerPersistenceError::InvalidReceipt,
            _ => ComposerPersistenceError::Storage(Error::BackendUnavailable),
        };
        let root = tempfile::tempdir().unwrap();
        let config = MobileUserStoreConfig::from_encoded(
            root.path(),
            AUTHOR,
            &"02".repeat(32),
            NOW,
            ProtectedDataAvailability::Available,
        )
        .unwrap();
        std::fs::create_dir_all(config.owner_directory()).unwrap();
        let owner = RuntimeBuilder::new(config.clone()).build().await.unwrap();
        let selected = scope();
        let id = ComposerId::generate().unwrap();
        let changed = form("\n unsaved café 1.\n2026- ");
        let initial = owner
            .composer_create(
                &selected,
                id,
                ComposerEditSequence::INITIAL,
                form("initial incomplete -"),
            )
            .await
            .unwrap();
        {
            let store = FaultStore {
                inner: owner.client.storage().unwrap(),
                fault,
                head_override: None,
                appends: AtomicUsize::new(0),
            };
            let repository = ComposerRepository {
                store: &store,
                scope: &selected,
            };
            assert_eq!(
                repository
                    .save(
                        id,
                        ComposerRevision::INITIAL,
                        ComposerEditSequence::new(2).unwrap(),
                        changed.clone(),
                        NOW + 1,
                    )
                    .await
                    .unwrap_err(),
                expected_error
            );
            assert_eq!(store.appends.load(Ordering::SeqCst), 1);
        }
        owner.shutdown().await.unwrap();
        drop(owner);
        let reopened = RuntimeBuilder::new(config.clone()).build().await.unwrap();
        let recovered = reopened.composer_load(&selected, id).await.unwrap();
        assert_eq!(recovered.id(), id);
        assert_eq!(recovered.scope(), &selected);
        assert_eq!(recovered.revision().get(), if installed { 2 } else { 1 });
        if installed {
            assert_eq!(recovered.form(), &changed);
            assert_eq!(recovered.edit_sequence().get(), 2);
        } else {
            assert_eq!(&recovered, initial.draft());
        }
        let newest = reopened
            .composer_save(
                &selected,
                id,
                recovered.revision(),
                ComposerEditSequence::new(3).unwrap(),
                form("newer edit must survive"),
            )
            .await
            .unwrap();
        assert_eq!(
            reopened
                .composer_save(
                    &selected,
                    id,
                    ComposerRevision::INITIAL,
                    ComposerEditSequence::new(2).unwrap(),
                    changed,
                )
                .await
                .unwrap_err(),
            ComposerPersistenceError::RevisionConflict
        );
        assert_eq!(
            reopened.composer_load(&selected, id).await.unwrap(),
            *newest.draft()
        );
        reopened.shutdown().await.unwrap();
        drop(reopened);
        let final_owner = RuntimeBuilder::new(config).build().await.unwrap();
        assert_eq!(
            final_owner.composer_load(&selected, id).await.unwrap(),
            *newest.draft()
        );
        final_owner.shutdown().await.unwrap();
    }
}
