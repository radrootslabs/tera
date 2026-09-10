use super::*;
use crate::runtime::product_surface::{AddCommandType, ComposerFormInput, LocalNetworkId};
use radroots_identity::PublicKey;
use radroots_storage::{
    authored_draft::{AuthoredDraft, AuthoredDraftRevision, DraftAppendReceipt},
    authored_draft_query::{AuthoredDraftPage, AuthoredDraftQuery},
    event::BoxFuture,
};
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) const AUTHOR: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";
const NOW: u64 = 1_800_000_000_000;
pub(super) fn scope() -> ComposerScope {
    ComposerScope::new(
        PublicKey::from_hex(AUTHOR).unwrap(),
        LocalNetworkId::new("nearby".into()).unwrap(),
    )
}
pub(super) fn form(content: &str) -> ComposerPartialForm {
    let mut input = ComposerFormInput::empty(AddCommandType::CreateFoodAvailability);
    input.content = content.into();
    input.price_amount = Some("-".into());
    input.event_start_date = Some("2026-09-".into());
    ComposerPartialForm::new(input).unwrap()
}
fn sequence(value: u64) -> ComposerEditSequence {
    ComposerEditSequence::new(value).unwrap()
}

#[tokio::test]
async fn a_different_record_from_the_owner_cannot_be_read_or_saved_under_the_requested_id() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let selected = scope();
    let id = ComposerId::new([1; 16]).unwrap();
    let unrelated = ComposerStorageRecord::initial(
        ComposerId::new([2; 16]).unwrap(),
        selected.clone(),
        sequence(1),
        form("unrelated"),
        NOW,
    )
    .unwrap();
    let store = FaultStore {
        inner: client.storage().unwrap(),
        fault: AppendFault::None,
        head_override: Some(unrelated.into_stored()),
        appends: AtomicUsize::new(0),
    };
    let repo = ComposerRepository {
        store: &store,
        scope: &selected,
    };
    let expected = ComposerPersistenceError::Record(ComposerStorageError::CorruptRecord);
    assert_eq!(repo.load(id).await.unwrap_err(), expected);
    assert_eq!(
        repo.save(
            id,
            ComposerRevision::INITIAL,
            sequence(2),
            form("unsaved"),
            NOW + 1
        )
        .await
        .unwrap_err(),
        expected
    );
    assert_eq!(store.appends.load(Ordering::SeqCst), 0);
}

enum AppendFault {
    None,
    BeforeCommit,
    WrongReceipt,
}
struct FaultStore<'a> {
    inner: &'a dyn AuthoredDraftStore,
    fault: AppendFault,
    head_override: Option<AuthoredDraft>,
    appends: AtomicUsize,
}
impl AuthoredDraftStore for FaultStore<'_> {
    fn query_authored_drafts(
        &self,
        query: AuthoredDraftQuery,
    ) -> BoxFuture<'_, Result<AuthoredDraftPage, Error>> {
        self.inner.query_authored_drafts(query)
    }
    fn append_authored_draft(
        &self,
        draft: AuthoredDraft,
        expected: Option<AuthoredDraftRevision>,
    ) -> BoxFuture<'_, Result<DraftAppendReceipt, Error>> {
        Box::pin(async move {
            self.appends.fetch_add(1, Ordering::SeqCst);
            match self.fault {
                AppendFault::None => self.inner.append_authored_draft(draft, expected).await,
                AppendFault::BeforeCommit => Err(Error::BackendUnavailable),
                AppendFault::WrongReceipt => Ok(DraftAppendReceipt::new(
                    draft.successor(
                        draft.payload().to_vec(),
                        draft.stage(),
                        draft.operation_id(),
                        draft.updated_at_unix_ms() + 1,
                    )?,
                    DraftAppendDisposition::Inserted,
                )),
            }
        })
    }
    fn authored_draft_head(
        &self,
        id: AuthoredDraftId,
    ) -> BoxFuture<'_, Result<Option<AuthoredDraft>, Error>> {
        Box::pin(async move {
            if let Some(head) = &self.head_override {
                return Ok(Some(head.clone()));
            }
            self.inner.authored_draft_head(id).await
        })
    }
    fn authored_draft_revision(
        &self,
        id: AuthoredDraftId,
        revision: AuthoredDraftRevision,
    ) -> BoxFuture<'_, Result<Option<AuthoredDraft>, Error>> {
        self.inner.authored_draft_revision(id, revision)
    }
    fn authored_draft_heads(
        &self,
        author: [u8; 32],
        limit: u16,
    ) -> BoxFuture<'_, Result<Vec<AuthoredDraft>, Error>> {
        self.inner.authored_draft_heads(author, limit)
    }
}

#[tokio::test]
async fn explicit_owner_replay_is_a_historical_receipt_and_never_regresses_the_head() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let selected = scope();
    let repo = ComposerRepository {
        store: client.storage().unwrap(),
        scope: &selected,
    };
    let id = ComposerId::generate().unwrap();
    let initial = repo
        .create(id, sequence(1), form("PRIVATE initial"), NOW)
        .await
        .unwrap();
    assert!(!initial.is_replay());
    let next = repo
        .save(
            id,
            ComposerRevision::INITIAL,
            sequence(8),
            form("PRIVATE changed"),
            NOW + 1,
        )
        .await
        .unwrap();
    assert_eq!(next.draft().revision().get(), 2);
    let replay = repo
        .create(id, sequence(1), form("PRIVATE initial"), NOW)
        .await
        .unwrap();
    assert!(replay.is_replay());
    assert_eq!(replay.draft(), initial.draft());
    assert_eq!(repo.load(id).await.unwrap().draft(), next.draft());
    assert_eq!(
        repo.create(id, sequence(1), form("different"), NOW)
            .await
            .unwrap_err(),
        ComposerPersistenceError::RevisionConflict
    );
    assert_eq!(
        repo.save(
            id,
            ComposerRevision::INITIAL,
            sequence(9),
            form("stale"),
            NOW + 2
        )
        .await
        .unwrap_err(),
        ComposerPersistenceError::RevisionConflict
    );
    assert!(!format!("{replay:?}").contains("PRIVATE"));
}

#[tokio::test]
async fn owner_failure_or_mismatched_receipt_never_acknowledges_or_changes_saved_work() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let selected = scope();
    let inner = client.storage().unwrap();
    let repo = ComposerRepository {
        store: inner,
        scope: &selected,
    };
    let id = ComposerId::generate().unwrap();
    repo.create(id, sequence(1), form("original"), NOW)
        .await
        .unwrap();
    let original = repo.load(id).await.unwrap();
    for (fault, expected) in [
        (
            AppendFault::BeforeCommit,
            ComposerPersistenceError::Storage(Error::BackendUnavailable),
        ),
        (
            AppendFault::WrongReceipt,
            ComposerPersistenceError::InvalidReceipt,
        ),
    ] {
        let store = FaultStore {
            inner,
            fault,
            head_override: None,
            appends: AtomicUsize::new(0),
        };
        let repo = ComposerRepository {
            store: &store,
            scope: &selected,
        };
        assert_eq!(
            repo.save(
                id,
                ComposerRevision::INITIAL,
                sequence(2),
                form("unsaved"),
                NOW + 1
            )
            .await
            .unwrap_err(),
            expected
        );
        assert_eq!(repo.load(id).await.unwrap().stored(), original.stored());
        let unsaved = ComposerId::generate().unwrap();
        assert_eq!(
            repo.create(unsaved, sequence(1), form("unsaved new"), NOW)
                .await
                .unwrap_err(),
            expected
        );
        assert_eq!(
            repo.load(unsaved).await.unwrap_err(),
            ComposerPersistenceError::NotFound
        );
        assert_eq!(store.appends.load(Ordering::SeqCst), 2);
    }
}

#[tokio::test]
async fn maximum_revision_and_nonadvancing_edit_sequences_fail_before_append() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let selected = scope();
    let id = ComposerId::generate().unwrap();
    let initial =
        ComposerStorageRecord::initial(id, selected.clone(), sequence(7), form("original"), NOW)
            .unwrap();
    let mut envelope = serde_json::to_value(initial.stored()).unwrap();
    envelope["revision"] = serde_json::json!(ComposerRevision::MAX);
    let maximum: AuthoredDraft = serde_json::from_value(envelope).unwrap();
    let maximum_edit = ComposerStorageRecord::initial(
        id,
        selected.clone(),
        sequence(u64::MAX),
        form("original"),
        NOW,
    )
    .unwrap();
    for (head, expected, edit, error) in [
        (
            maximum,
            ComposerRevision::new(ComposerRevision::MAX).unwrap(),
            sequence(8),
            ComposerPersistenceError::RevisionOverflow,
        ),
        (
            initial.stored().clone(),
            ComposerRevision::INITIAL,
            sequence(7),
            ComposerPersistenceError::EditSequenceConflict,
        ),
        (
            initial.into_stored(),
            ComposerRevision::INITIAL,
            sequence(6),
            ComposerPersistenceError::EditSequenceConflict,
        ),
        (
            maximum_edit.into_stored(),
            ComposerRevision::INITIAL,
            sequence(u64::MAX),
            ComposerPersistenceError::EditSequenceConflict,
        ),
    ] {
        let store = FaultStore {
            inner: client.storage().unwrap(),
            fault: AppendFault::None,
            head_override: Some(head),
            appends: AtomicUsize::new(0),
        };
        let repo = ComposerRepository {
            store: &store,
            scope: &selected,
        };
        assert_eq!(
            repo.save(id, expected, edit, form("unsaved"), NOW + 1)
                .await
                .unwrap_err(),
            error
        );
        assert_eq!(store.appends.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn backward_local_clock_preserves_order_and_invalid_time_does_not_append() {
    let client = radroots_sdk::ClientBuilder::memory_default()
        .build()
        .unwrap();
    let selected = scope();
    let repo = ComposerRepository {
        store: client.storage().unwrap(),
        scope: &selected,
    };
    let id = ComposerId::generate().unwrap();
    repo.create(id, sequence(1), form("original"), NOW)
        .await
        .unwrap();
    repo.save(
        id,
        ComposerRevision::INITIAL,
        sequence(9),
        form("changed"),
        NOW - 1,
    )
    .await
    .unwrap();
    let current = repo.load(id).await.unwrap();
    assert_eq!(current.stored().created_at_unix_ms(), NOW);
    assert_eq!(current.stored().updated_at_unix_ms(), NOW);
    assert_eq!(current.draft().revision().get(), 2);
    for time in [0, i64::MAX as u64 + 1] {
        assert_eq!(
            repo.save(
                id,
                current.draft().revision(),
                sequence(10),
                form("unsaved"),
                time
            )
            .await
            .unwrap_err(),
            ComposerPersistenceError::Record(ComposerStorageError::InvalidTimestamp)
        );
        assert_eq!(repo.load(id).await.unwrap().stored(), current.stored());
    }
}
