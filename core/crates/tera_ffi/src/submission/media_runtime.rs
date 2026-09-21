use super::TeraRuntime;
use crate::{
    FfiRuntimeChangeKind, FfiSubmissionMediaInput, FfiSubmissionOperationRecord,
    FfiSubmissionUploadJobRecord, FfiSubmissionUploadRenewal, FfiSubmissionUploadResponse,
    MOBILE_FFI_SCHEMA_VERSION, TeraAppError, dto::PreparedMedia,
};
use tera_core::runtime::product_surface::{SubmissionMediaRequest, SubmissionMediaResponse};

impl TryFrom<FfiSubmissionMediaInput> for SubmissionMediaRequest {
    type Error = TeraAppError;
    fn try_from(value: FfiSubmissionMediaInput) -> Result<Self, Self::Error> {
        if value.schema_version != MOBILE_FFI_SCHEMA_VERSION {
            return Err(TeraAppError::invalid_argument(
                "submission_schema_unsupported",
            ));
        }
        let request = value.request.try_into()?;
        PreparedMedia::try_from(value.media)?
            .into_submission_media(request, value.expected_revision)
    }
}

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl TeraRuntime {
    pub async fn submission_upload_media(
        &self,
        input: FfiSubmissionMediaInput,
    ) -> Result<FfiSubmissionOperationRecord, TeraAppError> {
        let result = self.inner.submission_upload_media(input.try_into()?).await;
        self.subscriptions.notify(FfiRuntimeChangeKind::Media, None);
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, None);
        Ok((&result?).into())
    }

    pub async fn submission_prepare_upload(
        &self,
        input: FfiSubmissionMediaInput,
    ) -> Result<FfiSubmissionUploadJobRecord, TeraAppError> {
        let input = input.try_into()?;
        let result = self.inner.submission_prepare_native_upload(input).await;
        self.subscriptions.notify(FfiRuntimeChangeKind::Media, None);
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, None);
        Ok(result?.into())
    }

    pub async fn submission_renew_native_upload(
        &self,
        input: FfiSubmissionMediaInput,
        renewal: FfiSubmissionUploadRenewal,
    ) -> Result<FfiSubmissionUploadJobRecord, TeraAppError> {
        let result = self
            .inner
            .submission_renew_native_upload(input.try_into()?, renewal.try_into()?)
            .await;
        self.subscriptions.notify(FfiRuntimeChangeKind::Media, None);
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, None);
        Ok(result?.into())
    }

    pub async fn submission_renew_upload_media(
        &self,
        input: FfiSubmissionMediaInput,
        renewal: FfiSubmissionUploadRenewal,
    ) -> Result<FfiSubmissionOperationRecord, TeraAppError> {
        let result = self
            .inner
            .submission_renew_upload_media(input.try_into()?, renewal.try_into()?)
            .await;
        self.subscriptions.notify(FfiRuntimeChangeKind::Media, None);
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, None);
        Ok((&result?).into())
    }

    pub async fn submission_complete_upload(
        &self,
        input: FfiSubmissionMediaInput,
        response: FfiSubmissionUploadResponse,
    ) -> Result<FfiSubmissionOperationRecord, TeraAppError> {
        if response.schema_version != MOBILE_FFI_SCHEMA_VERSION {
            return Err(TeraAppError::invalid_argument(
                "submission_schema_unsupported",
            ));
        }
        // Bound the complete native evidence before reading or hashing media.
        let response = SubmissionMediaResponse::new(
            response.status_code,
            response.media_type,
            response.content_encoding,
            response.body,
        )?;
        let input = input.try_into()?;
        let result = self
            .inner
            .submission_complete_native_upload(input, response)
            .await;
        self.subscriptions.notify(FfiRuntimeChangeKind::Media, None);
        self.subscriptions
            .notify(FfiRuntimeChangeKind::Drafts, None);
        Ok((&result?).into())
    }
}
