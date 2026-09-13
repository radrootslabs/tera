use super::TeraRuntime;
use crate::{
    FfiComposerScopeRecord, FfiPreparedMediaInput, FfiRuntimeChangeKind,
    FfiSubmissionOperationRecord, FfiSubmissionPageRecord, FfiSubmissionReservationRequest,
    TeraAppError, dto::PreparedMedia,
};
use tera_core::runtime::product_surface::{SubmissionOperationError, SubmissionReservationRequest};

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl TeraRuntime {
    /// Confirms the local immutable operation before any upload, signer or relay effect.
    /// Replays do not reopen media files or consult new publication settings.
    pub async fn submission_prepare(
        &self,
        request: FfiSubmissionReservationRequest,
        media: Vec<FfiPreparedMediaInput>,
    ) -> Result<FfiSubmissionOperationRecord, TeraAppError> {
        let request: SubmissionReservationRequest = request.try_into()?;
        match self.inner.submission_operation_status(&request).await {
            Ok(status) => return Ok((&status).into()),
            Err(SubmissionOperationError::NotFound) => {}
            Err(error) => return Err(error.into()),
        }
        if media.len() > 20 {
            return Err(TeraAppError::invalid_argument("submission_media_invalid"));
        }
        let bytes = media
            .into_iter()
            .map(|input| PreparedMedia::try_from(input).map(PreparedMedia::into_submission_bytes))
            .collect::<Result<_, _>>()?;
        let receipt = self.inner.submission_prepare(&request, bytes).await?;
        self.subscriptions.notify(
            FfiRuntimeChangeKind::Drafts,
            Some(hex::encode(receipt.intent_id().as_bytes())),
        );
        Ok((&self.inner.submission_operation_status(&request).await?).into())
    }

    /// An absent operation is distinct from a damaged or temporarily unreadable operation.
    pub async fn submission_recover(
        &self,
        request: FfiSubmissionReservationRequest,
    ) -> Result<Option<FfiSubmissionOperationRecord>, TeraAppError> {
        let request = request.try_into()?;
        match self.inner.submission_operation_status(&request).await {
            Ok(status) => Ok(Some((&status).into())),
            Err(SubmissionOperationError::NotFound) => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub async fn submission_status(
        &self,
        request: FfiSubmissionReservationRequest,
    ) -> Result<FfiSubmissionOperationRecord, TeraAppError> {
        let request = request.try_into()?;
        Ok((&self.inner.submission_operation_status(&request).await?).into())
    }

    pub async fn submission_advance(
        &self,
        request: FfiSubmissionReservationRequest,
        expected_revision: u64,
    ) -> Result<FfiSubmissionOperationRecord, TeraAppError> {
        let request = request.try_into()?;
        let result = self
            .inner
            .submission_advance(&request, expected_revision)
            .await;
        // A failure can follow a durable queue, signature or delivery receipt.
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, None);
        Ok((&result?).into())
    }

    pub async fn submission_page(
        &self,
        scope: FfiComposerScopeRecord,
        limit: u16,
        cursor: Option<String>,
    ) -> Result<FfiSubmissionPageRecord, TeraAppError> {
        let scope = scope.try_into()?;
        Ok((&self
            .inner
            .submission_page(&scope, limit, cursor.as_deref())
            .await?)
            .into())
    }
}
