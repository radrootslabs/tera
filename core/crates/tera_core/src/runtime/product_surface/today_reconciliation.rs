use std::collections::BTreeSet;

use super::*;

/// Current visible values for a bounded set of already displayed identities.
/// Missing identities are removals, not an invitation to retain an old value.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayReconciliation {
    pub projection_generation: u64,
    pub items: Vec<TodayCard>,
}

impl TeraRuntime {
    pub async fn phase1_today_reconcile(
        &self,
        context: &LocalNetwork,
        as_of: u64,
        card_ids: &[String],
        expected_generation: Option<u64>,
    ) -> Result<TodayReconciliation, TodayError> {
        let _command = self.lifecycle.enter()?;
        if as_of == 0 || card_ids.len() > usize::from(TODAY_PAGE_LIMIT_MAX) {
            return Err(TodayError::InvalidRequest);
        }
        let selected = card_ids
            .iter()
            .map(|id| CardId::parse(id).map_err(|_| TodayError::InvalidRequest))
            .collect::<Result<BTreeSet<_>, _>>()?;
        if selected.len() != card_ids.len() {
            return Err(TodayError::InvalidRequest);
        }
        let query_scope = paging_scope::query_scope(context, self.store_public_key)?;
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let state = load_state(storage, context, projection_generation()?)
            .await?
            .ok_or(TodayError::ProjectionMissing)?;
        if state.query_scope != Some(query_scope)
            || state.visibility_digest.is_none()
            || expected_generation.is_some_and(|value| value != state.content_generation)
            || state.store_generation != *EventStore::status(storage).await?.generation().as_bytes()
        {
            return Err(CursorError::Stale.into());
        }
        Ok(TodayReconciliation {
            projection_generation: state.content_generation,
            items: selected_ranked_cards(&state, context, as_of, Some(&selected))?,
        })
    }
}
