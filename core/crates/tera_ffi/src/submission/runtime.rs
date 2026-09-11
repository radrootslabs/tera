use super::TeraRuntime;
use crate::{FfiSubmissionReservationReceipt, FfiSubmissionReservationRequest, TeraAppError};

#[cfg_attr(not(coverage_nightly), uniffi::export(async_runtime = "tokio"))]
impl TeraRuntime {
    /// Retrying the original request recovers a committed reservation after a lost callback.
    pub async fn submission_reserve(
        &self,
        request: FfiSubmissionReservationRequest,
    ) -> Result<FfiSubmissionReservationReceipt, TeraAppError> {
        let request = request.try_into()?;
        let receipt = self.inner.submission_reserve(&request).await?;
        Ok((&receipt).into())
    }
}
