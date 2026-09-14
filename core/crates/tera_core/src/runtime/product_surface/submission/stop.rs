//! Stop admission remains available while an owned signer or transport is pending.

use super::{
    SubmissionOperationError as E, SubmissionOperationStatus, SubmissionReservationRequest,
};
use crate::{TeraRuntime, runtime::product_surface::Phase1DraftError};

impl TeraRuntime {
    /// Persists stop on the original shared operation without waiting for its
    /// application admission slot or cancelling its retained effect callbacks.
    pub async fn submission_request_stop(
        &self,
        request: &SubmissionReservationRequest,
    ) -> Result<SubmissionOperationStatus, E> {
        let _command = self.lifecycle.enter().map_err(Phase1DraftError::from)?;
        let (loaded, push) = self.load_submission_operation(request).await?;
        if push.delivery_plan().stop_requested_at_unix_ms().is_some() {
            return self.submission_operation_status(request).await;
        }
        let result = self
            .sync()?
            .cancel_push(loaded.request.operation_id())
            .await;
        // Concurrent stop or a lost callback can leave a durable first stop.
        // A receipt alone never substitutes for current backend state.
        let status = self.submission_operation_status(request).await?;
        if status
            .delivery_evidence()
            .stop_requested_at_unix_ms
            .is_some()
        {
            return Ok(status);
        }
        result.map_err(|_| Phase1DraftError::Operation)?;
        Err(E::Corrupt)
    }

    pub(super) async fn require_submission_running(
        &self,
        request: &SubmissionReservationRequest,
    ) -> Result<(), E> {
        let status = self.submission_operation_status(request).await?;
        if status
            .delivery_evidence()
            .stop_requested_at_unix_ms
            .is_some()
        {
            return Err(E::Stopped);
        }
        Ok(())
    }
}
