//! Passive current revision facts and action availability; no new delivery authority.
use super::revision_delivery::meets_requirement;
use super::*;
use crate::runtime::product_surface::{
    PublicationDeliveryEvidence, PublicationDeliveryState, PublicationTargetDetails,
};

#[derive(Clone, Debug)]
pub struct Phase1RevisionBranchStatus {
    pub stopped: bool,
    pub can_resume: bool,
    pub can_cancel: bool,
    pub targets: Option<PublicationTargetDetails>,
    may_have_effect: bool,
    can_reconcile: bool,
}

impl Phase1RevisionStatus {
    pub fn replacement_progress(&self) -> &Phase1RevisionBranchStatus {
        &self.replacement_progress
    }
    pub fn retraction_progress(&self) -> Option<&Phase1RevisionBranchStatus> {
        self.retraction_progress.as_ref()
    }
    pub const fn can_resume(&self) -> bool {
        self.can_resume
    }
    pub const fn can_cancel(&self) -> bool {
        self.can_cancel
    }
}

pub(super) fn stopped(status: &Phase1DraftStatus) -> bool {
    status.draft().stage() == AuthoredDraftStage::Cancelled
        || status
            .push()
            .is_some_and(|push| push.delivery_plan().stop_requested_at_unix_ms().is_some())
}

impl TeraRuntime {
    /// Reconstructs the complete ordered revision from durable replacement and
    /// optional retraction child state after any process boundary.
    pub async fn phase1_revision_status(
        &self,
        replacement_draft_id: [u8; 16],
    ) -> Result<Phase1RevisionStatus, Phase1DraftError> {
        let _command = self.lifecycle.enter()?;
        // Bound nested native async frames while keeping lifecycle admission
        // in the caller for the complete passive reconstruction.
        Box::pin(self.revision_status_current(replacement_draft_id)).await
    }

    async fn revision_status_current(
        &self,
        replacement_draft_id: [u8; 16],
    ) -> Result<Phase1RevisionStatus, Phase1DraftError> {
        let replacement = self.phase1_draft_status(replacement_draft_id).await?;
        let payload = Phase1DraftPayload::decode(replacement.draft())?;
        let revision = payload.revision.ok_or(Phase1DraftError::InvalidRevision)?;
        let retraction = match revision.retraction_draft_id {
            Some(id) => match self.phase1_draft_status(id).await {
                Ok(status) => {
                    validate_revision_retraction(&status, &revision.target)?;
                    if Phase1DraftPayload::decode(status.draft())?
                        .revision_parent_draft_id
                        .is_some_and(|parent| parent != replacement_draft_id)
                    {
                        return Err(Phase1DraftError::Corrupt);
                    }
                    Some(status)
                }
                Err(Phase1DraftError::NotFound) => None,
                Err(error) => return Err(error),
            },
            None => None,
        };
        let replacement_progress = Box::pin(self.revision_branch_status(&replacement)).await?;
        let retraction_progress = match retraction.as_ref() {
            Some(child) => {
                let mut branch = Box::pin(self.revision_branch_status(child)).await?;
                branch.can_resume &= child.revision_parent_draft_id() == Some(replacement_draft_id);
                Some(branch)
            }
            None => None,
        };
        let child_eligible = !replacement_progress.stopped
            && replacement_progress
                .targets
                .as_ref()
                .is_some_and(|details| {
                    details.targets.iter().any(|target| {
                        meets_requirement(target, details.requires_delivery)
                            && !retraction_progress
                                .as_ref()
                                .and_then(|branch| branch.targets.as_ref())
                                .is_some_and(|child| {
                                    child.targets.iter().any(|other| {
                                        other.fingerprint == target.fingerprint
                                            && meets_requirement(other, child.requires_delivery)
                                    })
                                })
                    })
                });
        let can_resume = replacement_progress.can_resume
            || (revision.policy == Phase1RevisionPolicy::ReplaceThenRetract
                && !replacement_progress.stopped
                && (child_eligible
                    || retraction_progress
                        .as_ref()
                        .is_some_and(|branch| branch.can_reconcile))
                && retraction_progress
                    .as_ref()
                    .is_none_or(|branch| branch.can_resume));
        let can_cancel = replacement_progress.can_cancel
            || retraction_progress
                .as_ref()
                .is_some_and(|branch| branch.can_cancel)
            || (revision.policy == Phase1RevisionPolicy::ReplaceThenRetract
                && retraction.is_none()
                && !replacement_progress.stopped
                && replacement.state() == Phase1OutboxState::Complete);
        let phase = revision_phase(
            &replacement,
            &replacement_progress,
            retraction.as_ref(),
            retraction_progress.as_ref(),
            revision.policy,
        );
        Ok(Phase1RevisionStatus {
            replacement,
            retraction,
            target: revision.target,
            policy: revision.policy,
            phase,
            replacement_progress,
            retraction_progress,
            can_resume,
            can_cancel,
        })
    }

    async fn revision_branch_status(
        &self,
        status: &Phase1DraftStatus,
    ) -> Result<Phase1RevisionBranchStatus, Phase1DraftError> {
        let stopped = stopped(status);
        let targets = match status.push() {
            Some(push) => Some(
                PublicationTargetDetails::load(
                    push,
                    self.client
                        .storage()
                        .map_err(|_| Phase1DraftError::Storage)?,
                )
                .await,
            ),
            None => None,
        };
        // Pending facts reconcile locally before retry admission. A signed or
        // expired-claim child must not be hidden by its aggregate display phase.
        let can_reconcile = !stopped
            && status.push().is_some_and(|push| {
                push.artifact().admission_state().is_admitted()
                    && !push.delivery_plan().state().is_terminal()
                    && push
                        .delivery_plan()
                        .pending_delivery_facts()
                        .next()
                        .is_some()
            });
        let can_resume = !stopped
            && (can_reconcile
                || match status.draft().stage() {
                    AuthoredDraftStage::Draft | AuthoredDraftStage::ReadyToSign => true,
                    AuthoredDraftStage::Queued => status
                        .push()
                        .map(|push| {
                            self.publication_retry_at(push, phase1_operation_now_unix_ms()?)
                        })
                        .transpose()?
                        .is_some_and(|decision| decision.may_start()),
                    _ => false,
                });
        let can_cancel = !stopped
            && !matches!(
                status.state(),
                Phase1OutboxState::Complete | Phase1OutboxState::Terminal
            );
        let may_have_effect = status.push().is_some_and(|push| {
            PublicationDeliveryEvidence::from_push(push).state
                != PublicationDeliveryState::NotIssued
        }) || targets.as_ref().is_some_and(|details| {
            details.targets.iter().any(|target| {
                target.accepted
                    || target.delivered
                    || target.uncertain
                    || target.read_back_observed_at_unix_ms.is_some()
            })
        });
        Ok(Phase1RevisionBranchStatus {
            stopped,
            can_resume,
            can_cancel,
            targets,
            may_have_effect,
            can_reconcile,
        })
    }
}

fn all_targets(branch: &Phase1RevisionBranchStatus) -> bool {
    branch.targets.as_ref().is_some_and(|details| {
        !details.targets.is_empty()
            && details
                .targets
                .iter()
                .all(|target| meets_requirement(target, details.requires_delivery))
    })
}

fn revision_phase(
    replacement: &Phase1DraftStatus,
    parent: &Phase1RevisionBranchStatus,
    retraction: Option<&Phase1DraftStatus>,
    child: Option<&Phase1RevisionBranchStatus>,
    policy: Phase1RevisionPolicy,
) -> Phase1RevisionPhase {
    let replacement_complete =
        replacement.state() == Phase1OutboxState::Complete && all_targets(parent);
    if replacement_complete
        && (policy == Phase1RevisionPolicy::AddressableReplacement
            || (retraction.is_some_and(|status| status.state() == Phase1OutboxState::Complete)
                && child.is_some_and(all_targets)))
    {
        return Phase1RevisionPhase::Complete;
    }
    if replacement_complete
        && !parent.stopped
        && child.is_none_or(|branch| !branch.stopped && !branch.may_have_effect)
        && retraction.is_none_or(|status| status.state() != Phase1OutboxState::Terminal)
    {
        return Phase1RevisionPhase::RetractionPending;
    }
    if parent.may_have_effect || child.is_some_and(|branch| branch.may_have_effect) {
        return Phase1RevisionPhase::PartialEffect;
    }
    if parent.stopped {
        return Phase1RevisionPhase::Cancelled;
    }
    if replacement.state() == Phase1OutboxState::Terminal {
        return Phase1RevisionPhase::ReplacementFailed;
    }
    Phase1RevisionPhase::ReplacementPending
}
