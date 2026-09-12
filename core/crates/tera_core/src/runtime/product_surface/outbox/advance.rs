//! Shared bounded effect phases for validated legacy and scoped operation requests.

use super::*;

impl TeraRuntime {
    pub(in crate::runtime::product_surface) async fn advance_push_request(
        &self,
        request: PushRequest,
    ) -> Result<(), Phase1DraftError> {
        let operation_id = request.operation_id();
        let sync = self.sync()?;
        let mut status = sync
            .push_status(operation_id)
            .await
            .map_err(|_| Phase1DraftError::Operation)?
            .ok_or(Phase1DraftError::Corrupt)?;

        if matches!(
            status.artifact().signing_state(),
            SigningState::Planned | SigningState::Retryable
        ) {
            sync.sign_prepared(request)
                .await
                .map_err(|_| Phase1DraftError::Operation)?;
            status = sync
                .push_status(operation_id)
                .await
                .map_err(|_| Phase1DraftError::Operation)?
                .ok_or(Phase1DraftError::Corrupt)?;
        }
        if status.artifact().signing_state() == SigningState::Signed
            && matches!(
                status.artifact().admission_state(),
                AdmissionState::Pending | AdmissionState::Retryable
            )
        {
            sync.admit_signed(operation_id)
                .await
                .map_err(|_| Phase1DraftError::Operation)?;
            status = sync
                .push_status(operation_id)
                .await
                .map_err(|_| Phase1DraftError::Operation)?
                .ok_or(Phase1DraftError::Corrupt)?;
        }
        if status.artifact().admission_state().is_admitted()
            && matches!(
                status.delivery_plan().state(),
                AuthoredDeliveryState::Pending | AuthoredDeliveryState::Retryable
            )
        {
            sync.deliver_push(operation_id)
                .await
                .map_err(|_| Phase1DraftError::Operation)?;
        }
        Ok(())
    }
}
