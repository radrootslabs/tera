//! Lossless editing uses an exact stored source, never a displayed page scan.

use super::*;
use crate::runtime::product_surface::{
    SubmissionOperationStatus, recovery_inventory::RecoveryOwner,
};

impl TeraRuntime {
    /// Reads the original form only when this author's signed source matches
    /// the selected event. This performs no preparation, signing or delivery.
    pub async fn revision_source_form(
        &self,
        source_id: [u8; 16],
        target: &Phase1RevisionTarget,
    ) -> Result<Phase1DraftFormSnapshot, Phase1DraftError> {
        let _command = self.lifecycle.enter()?;
        target.validate()?;
        if target.author_public_key != hex::encode(self.draft_author()?) {
            return Err(Phase1DraftError::InvalidRevision);
        }
        let owner = self
            .recovery_parent(source_id)
            .await?
            .ok_or(Phase1DraftError::NotFound)?
            .owner;
        let (form, push) = match owner {
            RecoveryOwner::Legacy => {
                let source = self.phase1_draft_status(source_id).await?;
                if source.kind() != Phase1DraftKind::Add || source.card_id() != target.card_id {
                    return Err(Phase1DraftError::InvalidRevision);
                }
                (
                    source.form().cloned().ok_or(Phase1DraftError::NotFound)?,
                    source
                        .push()
                        .cloned()
                        .ok_or(Phase1DraftError::InvalidRevision)?,
                )
            }
            RecoveryOwner::Submission(request) => {
                let source = self
                    .submission_operation_status(&request)
                    .await
                    .map_err(|_| Phase1DraftError::Corrupt)?;
                (submission_form(&source)?, source.push().clone())
            }
            RecoveryOwner::Repair(_) => return Err(Phase1DraftError::Corrupt),
        };
        let artifact = push.artifact();
        let signed = artifact.signed().ok_or(Phase1DraftError::InvalidRevision)?;
        let plan = artifact
            .plan()
            .ok_or(Phase1DraftError::InvalidRevision)?
            .decode()
            .map_err(|_| Phase1DraftError::Corrupt)?;
        if signed.event().id().to_hex() != target.source_event_id
            || plan.plan().author().as_bytes() != &self.draft_author()?
            || plan.plan().body().kind() != target.source_kind
            || card_id(form.command_type, plan.plan())? != target.card_id
        {
            return Err(Phase1DraftError::InvalidRevision);
        }
        Ok(form)
    }
}

fn submission_form(
    source: &SubmissionOperationStatus,
) -> Result<Phase1DraftFormSnapshot, Phase1DraftError> {
    let input = source.captured().form().input();
    if input.media.len() != source.media().len() {
        return Err(Phase1DraftError::Corrupt);
    }
    let form = Phase1DraftFormSnapshot {
        command_type: input.command_type,
        content: input.content.clone(),
        identifier: input.identifier.clone(),
        title: input.title.clone(),
        summary: input.summary.clone(),
        location: input.location.clone(),
        event_timing: input.event_timing,
        event_start_date: input.event_start_date.clone(),
        event_end_date: input.event_end_date.clone(),
        event_start_unix_s: input.event_start_unix_s,
        event_end_unix_s: input.event_end_unix_s,
        event_timezone: input.event_timezone.clone(),
        price_amount: input.price_amount.clone(),
        currency: input.currency.clone(),
        unit: input.unit.clone(),
        quantity: input.quantity.clone(),
        food_published_at_unix_s: input.food_published_at_unix_s,
        food_status: input.food_status.clone(),
        media: input
            .media
            .iter()
            .zip(source.media())
            .map(|(input, media)| Phase1DraftMediaSnapshot {
                opaque_reference: input.opaque_reference.clone(),
                url: media.url().to_owned(),
                sha256: input.sha256.clone(),
                media_type: input.media_type.clone(),
                byte_size: input.byte_size,
                width: input.width,
                height: input.height,
                alt: input.alt.clone(),
                prepared_at_unix_s: input.prepared_at_unix_s,
            })
            .collect(),
    };
    form.validate(input.command_type, source.media())?;
    Ok(form)
}
