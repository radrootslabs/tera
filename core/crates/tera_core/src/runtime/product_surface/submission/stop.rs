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
        result.map_err(Phase1DraftError::sync_error)?;
        Err(E::Corrupt)
    }

    pub(super) async fn require_submission_running(
        &self,
        request: &SubmissionReservationRequest,
    ) -> Result<(), E> {
        self.require_restore_effects_allowed()
            .await
            .map_err(Phase1DraftError::from)?;
        let configuration = self.publication_configuration.read().await;
        if !configuration.allowed {
            self.submission_request_stop(request).await?;
            return Err(E::Stopped);
        }
        let status = self.submission_operation_status(request).await?;
        if status
            .delivery_evidence()
            .stop_requested_at_unix_ms
            .is_some()
        {
            return Err(E::Stopped);
        }
        let relays = self
            .client
            .nostr_status()
            .map_err(|_| Phase1DraftError::OperationUnavailable)?;
        let removed = status
            .push()
            .delivery_plan()
            .intent()
            .target_set()
            .targets()
            .iter()
            .any(|target| {
                !relays.as_ref().is_some_and(|profile| {
                    profile.relays().iter().any(|relay| {
                        relay.endpoint().access().can_write()
                            && relay.endpoint().url().as_str() == target.uri().as_str()
                    })
                })
            });
        if removed {
            self.submission_request_stop(request).await?;
            return Err(E::Stopped);
        }
        Ok(())
    }
}
