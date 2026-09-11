use radroots_storage::{
    Error,
    authored_draft::{
        AuthoredDraft, AuthoredDraftId, AuthoredDraftRevision, AuthoredDraftStore,
        DraftAppendReceipt,
    },
    authored_draft_query::{AuthoredDraftPage, AuthoredDraftQuery},
    event::BoxFuture,
};
use std::sync::atomic::{AtomicUsize, Ordering};

pub(super) enum Fault {
    None,
    BeforeCommit,
    LostCallback,
    WrongReceipt,
    Race(tokio::sync::Barrier),
}

pub(super) struct FaultStore<'a> {
    pub inner: &'a dyn AuthoredDraftStore,
    pub fault: Fault,
    pub appends: AtomicUsize,
    pub historical_override: Option<AuthoredDraft>,
}

impl<'a> FaultStore<'a> {
    pub fn new(inner: &'a dyn AuthoredDraftStore, fault: Fault) -> Self {
        Self {
            inner,
            fault,
            appends: AtomicUsize::new(0),
            historical_override: None,
        }
    }
    pub fn append_count(&self) -> usize {
        self.appends.load(Ordering::SeqCst)
    }
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
            let attempt = self.appends.fetch_add(1, Ordering::SeqCst);
            if attempt == 0 && matches!(self.fault, Fault::BeforeCommit) {
                return Err(Error::BackendUnavailable);
            }
            if let Fault::Race(barrier) = &self.fault {
                barrier.wait().await;
            }
            let receipt = self
                .inner
                .append_authored_draft(draft.clone(), expected)
                .await?;
            if attempt == 0 {
                match self.fault {
                    Fault::LostCallback => return Err(Error::BackendUnavailable),
                    Fault::WrongReceipt => {
                        return Ok(DraftAppendReceipt::new(
                            draft.successor(
                                draft.payload().to_vec(),
                                draft.stage(),
                                None,
                                draft.updated_at_unix_ms() + 1,
                            )?,
                            receipt.disposition(),
                        ));
                    }
                    _ => {}
                }
            }
            Ok(receipt)
        })
    }
    fn authored_draft_head(
        &self,
        id: AuthoredDraftId,
    ) -> BoxFuture<'_, Result<Option<AuthoredDraft>, Error>> {
        self.inner.authored_draft_head(id)
    }
    fn authored_draft_revision(
        &self,
        id: AuthoredDraftId,
        revision: AuthoredDraftRevision,
    ) -> BoxFuture<'_, Result<Option<AuthoredDraft>, Error>> {
        Box::pin(async move {
            if let Some(value) = &self.historical_override {
                return Ok(Some(value.clone()));
            }
            self.inner.authored_draft_revision(id, revision).await
        })
    }
    fn authored_draft_heads(
        &self,
        author: [u8; 32],
        limit: u16,
    ) -> BoxFuture<'_, Result<Vec<AuthoredDraft>, Error>> {
        self.inner.authored_draft_heads(author, limit)
    }
}
