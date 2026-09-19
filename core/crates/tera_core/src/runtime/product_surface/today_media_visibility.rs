use super::*;

#[cfg(all(test, feature = "mobile-social"))]
#[path = "today_media_visibility_tests.rs"]
mod tests;

/// A cached reference is not authority after the selected query or visible
/// event set changes. The owner re-evaluates replacement/deletion semantics;
/// raw row counts and the store's identity generation cannot establish this.
pub(super) async fn current_state(
    runtime: &TeraRuntime,
    context: &LocalNetwork,
) -> Result<TodayProjectionState, TodayError> {
    let scope = paging_scope::query_scope(context, runtime.store_public_key)?;
    let storage = runtime
        .client
        .storage()
        .map_err(|_| TodayError::RuntimeUnavailable)?;
    let state = load_state(storage, context, projection_generation()?)
        .await?
        .ok_or(TodayError::ProjectionMissing)?;
    if state.query_scope != Some(scope) {
        return Err(TodayError::InvalidRequest);
    }
    let visibility = EventStore::rebuild_visibility(storage).await?;
    if state.visibility_digest != Some(*visibility.digest().as_bytes()) {
        return Err(TodayError::InvalidRequest);
    }
    Ok(state)
}
