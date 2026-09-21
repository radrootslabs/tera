//! Current claim, immutable capture and canonical raw event-head checks.

use super::{record::OwnershipRecord, *};
use crate::TeraRuntime;
use radroots_storage::authored_draft::AuthoredDraftRevision;

impl TeraRuntime {
    pub(super) async fn coordinate_binding(
        &self,
        intent: &CoordinateIntent,
    ) -> Result<Option<OwnershipRecord>, E> {
        let store = self.client.storage().map_err(|_| E::Storage)?;
        let Some(binding) = store
            .authored_draft_head(intent.binding_id()?)
            .await
            .map_err(|_| E::Storage)?
        else {
            return Ok(None);
        };
        let record = OwnershipRecord::decode(&binding)?;
        if record.intent != *intent {
            return Err(E::RevisionConflict);
        }
        self.coordinate_validate_capture(&record).await?;
        Ok(Some(record))
    }

    pub(super) async fn coordinate_validate_capture(
        &self,
        record: &OwnershipRecord,
    ) -> Result<(), E> {
        let store = self.client.storage().map_err(|_| E::Storage)?;
        let captured = store
            .authored_draft_revision(
                AuthoredDraftId::new(record.intent.draft_id).map_err(|_| E::Corrupt)?,
                AuthoredDraftRevision::new(record.captured_revision).map_err(|_| E::Corrupt)?,
            )
            .await
            .map_err(|_| E::Storage)?
            .ok_or(E::Corrupt)?;
        if Sha256::digest(captured.payload()).as_slice() != record.captured_sha256
            || intent_from_draft(&captured)?.as_ref() != Some(&record.intent)
        {
            return Err(E::Corrupt);
        }
        Ok(())
    }

    pub(in crate::runtime::product_surface) async fn coordinate_is_current(
        &self,
        intent: &CoordinateIntent,
    ) -> Result<bool, E> {
        self.coordinate_is_current_at(
            intent,
            super::super::phase1_operation_now_unix_ms()? / 1_000,
        )
        .await
    }

    async fn coordinate_is_current_at(
        &self,
        intent: &CoordinateIntent,
        now_unix_s: u64,
    ) -> Result<bool, E> {
        let Some(binding) = self.coordinate_binding(intent).await? else {
            return Ok(false);
        };
        let store = self.client.storage().map_err(|_| E::Storage)?;
        let claim = store
            .authored_draft_head(intent.claim_id()?)
            .await
            .map_err(|_| E::Storage)?
            .ok_or(E::Corrupt)?;
        let current = OwnershipRecord::decode(&claim)?;
        Ok(current == binding
            && self
                .coordinate_known_winner_matches_at(intent, now_unix_s)
                .await?)
    }

    pub(in crate::runtime::product_surface) async fn require_coordinate_current(
        &self,
        intent: &CoordinateIntent,
    ) -> Result<(), E> {
        if !self.coordinate_is_current(intent).await? {
            return Err(E::RevisionConflict);
        }
        Ok(())
    }

    pub(in crate::runtime::product_surface) async fn require_coordinate_current_at(
        &self,
        intent: &CoordinateIntent,
        now_unix_s: u64,
    ) -> Result<(), E> {
        if !self.coordinate_is_current_at(intent, now_unix_s).await? {
            return Err(E::RevisionConflict);
        }
        Ok(())
    }

    pub(in crate::runtime::product_surface) async fn coordinate_may_resume(
        &self,
        head: &AuthoredDraft,
    ) -> Result<bool, E> {
        // Keep the bounded storage reconstruction off the native caller's
        // nested async frame. This remains caller-owned and cancellation-safe.
        Box::pin(self.coordinate_may_resume_current(head)).await
    }

    async fn coordinate_may_resume_current(&self, head: &AuthoredDraft) -> Result<bool, E> {
        let Some(intent) = intent_from_draft(head)? else {
            return Ok(true);
        };
        if self.coordinate_binding(&intent).await?.is_some() {
            return self.coordinate_is_current(&intent).await;
        }
        if !self.coordinate_known_winner_matches(&intent).await? {
            return Ok(false);
        }
        let store = self.client.storage().map_err(|_| E::Storage)?;
        match store
            .authored_draft_head(intent.claim_id()?)
            .await
            .map_err(|_| E::Storage)?
        {
            Some(claim) => {
                if OwnershipRecord::decode(&claim)?.intent.coordinate()? != intent.coordinate()? {
                    return Err(E::Corrupt);
                }
                self.coordinate_previous_retirable(&claim).await
            }
            None => Ok(true),
        }
    }

    pub(in crate::runtime::product_surface) async fn require_draft_coordinate_current(
        &self,
        head: &AuthoredDraft,
    ) -> Result<(), E> {
        if let Some(intent) = intent_from_draft(head)? {
            self.require_coordinate_current(&intent).await?;
        }
        Ok(())
    }

    pub(in crate::runtime::product_surface) async fn draft_has_coordinate_binding(
        &self,
        head: &AuthoredDraft,
    ) -> Result<bool, E> {
        let Some(intent) = intent_from_draft(head)? else {
            return Ok(false);
        };
        Ok(self.coordinate_binding(&intent).await?.is_some())
    }

    pub(super) async fn coordinate_previous_retirable(
        &self,
        claim: &AuthoredDraft,
    ) -> Result<bool, E> {
        let record = OwnershipRecord::decode(claim)?;
        if self.coordinate_binding(&record.intent).await?.as_ref() != Some(&record) {
            return Err(E::Corrupt);
        }
        let store = self.client.storage().map_err(|_| E::Storage)?;
        let head = store
            .authored_draft_head(
                AuthoredDraftId::new(record.intent.draft_id).map_err(|_| E::Corrupt)?,
            )
            .await
            .map_err(|_| E::Storage)?
            .ok_or(E::Corrupt)?;
        if intent_from_draft(&head)?.as_ref() != Some(&record.intent) {
            return Err(E::Corrupt);
        }
        match head.payload_schema() {
            super::super::outbox::DRAFT_PAYLOAD_SCHEMA => {
                self.legacy_coordinate_retirable(&head).await
            }
            super::super::SUBMISSION_INTENT_PAYLOAD_SCHEMA => {
                self.submission_coordinate_retirable(&head).await
            }
            _ => Err(E::Corrupt),
        }
    }
}
