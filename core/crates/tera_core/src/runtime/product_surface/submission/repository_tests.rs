use super::*;
use crate::runtime::product_surface::submission::test_support::*;
use crate::runtime::product_surface::{ComposerId, ComposerRevision, SubmissionCommandId};

#[path = "fault_store.rs"]
mod fault_store;
use fault_store::{Fault, FaultStore};

#[tokio::test]
async fn concurrent_equivalent_reservations_reuse_one_winner_despite_different_clocks() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let inner = client.storage().unwrap();
    inner
        .append_authored_draft(source().into_stored(), None)
        .await
        .unwrap();
    let store = FaultStore::new(inner, Fault::Race(tokio::sync::Barrier::new(2)));
    let repo = SubmissionRepository { store: &store };
    let request = request();
    let (a, b) = tokio::join!(
        repo.reserve(&request, Some(NOW + 1)),
        repo.reserve(&request, Some(NOW + 2))
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.reservation_id(), b.reservation_id());
    assert_eq!(a.reserved_at_unix_ms(), b.reserved_at_unix_ms());
    assert_ne!(a.is_replay(), b.is_replay());
    assert_eq!(a.captured(), b.captured());
    assert_eq!(store.append_count(), 2);
    let head = inner
        .authored_draft_head(a.reservation_id())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(head.revision().get(), 1);
    assert!(
        inner
            .authored_draft_revision(
                a.reservation_id(),
                radroots_storage::authored_draft::AuthoredDraftRevision::new(2).unwrap()
            )
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        inner
            .authored_draft_heads(*head.author(), 20)
            .await
            .unwrap()
            .len(),
        2
    );
}

#[tokio::test]
async fn changed_request_conflicts_and_intentional_equal_posts_get_distinct_reservations() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let inner = client.storage().unwrap();
    inner
        .append_authored_draft(source().into_stored(), None)
        .await
        .unwrap();
    let store = FaultStore::new(inner, Fault::None);
    let repo = SubmissionRepository { store: &store };
    let request = request();
    let first = repo.reserve(&request, Some(NOW)).await.unwrap();
    let mut variants = [request.clone(), request.clone(), request.clone()];
    variants[0].scope = scope(AUTHOR, "other");
    variants[1].composer_id = ComposerId::new([9; 16]).unwrap();
    variants[2].expected_revision = ComposerRevision::new(2).unwrap();
    for changed in variants {
        assert_eq!(
            repo.reserve(&changed, Some(NOW)).await.unwrap_err(),
            E::IdempotencyConflict
        );
    }
    assert_eq!(store.append_count(), 1);
    let mut second = request.clone();
    second.command_id = SubmissionCommandId::generate().unwrap();
    let second = repo.reserve(&second, Some(NOW)).await.unwrap();
    assert_ne!(first.reservation_id(), second.reservation_id());
    assert_eq!(first.captured(), second.captured());
    let replay = repo.reserve(&request, None).await.unwrap();
    assert!(replay.is_replay());
    assert_eq!(replay.reserved_at_unix_ms(), first.reserved_at_unix_ms());
    assert!(!format!("{replay:?}").contains("PRIVATE"));
}

#[tokio::test]
async fn failed_and_unconfirmed_appends_never_acknowledge_but_same_command_recovers() {
    for fault in [
        Fault::BeforeCommit,
        Fault::LostCallback,
        Fault::WrongReceipt,
    ] {
        let client = radroots_sdk::ClientBuilder::memory_default()
            .build()
            .unwrap();
        let inner = client.storage().unwrap();
        inner
            .append_authored_draft(source().into_stored(), None)
            .await
            .unwrap();
        let before = matches!(fault, Fault::BeforeCommit);
        let wrong = matches!(fault, Fault::WrongReceipt);
        let store = FaultStore::new(inner, fault);
        let repo = SubmissionRepository { store: &store };
        let request = request();
        assert_eq!(
            repo.reserve(&request, Some(NOW)).await.unwrap_err(),
            if wrong {
                E::InvalidReceipt
            } else {
                E::Storage(Error::BackendUnavailable)
            }
        );
        assert_eq!(
            inner
                .authored_draft_head(record::reservation_id(&request).unwrap())
                .await
                .unwrap()
                .is_none(),
            before
        );
        let recovered = repo.reserve(&request, Some(NOW + 1)).await.unwrap();
        assert_eq!(recovered.is_replay(), !before);
        assert_eq!(store.append_count(), if before { 2 } else { 1 });
        assert_eq!(recovered.captured(), source().draft());
    }
}

#[tokio::test]
async fn historical_source_integrity_and_reservation_corruption_do_not_create_replacements() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let inner = client.storage().unwrap();
    let source = source();
    inner
        .append_authored_draft(source.stored().clone(), None)
        .await
        .unwrap();
    let request = request();
    let repo = SubmissionRepository { store: inner };
    let receipt = repo.reserve(&request, Some(NOW)).await.unwrap();
    let mut store = FaultStore::new(inner, Fault::None);
    store.historical_override = Some(
        AuthoredDraft::initial(
            source.stored().draft_id(),
            *source.stored().author(),
            source.stored().payload_schema(),
            b"changed bytes".to_vec(),
            source.stored().stage(),
            None,
            NOW,
        )
        .unwrap()
        .with_scope(source.stored().scope().unwrap())
        .unwrap(),
    );
    assert_eq!(
        SubmissionRepository { store: &store }
            .reserve(&request, Some(NOW + 1))
            .await
            .unwrap_err(),
        E::CorruptRecord
    );
    assert_eq!(store.append_count(), 0);
    let reservation = inner
        .authored_draft_head(receipt.reservation_id())
        .await
        .unwrap()
        .unwrap();
    inner
        .append_authored_draft(
            reservation
                .successor(
                    reservation.payload().to_vec(),
                    reservation.stage(),
                    None,
                    NOW + 1,
                )
                .unwrap(),
            Some(reservation.revision()),
        )
        .await
        .unwrap();
    let store = FaultStore::new(inner, Fault::None);
    assert_eq!(
        SubmissionRepository { store: &store }
            .reserve(&request, Some(NOW + 2))
            .await
            .unwrap_err(),
        E::CorruptRecord
    );
    assert_eq!(store.append_count(), 0);
}

#[tokio::test]
async fn first_reservation_requires_existing_current_source_and_a_valid_clock() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let inner = client.storage().unwrap();
    let store = FaultStore::new(inner, Fault::None);
    let repo = SubmissionRepository { store: &store };
    let mut request = request();
    assert_eq!(
        repo.reserve(&request, Some(NOW)).await.unwrap_err(),
        E::Source(SourceError::NotFound)
    );
    inner
        .append_authored_draft(source().into_stored(), None)
        .await
        .unwrap();
    assert_eq!(
        repo.reserve(&request, None).await.unwrap_err(),
        E::ClockUnavailable
    );
    request.expected_revision = ComposerRevision::new(2).unwrap();
    assert_eq!(
        repo.reserve(&request, Some(NOW)).await.unwrap_err(),
        E::Source(SourceError::RevisionConflict)
    );
    request.expected_revision = ComposerRevision::INITIAL;
    request.scope = scope(AUTHOR, "other");
    assert_eq!(
        repo.reserve(&request, Some(NOW)).await.unwrap_err(),
        E::Source(SourceError::ScopeMismatch)
    );
    assert_eq!(store.append_count(), 0);
}
