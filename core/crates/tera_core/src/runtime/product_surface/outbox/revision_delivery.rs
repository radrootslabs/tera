//! Revision children retain the full frozen destination policy and admit only
//! targets with exact replacement evidence. No aggregate state grants admission.

use super::*;
use crate::runtime::{
    mutation_admission::MutationPermit,
    product_surface::publication_targets::{PublicationTargetDetails, PublicationTargetEvidence},
};

pub(super) enum RevisionDelivery {
    Independent,
    Held,
    Selected(TargetSet),
}

impl Phase1DraftPayload {
    pub(super) fn revision_parent(&self) -> Result<Option<[u8; 16]>, Phase1DraftError> {
        if let Some(parent) = self.revision_parent_draft_id {
            return Ok(Some(parent));
        }
        // Old revision children have no exact reverse key. Their recognizable
        // reserved reason is held, never rebound to a guessed parent or policy.
        if self.kind == Phase1DraftKind::Retraction {
            let plan = PlanWireV1::from_json(&self.plan_wire_json)
                .map_err(|_| Phase1DraftError::Corrupt)?;
            if plan.plan().body().content() == REVISION_RETRACTION_REASON {
                return Err(Phase1DraftError::InvalidRevision);
            }
        }
        Ok(None)
    }
}

impl TeraRuntime {
    pub(super) async fn revision_has_replacement_evidence(
        &self,
        replacement: &Phase1DraftStatus,
    ) -> Result<bool, Phase1DraftError> {
        let Some(push) = replacement.push() else {
            return Ok(false);
        };
        if replacement.draft().stage() == AuthoredDraftStage::Cancelled
            || push.delivery_plan().stop_requested_at_unix_ms().is_some()
        {
            return Ok(false);
        }
        let storage = self
            .client
            .storage()
            .map_err(|_| Phase1DraftError::Storage)?;
        let evidence = PublicationTargetDetails::load(push, storage).await;
        Ok(evidence
            .targets
            .iter()
            .any(|target| meets_requirement(target, evidence.requires_delivery)))
    }

    pub(super) fn revision_parent_admission(
        &self,
        head: &AuthoredDraft,
    ) -> Result<Option<MutationPermit<'_>>, Phase1DraftError> {
        Phase1DraftPayload::decode(head)?
            .revision_parent()?
            .map(|parent| self.mutations.draft(parent))
            .transpose()
    }

    /// Validates the exact relation independently of current relay configuration.
    async fn revision_child_parent(
        &self,
        child: &AuthoredDraft,
    ) -> Result<Option<Phase1DraftStatus>, Phase1DraftError> {
        let payload = Phase1DraftPayload::decode(child)?;
        let Some(parent_id) = payload.revision_parent()? else {
            return Ok(None);
        };
        if parent_id == *child.draft_id().as_bytes() {
            return Err(Phase1DraftError::Corrupt);
        }
        let parent = self.phase1_draft_status(parent_id).await?;
        let parent_payload = Phase1DraftPayload::decode(parent.draft())?;
        let revision = parent_payload.revision.ok_or(Phase1DraftError::Corrupt)?;
        if revision.policy != Phase1RevisionPolicy::ReplaceThenRetract
            || revision.retraction_draft_id != Some(*child.draft_id().as_bytes())
            || child.author() != parent.draft().author()
            || revision.target.author_public_key != hex::encode(parent.draft().author())
        {
            return Err(Phase1DraftError::Corrupt);
        }
        validate_revision_retraction(
            &self.draft_status_from(child.clone()).await?,
            &revision.target,
        )?;
        if parent.draft().stage() == AuthoredDraftStage::Cancelled
            || parent
                .push()
                .is_some_and(|push| push.delivery_plan().stop_requested_at_unix_ms().is_some())
        {
            return Err(Phase1DraftError::Terminal);
        }
        Ok(Some(parent))
    }

    pub(super) async fn revision_child_queue_policy(
        &self,
        child: &AuthoredDraft,
    ) -> Result<Option<Phase1QueuePolicy>, Phase1DraftError> {
        let Some(parent) = Box::pin(self.revision_child_parent(child)).await? else {
            return Ok(None);
        };
        Phase1DraftPayload::decode(parent.draft())?
            .queue
            .map(Some)
            .ok_or(Phase1DraftError::InvalidRevision)
    }

    /// Called with parent mutation admission retained across signing/delivery.
    pub(super) async fn revision_delivery_selection(
        &self,
        child: &AuthoredDraft,
    ) -> Result<RevisionDelivery, Phase1DraftError> {
        let Some(parent) = Box::pin(self.revision_child_parent(child)).await? else {
            return Ok(RevisionDelivery::Independent);
        };
        let frozen = Phase1DraftPayload::decode(parent.draft())?
            .queue
            .ok_or(Phase1DraftError::InvalidRevision)?;
        if Phase1DraftPayload::decode(child)?.queue.as_ref() != Some(&frozen) {
            return Err(Phase1DraftError::InvalidRevision);
        }
        let Some(parent_push) = parent.push() else {
            return Ok(RevisionDelivery::Held);
        };
        let storage = self
            .client
            .storage()
            .map_err(|_| Phase1DraftError::Storage)?;
        let replacement = PublicationTargetDetails::load(parent_push, storage).await;
        let child_details = match self.push_status_for(child).await? {
            Some(push) => Some(PublicationTargetDetails::load(&push, storage).await),
            None => None,
        };
        let (targets, _, _) = frozen.materialize()?;
        let eligible = targets
            .targets()
            .iter()
            .filter(|target| {
                let fingerprint = target.fingerprint().as_str();
                replacement.targets.iter().any(|evidence| {
                    evidence.fingerprint == fingerprint
                        && meets_requirement(evidence, replacement.requires_delivery)
                }) && !child_details.as_ref().is_some_and(|details| {
                    details.targets.iter().any(|evidence| {
                        evidence.fingerprint == fingerprint
                            && meets_requirement(evidence, details.requires_delivery)
                    })
                })
            })
            .cloned()
            .collect::<Vec<_>>();
        if eligible.is_empty() {
            Ok(RevisionDelivery::Held)
        } else {
            TargetSet::new(eligible)
                .map(RevisionDelivery::Selected)
                .map_err(|_| Phase1DraftError::Corrupt)
        }
    }

    pub(super) async fn advance_revision_child_request(
        &self,
        head: &AuthoredDraft,
        request: PushRequest,
    ) -> Result<(), Phase1DraftError> {
        match self.revision_delivery_selection(head).await? {
            RevisionDelivery::Independent => self.advance_push_request(request).await,
            RevisionDelivery::Selected(targets) => {
                self.advance_push_request_selected(request, targets).await
            }
            RevisionDelivery::Held => {
                // Reconciliation cannot initiate another effect in this call.
                // Preserve it without claiming/consuming a held delivery attempt.
                let push = self
                    .push_status_for(head)
                    .await?
                    .ok_or(Phase1DraftError::Corrupt)?;
                if push
                    .delivery_plan()
                    .pending_delivery_facts()
                    .next()
                    .is_some()
                {
                    self.sync()?
                        .deliver_push(request.operation_id())
                        .await
                        .map_err(|_| Phase1DraftError::Operation)?;
                }
                Ok(())
            }
        }
    }
}

pub(super) fn meets_requirement(
    evidence: &PublicationTargetEvidence,
    requires_delivery: bool,
) -> bool {
    evidence.read_back_observed_at_unix_ms.is_some()
        || evidence.delivered
        || (!requires_delivery && evidence.accepted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replacement_requirement_distinguishes_acceptance_delivery_and_positive_read_back() {
        let mut evidence = PublicationTargetEvidence {
            fingerprint: "one".into(),
            endpoint: "wss://one.example".into(),
            attempted: true,
            accepted: true,
            delivered: false,
            rejected: false,
            uncertain: true,
            read_back_observed_at_unix_ms: None,
        };
        assert!(meets_requirement(&evidence, false));
        assert!(!meets_requirement(&evidence, true));
        evidence.delivered = true;
        assert!(meets_requirement(&evidence, true));
        evidence.delivered = false;
        evidence.accepted = false;
        assert!(!meets_requirement(&evidence, false));
        evidence.read_back_observed_at_unix_ms = Some(1);
        assert!(meets_requirement(&evidence, false));
        assert!(meets_requirement(&evidence, true));
    }
}
