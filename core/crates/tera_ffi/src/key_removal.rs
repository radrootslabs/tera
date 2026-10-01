use crate::{
    FfiDraftStatusRecord, FfiRuntimeChangeKind, TeraAppError, TeraRuntime, dto::decode_id,
};

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl TeraRuntime {
    pub async fn prepare_retraction_for_key_removal(
        &self,
        draft_id: String,
        expected_revision: u64,
    ) -> Result<FfiDraftStatusRecord, TeraAppError> {
        let id = decode_id(&draft_id, "invalid_draft_id")?;
        let result = self
            .inner
            .prepare_retraction_for_key_removal(id, expected_revision)
            .await;
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, Some(draft_id));
        result.map(Into::into).map_err(Into::into)
    }
}
