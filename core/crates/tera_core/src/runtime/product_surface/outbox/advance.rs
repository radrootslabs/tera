//! Shared bounded effect phases for validated legacy and scoped operation requests.

use super::*;

impl TeraRuntime {
    pub(in crate::runtime::product_surface) async fn advance_push_request(
        &self,
        request: PushRequest,
    ) -> Result<(), Phase1DraftError> {
        self.advance_push_request_with_clock(request, phase1_operation_now_unix_ms)
            .await
    }

    pub(in crate::runtime::product_surface) async fn advance_push_request_with_clock(
        &self,
        request: PushRequest,
        clock: impl Fn() -> Result<u64, Phase1DraftError> + Send + Sync,
    ) -> Result<(), Phase1DraftError> {
        self.advance_push_request_inner(request, None, clock).await
    }

    pub(in crate::runtime::product_surface) async fn advance_push_request_selected(
        &self,
        request: PushRequest,
        selected: TargetSet,
    ) -> Result<(), Phase1DraftError> {
        self.advance_push_request_inner(request, Some(selected), phase1_operation_now_unix_ms)
            .await
    }

    async fn advance_push_request_inner(
        &self,
        request: PushRequest,
        selected: Option<TargetSet>,
        clock: impl Fn() -> Result<u64, Phase1DraftError> + Send + Sync,
    ) -> Result<(), Phase1DraftError> {
        let operation_id = request.operation_id();
        self.require_legacy_publication_running(operation_id)
            .await?;
        let sync = self.sync()?;
        let mut status = sync
            .push_status(operation_id)
            .await
            .map_err(|_| Phase1DraftError::Operation)?
            .ok_or(Phase1DraftError::Corrupt)?;

        // Existing fact reconciliation is a bounded local action and never
        // also sends a retry. Preserve it even after a delivery deadline.
        if status.artifact().admission_state().is_admitted()
            && !status.delivery_plan().state().is_terminal()
            && status
                .delivery_plan()
                .pending_delivery_facts()
                .next()
                .is_some()
        {
            sync.deliver_push(operation_id)
                .await
                .map_err(|_| Phase1DraftError::Operation)?;
            return Ok(());
        }
        if !self.publication_retry_at(&status, clock()?)?.may_start() {
            return Ok(());
        }

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
            && self.publication_retry_at(&status, clock()?)?.may_start()
            && matches!(
                status.delivery_plan().state(),
                AuthoredDeliveryState::Pending | AuthoredDeliveryState::Retryable
            )
        {
            match selected {
                Some(targets) => sync.deliver_push_selected(operation_id, targets).await,
                None => sync.deliver_push(operation_id).await,
            }
            .map_err(|_| Phase1DraftError::Operation)?;
        }
        Ok(())
    }
}
