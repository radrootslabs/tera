//! A local operation overlay never changes source event or remote acceptance.

use super::*;
#[cfg(feature = "mobile-social")]
use crate::runtime::product_surface::{SubmissionOperationStatus, outbox};

pub(super) fn sources(prior: Option<&TodayProjectionState>) -> BTreeMap<CardId, String> {
    prior
        .into_iter()
        .flat_map(|state| {
            state
                .cards
                .iter()
                .filter(|card| state.overlays.contains_key(&card.card.card_id.to_hex()))
        })
        .map(|card| (card.card.card_id, card.card.source_event_id.clone()))
        .collect()
}

pub(super) fn retain_sources(state: &mut TodayProjectionState, sources: &BTreeMap<CardId, String>) {
    let mut retained = BTreeMap::new();
    for card in &state.cards {
        let key = card.card.card_id.to_hex();
        if sources.get(&card.card.card_id) == Some(&card.card.source_event_id)
            && let Some(overlay) = state.overlays.remove(&key)
        {
            retained.insert(key, overlay);
        }
    }
    // A stable addressable card ID does not make the prior event's operation
    // receipt evidence for its replacement. Only derived overlays are removed.
    state.overlays = retained;
}

#[cfg(feature = "mobile-social")]
impl TeraRuntime {
    pub(in crate::runtime::product_surface) async fn apply_submission_overlay(
        &self,
        context: &LocalNetwork,
        status: &SubmissionOperationStatus,
    ) -> Result<bool, TodayError> {
        let _projection = self.today_projection_lock.lock().await;
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let generation = projection_generation()?;
        let mut state = load_state(storage, context, generation)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        if state.query_scope != Some(paging_scope::query_scope(context, self.store_public_key)?) {
            return Err(CursorError::Stale.into());
        }
        let artifact = status.push().artifact();
        let signed = artifact.signed().ok_or(TodayError::InvalidRequest)?;
        let plan = artifact
            .plan()
            .ok_or(TodayError::InvalidRequest)?
            .decode()?;
        let card_id = outbox::card_id(status.captured().form().input().command_type, plan.plan())
            .map_err(|_| TodayError::InvalidRequest)?;
        // A superseded addressable event or a hidden card cannot relabel the
        // winning visible event. Scope and author remain independently checked.
        if !state.cards.iter().any(|card| {
            card.card.card_id == card_id
                && card.card.source_event_id == signed.event().id().to_hex()
                && card.card.author_pubkey == status.receipt().request().scope().author().to_hex()
        }) {
            return Ok(false);
        }
        let key = card_id.to_hex();
        let overlay = LocalAuthorOverlay {
            operation_id: hex::encode(status.receipt().operation_id().as_bytes()),
            state: status.state().label().to_owned(),
        };
        if state.overlays.get(&key) == Some(&overlay) {
            return Ok(false);
        }
        state.overlays.insert(key, overlay);
        state.content_generation = content_generation(&state)?;
        store_state(self, storage, context, generation, &state).await?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::super::tests::{context, ingest, signed};
    use super::*;

    #[tokio::test]
    async fn replacement_does_not_inherit_prior_event_acceptance_overlay() {
        let runtime = TeraRuntime::test_memory().unwrap();
        let selected = context(None, 1);
        let tags = vec![
            vec!["d", "overlay-food"],
            vec!["title", "Carrots"],
            vec!["summary", "Fresh"],
            vec!["published_at", "2000000000"],
            vec!["location", "Victoria"],
            vec!["price", "3", "CAD"],
            vec!["radroots:price_unit", "lb"],
            vec!["status", "active"],
        ];
        let original = signed(30402, tags.clone(), "Original carrots", 2_000_000_000);
        ingest(&runtime, &selected, original, 2_000_000_010).await;
        let storage = runtime.client.storage().unwrap();
        let state = load_state(storage, &selected, projection_generation().unwrap())
            .await
            .unwrap()
            .unwrap();
        let card_id = state.cards[0].card.card_id;
        let overlay = LocalAuthorOverlay {
            operation_id: "original-operation".into(),
            state: "complete".into(),
        };
        runtime
            .phase1_set_local_author_overlay(&selected, card_id, Some(overlay.clone()))
            .await
            .unwrap();
        runtime
            .phase1_refresh_today(&selected, 2_000_000_011, TodayProjectionUpdate::Rebuild)
            .await
            .unwrap();
        let same = load_state(storage, &selected, projection_generation().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(same.overlays.get(&card_id.to_hex()), Some(&overlay));
        let replacement = signed(30402, tags, "Replacement carrots", 2_000_000_012);
        let replacement_id = replacement.id().to_hex();
        ingest(&runtime, &selected, replacement, 2_000_000_013).await;
        let replaced = load_state(storage, &selected, projection_generation().unwrap())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(replaced.cards.len(), 1);
        assert_eq!(replaced.cards[0].card.card_id, card_id);
        assert_eq!(replaced.cards[0].card.source_event_id, replacement_id);
        assert!(!replaced.overlays.contains_key(&card_id.to_hex()));
        assert_eq!(EventStore::status(storage).await.unwrap().raw_events(), 2);
        runtime.shutdown().await.unwrap();
    }
}
