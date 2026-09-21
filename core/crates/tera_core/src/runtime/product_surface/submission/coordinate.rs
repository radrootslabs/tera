//! Scoped submissions keep their immutable PrepareFromDraft ownership.

use super::intent::IntentPayload;
use crate::{
    TeraRuntime,
    runtime::product_surface::{Phase1DraftError as E, coordinate::CoordinatePlan},
};
use radroots_storage::authored_draft::AuthoredDraft;

pub(in crate::runtime::product_surface) fn coordinate_plan(
    head: &AuthoredDraft,
) -> Result<Option<CoordinatePlan>, E> {
    IntentPayload::coordinate_plan(head).map_err(|_| E::Corrupt)
}

impl TeraRuntime {
    pub(in crate::runtime::product_surface) async fn submission_coordinate_retirable(
        &self,
        head: &AuthoredDraft,
    ) -> Result<bool, E> {
        let store = self.client.storage().map_err(|_| E::Storage)?;
        let request = super::recovery_request(head, *head.author(), store)
            .await
            .map_err(|_| E::Corrupt)?;
        let (loaded, push) = self
            .load_submission_operation(&request)
            .await
            .map_err(|_| E::Corrupt)?;
        if loaded.head != *head {
            return Err(E::RevisionConflict);
        }
        Ok(push.delivery_plan().stop_requested_at_unix_ms().is_some()
            || push.delivery_plan().state().is_terminal())
    }
}
