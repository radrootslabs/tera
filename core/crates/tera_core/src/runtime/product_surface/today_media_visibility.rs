use super::*;

#[cfg(all(test, feature = "mobile-social"))]
#[path = "today_media_visibility_tests.rs"]
mod tests;

/// Media has no calendar selection. It shares the same complete source,
/// policy, full context, owner, promoted generation and local-evidence scope.
pub(super) async fn current_state(
    runtime: &TeraRuntime,
    context: &LocalNetwork,
) -> Result<TodayProjectionState, TodayError> {
    let mut budget = page_budget::Budget::default();
    let scope = current_scope(runtime, context, &mut budget).await?;
    let mut finish = budget.reserve_scope()?;
    let state = scope.load_state(runtime, context).await?;
    scope.check_current(runtime, context, &mut finish).await?;
    Ok(state)
}

/// After an acknowledged own receipt/LRU write, compare its resulting revision
/// rather than accidentally rejecting the write's intentional local change.
pub(super) async fn confirm_state(
    runtime: &TeraRuntime,
    context: &LocalNetwork,
    state: &TodayProjectionState,
) -> Result<(), TodayError> {
    let mut budget = page_budget::Budget::default();
    let scope = current_scope(runtime, context, &mut budget).await?;
    scope.check_state(state)?;
    let mut finish = budget.reserve_scope()?;
    scope.check_current(runtime, context, &mut finish).await
}

async fn current_scope(
    runtime: &TeraRuntime,
    context: &LocalNetwork,
    budget: &mut page_budget::Budget,
) -> Result<read_scope::Scope, TodayError> {
    if let Some(scope) = read_scope::Scope::load(runtime, context, budget).await? {
        return Ok(scope);
    }
    let storage = runtime
        .client
        .storage()
        .map_err(|_| TodayError::RuntimeUnavailable)?;
    if sidecar::load(storage, context).await?.is_some() {
        Err(TodayError::InvalidRequest)
    } else {
        Err(TodayError::ProjectionMissing)
    }
}
