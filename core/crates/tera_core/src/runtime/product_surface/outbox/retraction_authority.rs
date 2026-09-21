//! Exact signed source authority is independent of native cards and visibility.

use super::*;
use crate::runtime::product_surface::projection::{admitted_card_source, admitted_card_type};
use radroots_event::EventId;
use radroots_event_codec::{admission::admit_verified_event, verify::verify_nip01_event};
use radroots_storage::event::{EventQuery, EventQueryBounds};

impl TeraRuntime {
    pub(super) async fn require_revision_source(
        &self,
        target: &Phase1RevisionTarget,
    ) -> Result<(), Phase1DraftError> {
        // Bound the frame while retaining this read in the caller's lifetime.
        Box::pin(self.check_revision_source(target)).await
    }

    async fn check_revision_source(
        &self,
        target: &Phase1RevisionTarget,
    ) -> Result<(), Phase1DraftError> {
        target.validate()?;
        if target.author_public_key != hex::encode(self.draft_author()?) {
            return Err(Phase1DraftError::InvalidRevision);
        }
        let id = EventId::parse(&target.source_event_id)
            .map_err(|_| Phase1DraftError::InvalidRevision)?;
        let query = EventQuery::for_ids(
            EventQueryBounds::first(1).map_err(|_| Phase1DraftError::InvalidRevision)?,
            vec![id],
        )
        .map_err(|_| Phase1DraftError::InvalidRevision)?;
        let page = self
            .client
            .storage()
            .map_err(|_| Phase1DraftError::Storage)?
            .query_verified(query)
            .await
            .map_err(|_| Phase1DraftError::Storage)?;
        let [stored] = page.items() else {
            return Err(Phase1DraftError::InvalidRevision);
        };
        // Stored admission typestate is not a substitute for actual signature
        // evidence, and suppressed sources remain eligible exact-ID evidence.
        let verified = verify_nip01_event(stored.event().envelope().clone())
            .map_err(|_| Phase1DraftError::InvalidRevision)?;
        let admitted =
            admit_verified_event(verified).map_err(|_| Phase1DraftError::InvalidRevision)?;
        let event = admitted.event();
        let kind = admitted_card_type(&admitted).map_err(|_| Phase1DraftError::InvalidRevision)?;
        let source =
            admitted_card_source(&admitted, kind).map_err(|_| Phase1DraftError::InvalidRevision)?;
        let address = match &source {
            CardSourceIdentity::Event(_) => None,
            CardSourceIdentity::Address {
                kind,
                author_pubkey,
                identifier,
            } => Some(format!("{kind}:{author_pubkey}:{identifier}")),
        };
        if event.id() != &id
            || event.author().to_hex() != target.author_public_key
            || event.kind_u32() != target.source_kind
            || address != target.source_address
            || CardId::derive(kind, &source) != target.card_id
        {
            return Err(Phase1DraftError::InvalidRevision);
        }
        Ok(())
    }

    pub(super) async fn require_publication_source(
        &self,
        plan: &AuthoredEventPlan,
        owner: Option<&AuthoredDraft>,
    ) -> Result<(), Phase1DraftError> {
        if plan.author().as_bytes() != &self.draft_author()? {
            return Err(Phase1DraftError::InvalidRevision);
        }
        let payload = match owner {
            Some(head) if head.payload_schema() == DRAFT_PAYLOAD_SCHEMA => {
                let payload = Phase1DraftPayload::decode(head)?;
                let frozen = PlanWireV1::from_json(&payload.plan_wire_json)
                    .map_err(|_| Phase1DraftError::Corrupt)?;
                if frozen.plan() != plan || head.author() != plan.author().as_bytes() {
                    return Err(Phase1DraftError::InvalidRevision);
                }
                Some(payload)
            }
            _ => None,
        };
        if plan.body().kind() == 5 {
            let payload = payload.ok_or(Phase1DraftError::InvalidRevision)?;
            let target = retraction_target(&payload, plan)?;
            self.require_revision_source(&target).await?;
        } else if let Some(revision) = payload.and_then(|payload| payload.revision)
            && revision.policy == Phase1RevisionPolicy::ReplaceThenRetract
        {
            self.require_revision_source(&revision.target).await?;
        }
        Ok(())
    }
}

fn retraction_target(
    payload: &Phase1DraftPayload,
    plan: &AuthoredEventPlan,
) -> Result<Phase1RevisionTarget, Phase1DraftError> {
    if payload.kind != Phase1DraftKind::Retraction {
        return Err(Phase1DraftError::InvalidRevision);
    }
    let tags = plan.body().tags();
    let value = |name: &str| -> Result<Option<&str>, Phase1DraftError> {
        let mut matching = tags
            .iter()
            .filter(|tag| tag.first().map(String::as_str) == Some(name));
        let first = matching.next();
        if matching.next().is_some() || first.is_some_and(|tag| tag.len() != 2) {
            return Err(Phase1DraftError::InvalidRevision);
        }
        Ok(first.and_then(|tag| tag.get(1)).map(String::as_str))
    };
    let event = value("e")?.ok_or(Phase1DraftError::InvalidRevision)?;
    let kind = value("k")?
        .ok_or(Phase1DraftError::InvalidRevision)?
        .parse()
        .map_err(|_| Phase1DraftError::InvalidRevision)?;
    let address = value("a")?;
    let target = Phase1RevisionTarget::new(
        payload.command_type,
        payload
            .target_card_id
            .ok_or(Phase1DraftError::InvalidRevision)?,
        event,
        kind,
        address.map(str::to_owned),
        plan.author().to_hex(),
    )?;
    let request = AuthoredNip09DeletionRequest::new(
        plan.body().content(),
        vec![
            Nip09DeletionEventTarget::parse(event, kind)
                .map_err(|_| Phase1DraftError::InvalidRevision)?,
        ],
        address
            .map(Nip09DeletionAddressTarget::parse)
            .transpose()
            .map_err(|_| Phase1DraftError::InvalidRevision)?
            .into_iter()
            .collect(),
    )
    .map_err(|_| Phase1DraftError::InvalidRevision)?;
    let canonical = phase1_retraction_plan(&request, plan.created_at(), plan.author().to_hex())
        .map_err(|_| Phase1DraftError::InvalidRevision)?;
    if canonical != *plan {
        return Err(Phase1DraftError::InvalidRevision);
    }
    Ok(target)
}
