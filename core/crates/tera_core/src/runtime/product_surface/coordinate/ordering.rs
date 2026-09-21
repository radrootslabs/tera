//! Frozen plans consume shared Nostr ordering before acquiring effect authority.

use super::*;
use crate::TeraRuntime;
use radroots_event::{
    EventId,
    envelope::event_head::{EventHeadCandidate, EventHeadDecision, select_event_head},
};
use radroots_storage::event::EventStore;

impl TeraRuntime {
    pub(in crate::runtime::product_surface) async fn coordinate_known_winner_matches(
        &self,
        intent: &CoordinateIntent,
    ) -> Result<bool, E> {
        self.coordinate_known_winner_matches_at(
            intent,
            super::super::phase1_operation_now_unix_ms()? / 1_000,
        )
        .await
    }

    pub(in crate::runtime::product_surface) async fn coordinate_known_winner_matches_at(
        &self,
        intent: &CoordinateIntent,
        now_unix_s: u64,
    ) -> Result<bool, E> {
        let store = self.client.storage().map_err(|_| E::Storage)?;
        let draft = store
            .authored_draft_head(AuthoredDraftId::new(intent.draft_id).map_err(|_| E::Corrupt)?)
            .await
            .map_err(|_| E::Storage)?
            .ok_or(E::Corrupt)?;
        let plan = plan_from_draft(&draft)?.ok_or(E::Corrupt)?;
        if plan.intent != *intent {
            return Err(E::RevisionConflict);
        }
        // The bounded policy permits zero synthetic advancement and no future
        // captured timestamp relative to the current clock. Never repair clock
        // rollback by changing a saved preimage. Existing delivery deadlines
        // separately bound attempts after a forward jump.
        if plan.created_at > now_unix_s {
            return Ok(false);
        }
        let coordinate = intent.coordinate()?;
        let snapshot = EventStore::rebuild_visibility(store)
            .await
            .map_err(|_| E::Storage)?;
        let current = snapshot
            .current_heads()
            .iter()
            .find(|head| head.coordinate == coordinate);
        let selected = match current {
            Some(head) => {
                head.event_id.as_bytes() == &intent.event_id
                    || intent.prior_event_id.as_ref() == Some(head.event_id.as_bytes())
            }
            None => intent.prior_event_id.is_none(),
        };
        if !selected {
            return Ok(false);
        }
        let candidate = EventHeadCandidate {
            coordinate,
            event_id: EventId::parse(hex::encode(intent.event_id)).map_err(|_| E::Corrupt)?,
            created_at: plan.created_at,
        };
        Ok(matches!(
            select_event_head(candidate, current),
            EventHeadDecision::Applied(_) | EventHeadDecision::SkippedDuplicate
        ))
    }
}
