//! Repair derived local visibility from durable signed truth without delivery.

use super::{
    SubmissionOperationError as E, SubmissionOperationStatus, SubmissionReservationRequest, intent,
};
use crate::{
    TeraRuntime,
    runtime::product_surface::{
        LocalNetwork, Phase1DraftError, TodayProjectionUpdate, phase1_operation_now_unix_ms,
    },
};
use radroots_event_codec::{admission::admit_verified_event, verify::verify_nip01_event};

/// A local projection acknowledgement, never evidence of a relay attempt.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SubmissionLocalReceipt {
    status: SubmissionOperationStatus,
    changed: bool,
}

impl SubmissionLocalReceipt {
    pub fn status(&self) -> &SubmissionOperationStatus {
        &self.status
    }
    pub const fn changed(&self) -> bool {
        self.changed
    }
}

impl TeraRuntime {
    /// Reconciles only local admission and its active-author projection. This
    /// never invokes a signer, changes the frozen intent or schedules transport.
    pub async fn submission_reconcile_local(
        &self,
        request: &SubmissionReservationRequest,
        context: &LocalNetwork,
    ) -> Result<SubmissionLocalReceipt, E> {
        let _command = self.lifecycle.enter().map_err(Phase1DraftError::from)?;
        self.validate_submission_owner(request)?;
        if request.scope().local_network() != &context.id {
            return Err(E::Corrupt);
        }
        let _admission = self
            .mutations
            .draft(*intent::intent_id(request)?.as_bytes())?;
        let status = self.submission_operation_status(request).await?;
        if status
            .delivery_evidence()
            .stop_requested_at_unix_ms
            .is_some()
            && !status.push().artifact().admission_state().is_admitted()
        {
            // Preserve the shared stop fence. Reading retained signed evidence
            // remains available; stop does not authorize a new local admission.
            return Ok(SubmissionLocalReceipt {
                status,
                changed: false,
            });
        }
        let Some(signed) = status.push().artifact().signed() else {
            return Ok(SubmissionLocalReceipt {
                status,
                changed: false,
            });
        };
        // Recheck real cryptography even when storage presents a typed artifact.
        // No permissive producer/test typestate can become local event truth.
        let verified =
            verify_nip01_event(signed.event().envelope().clone()).map_err(|_| E::Corrupt)?;
        admit_verified_event(verified).map_err(|_| E::Corrupt)?;
        let plan = status
            .push()
            .artifact()
            .plan()
            .ok_or(E::Corrupt)?
            .decode()
            .map_err(|_| E::Corrupt)?;
        if signed.event().id() != plan.plan().expected_event_id() {
            return Err(E::Corrupt);
        }
        if !status.push().artifact().admission_state().is_admitted() {
            let _coordinate = self.admit_coordinate(status.intent()).await?;
            self.sync()?
                .admit_signed(
                    radroots_sync::policy::SyncId::new(*status.receipt().operation_id().as_bytes())
                        .map_err(|_| E::Corrupt)?,
                )
                .await
                .map_err(Phase1DraftError::sync_error)?;
        }
        let current = self.submission_operation_status(request).await?;
        let projection = self
            .phase1_refresh_today(
                context,
                phase1_operation_now_unix_ms()? / 1000,
                TodayProjectionUpdate::Incremental,
            )
            .await
            .map_err(|_| Phase1DraftError::Overlay)?;
        let overlay = self
            .apply_submission_overlay(context, &current)
            .await
            .map_err(|_| Phase1DraftError::Overlay)?;
        let changed = status != current || projection.changed || overlay;
        Ok(SubmissionLocalReceipt {
            status: current,
            changed,
        })
    }
}
