//! One current claim and one permanent request binding in a bounded atomic pair.

use super::{record::OwnershipRecord, *};
use crate::{TeraRuntime, runtime::mutation_admission::MutationPermit};
use radroots_storage::{
    authored_draft::{AuthoredDraftRevision, AuthoredDraftStage},
    authored_draft_pair::AuthoredDraftPair,
};

impl TeraRuntime {
    pub(in crate::runtime::product_surface) async fn admit_coordinate<'a>(
        &'a self,
        head: &AuthoredDraft,
    ) -> Result<Option<MutationPermit<'a>>, E> {
        // Preserve the default host stack budget across nested signing/queue
        // entrypoints without detaching ownership from the caller's future.
        Box::pin(self.admit_coordinate_current(head)).await
    }

    async fn admit_coordinate_current<'a>(
        &'a self,
        head: &AuthoredDraft,
    ) -> Result<Option<MutationPermit<'a>>, E> {
        let Some(intent) = intent_from_draft(head)? else {
            return Ok(None);
        };
        if self.store_public_key.as_ref().map(|key| key.as_bytes()) != Some(&intent.author) {
            return Err(E::IdentityUnavailable);
        }
        let permit = self.mutations.coordinate(intent.scope_key())?;
        self.coordinate_install_binding(head, &intent).await?;
        self.require_coordinate_current(&intent).await?;
        Ok(Some(permit))
    }

    async fn coordinate_install_binding(
        &self,
        head: &AuthoredDraft,
        intent: &CoordinateIntent,
    ) -> Result<(), E> {
        if self.coordinate_binding(intent).await?.is_some() {
            // A prior request never reacquires a later claim generation.
            return self.require_coordinate_current(intent).await;
        }
        if head.stage() == AuthoredDraftStage::Cancelled
            || !self.coordinate_known_winner_matches(intent).await?
        {
            return Err(E::RevisionConflict);
        }
        let store = self.client.storage().map_err(|_| E::Storage)?;
        let current_head = store
            .authored_draft_head(head.draft_id())
            .await
            .map_err(|_| E::Storage)?
            .ok_or(E::Corrupt)?;
        if current_head != *head {
            return Err(E::RevisionConflict);
        }
        let previous = store
            .authored_draft_head(intent.claim_id()?)
            .await
            .map_err(|_| E::Storage)?;
        if let Some(previous) = &previous {
            let record = OwnershipRecord::decode(previous)?;
            if record.intent.coordinate()? != intent.coordinate()? {
                return Err(E::Corrupt);
            }
            if !self.coordinate_previous_retirable(previous).await? {
                return Err(E::RevisionConflict);
            }
        }
        let revision = previous
            .as_ref()
            .map(|row| row.revision().next())
            .transpose()
            .map_err(|_| E::Corrupt)?
            .unwrap_or(AuthoredDraftRevision::INITIAL);
        let record = OwnershipRecord {
            intent: intent.clone(),
            captured_revision: head.revision().get(),
            captured_sha256: Sha256::digest(head.payload()).into(),
            claim_revision: revision.get(),
        };
        let now = super::super::phase1_operation_now_unix_ms()?
            .max(head.updated_at_unix_ms())
            .max(previous.as_ref().map_or(0, |row| row.updated_at_unix_ms()));
        let claim = match &previous {
            Some(previous) => {
                previous.successor(record.encode()?, AuthoredDraftStage::Draft, None, now)
            }
            None => AuthoredDraft::initial(
                intent.claim_id()?,
                intent.author,
                CLAIM_SCHEMA,
                record.encode()?,
                AuthoredDraftStage::Draft,
                None,
                now,
            ),
        }
        .map_err(|_| E::Corrupt)?;
        let binding = AuthoredDraft::initial(
            intent.binding_id()?,
            intent.author,
            BINDING_SCHEMA,
            record.encode()?,
            AuthoredDraftStage::Draft,
            None,
            now,
        )
        .map_err(|_| E::Corrupt)?;
        OwnershipRecord::decode(&claim)?;
        OwnershipRecord::decode(&binding)?;
        let pair = AuthoredDraftPair::new(
            claim.clone(),
            previous.as_ref().map(|row| row.revision()),
            binding.clone(),
            None,
        )
        .map_err(|_| E::Corrupt)?;
        let receipts =
            store
                .append_authored_draft_pair(pair)
                .await
                .map_err(|error| match error {
                    radroots_storage::Error::DraftRevisionConflict => E::RevisionConflict,
                    _ => E::Storage,
                })?;
        if receipts[0].draft() != &claim || receipts[1].draft() != &binding {
            return Err(E::Corrupt);
        }
        let retained = store
            .authored_draft_head(head.draft_id())
            .await
            .map_err(|_| E::Storage)?
            .ok_or(E::Corrupt)?;
        if intent_from_draft(&retained)?.as_ref() != Some(intent) {
            return Err(E::RevisionConflict);
        }
        Ok(())
    }
}
