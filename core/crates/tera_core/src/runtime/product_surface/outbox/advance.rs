//! Shared bounded effect phases for validated legacy and scoped operation requests.

use super::*;

impl TeraRuntime {
    #[cfg(test)]
    pub(in crate::runtime::product_surface) async fn advance_push_request_with_clock(
        &self,
        request: PushRequest,
        clock: impl Fn() -> Result<u64, Phase1DraftError> + Send + Sync,
    ) -> Result<(), Phase1DraftError> {
        self.advance_push_request_inner(request, None, None, clock)
            .await
    }

    pub(in crate::runtime::product_surface) async fn advance_owned_push_request(
        &self,
        request: PushRequest,
        head: &AuthoredDraft,
    ) -> Result<(), Phase1DraftError> {
        self.advance_push_request_inner(request, None, Some(head), phase1_operation_now_unix_ms)
            .await
    }

    #[cfg(test)]
    pub(super) async fn advance_owned_push_request_with_clock(
        &self,
        request: PushRequest,
        head: &AuthoredDraft,
        clock: impl Fn() -> Result<u64, Phase1DraftError> + Send + Sync,
    ) -> Result<(), Phase1DraftError> {
        self.advance_push_request_inner(request, None, Some(head), clock)
            .await
    }

    pub(in crate::runtime::product_surface) async fn advance_push_request_selected(
        &self,
        request: PushRequest,
        selected: TargetSet,
        owner: &AuthoredDraft,
    ) -> Result<(), Phase1DraftError> {
        self.advance_push_request_inner(
            request,
            Some(selected),
            Some(owner),
            phase1_operation_now_unix_ms,
        )
        .await
    }

    async fn advance_push_request_inner(
        &self,
        request: PushRequest,
        selected: Option<TargetSet>,
        owner: Option<&AuthoredDraft>,
        clock: impl Fn() -> Result<u64, Phase1DraftError> + Send + Sync,
    ) -> Result<(), Phase1DraftError> {
        let operation_id = request.operation_id();
        self.require_legacy_publication_running(operation_id)
            .await?;
        let sync = self.sync()?;
        let mut status = sync
            .push_status(operation_id)
            .await
            .map_err(Phase1DraftError::sync_error)?
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
                .map_err(Phase1DraftError::sync_error)?;
            return Ok(());
        }
        let now = clock()?;
        if !self.publication_retry_at(&status, now)?.may_start() {
            return Ok(());
        }
        let authority_plan = request.plan().clone();
        self.require_publication_source(&authority_plan, owner)
            .await?;

        let intent = owner
            .map(super::super::coordinate::intent_from_draft)
            .transpose()?
            .flatten();
        if (30_000..40_000).contains(&request.plan().body().kind())
            && intent.as_ref().is_none_or(|intent| {
                intent.event_id != *request.plan().expected_event_id().as_bytes()
                    || intent.author != *request.plan().author().as_bytes()
            })
        {
            return Err(Phase1DraftError::InvalidRevision);
        }
        let _coordinate = match owner {
            Some(head) => self.admit_coordinate(head).await?,
            None => None,
        };
        if let Some(intent) = &intent {
            self.require_coordinate_current_at(intent, now / 1_000)
                .await?;
        }

        if matches!(
            status.artifact().signing_state(),
            SigningState::Planned | SigningState::Retryable
        ) {
            sync.sign_prepared(request)
                .await
                .map_err(Phase1DraftError::sync_error)?;
            status = sync
                .push_status(operation_id)
                .await
                .map_err(Phase1DraftError::sync_error)?
                .ok_or(Phase1DraftError::Corrupt)?;
        }
        if status.artifact().signing_state() == SigningState::Signed
            && matches!(
                status.artifact().admission_state(),
                AdmissionState::Pending | AdmissionState::Retryable
            )
        {
            self.require_publication_source(&authority_plan, owner)
                .await?;
            if let Some(intent) = &intent {
                self.require_coordinate_current_at(intent, clock()? / 1_000)
                    .await?;
            }
            sync.admit_signed(operation_id)
                .await
                .map_err(Phase1DraftError::sync_error)?;
            status = sync
                .push_status(operation_id)
                .await
                .map_err(Phase1DraftError::sync_error)?
                .ok_or(Phase1DraftError::Corrupt)?;
        }
        if status.artifact().admission_state().is_admitted()
            && self.publication_retry_at(&status, clock()?)?.may_start()
            && matches!(
                status.delivery_plan().state(),
                AuthoredDeliveryState::Pending | AuthoredDeliveryState::Retryable
            )
        {
            self.require_publication_source(&authority_plan, owner)
                .await?;
            if let Some(intent) = &intent {
                self.require_coordinate_current_at(intent, clock()? / 1_000)
                    .await?;
            }
            match selected {
                Some(targets) => sync.deliver_push_selected(operation_id, targets).await,
                None => sync.deliver_push(operation_id).await,
            }
            .map_err(Phase1DraftError::sync_error)?;
        }
        Ok(())
    }
}
