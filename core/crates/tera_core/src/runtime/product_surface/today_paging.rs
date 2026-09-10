use super::*;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TodayPageRequest {
    pub limit: u16,
    pub as_of: Option<u64>,
    pub cursor: Option<String>,
    pub viewer_time_zone: Option<String>,
}

impl TodayPageRequest {
    pub fn first(limit: u16, as_of: u64, viewer_time_zone: &str) -> Self {
        Self {
            limit,
            as_of: Some(as_of),
            cursor: None,
            viewer_time_zone: Some(viewer_time_zone.to_owned()),
        }
    }

    pub fn after(limit: u16, cursor: String) -> Self {
        Self {
            limit,
            as_of: None,
            cursor: Some(cursor),
            viewer_time_zone: None,
        }
    }
}

impl TeraRuntime {
    /// Returns one page from a durable frozen Today snapshot.
    pub async fn phase1_today_page(
        &self,
        context: &LocalNetwork,
        request: TodayPageRequest,
    ) -> Result<TodayPage, TodayError> {
        let _command = self.lifecycle.enter()?;
        if request.limit == 0 || request.limit > TODAY_PAGE_LIMIT_MAX {
            return Err(TodayError::InvalidRequest);
        }
        if request.cursor.is_some() {
            if request.as_of.is_some() || request.viewer_time_zone.is_some() {
                return Err(TodayError::InvalidRequest);
            }
        } else if request.as_of.is_none() || request.viewer_time_zone.is_none() {
            return Err(TodayError::InvalidRequest);
        }
        let first_calendar = request
            .as_of
            .map(|as_of| {
                crate::runtime::product_surface::ViewerCalendarContext::new(
                    as_of,
                    request
                        .viewer_time_zone
                        .as_deref()
                        .ok_or(TodayError::InvalidRequest)?,
                )
            })
            .transpose()?;
        // Decode and validate all caller-owned scope before any storage I/O.
        let decoded_scope = request
            .cursor
            .as_deref()
            .map(TodayCursor::scope)
            .transpose()?;
        let query_scope = paging_scope::query_scope(context, self.store_public_key)?;
        if let Some(scope) = &decoded_scope {
            if scope.context_id != context.id
                || scope.context_generation != context.generation
                || scope.query_scope != query_scope
            {
                return Err(CursorError::ContextMismatch.into());
            }
            if request.as_of.is_some_and(|as_of| as_of != scope.as_of) {
                return Err(CursorError::SnapshotMismatch.into());
            }
        } else if request.as_of.is_none_or(|value| value == 0) {
            return Err(TodayError::InvalidRequest);
        }
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let store_generation = current_store_generation(storage).await?;
        let algorithm_generation = projection_generation()?;
        let projection_id = projection_id()?;
        let (scope, snapshot, after) = if let Some(cursor) = request.cursor.as_deref() {
            let scope = decoded_scope.expect("cursor presence was decoded above");
            if scope.store_generation != store_generation {
                return Err(CursorError::Stale.into());
            }
            let position = TodayCursor::decode(cursor, &scope)?;
            let mut snapshot = load_snapshot(storage, projection_id, algorithm_generation, &scope)
                .await?
                .ok_or(CursorError::Stale)?;
            let current = load_state(storage, context, algorithm_generation)
                .await?
                .ok_or(CursorError::Stale)?;
            if current.store_generation != scope.store_generation
                || current.query_scope != Some(scope.query_scope)
                || current.visibility_digest.is_none()
                || !calendar_projection_ready(&current)
                || current.content_generation != scope.projection_generation
            {
                return Err(CursorError::Stale.into());
            }
            sanitize_snapshot_media(&mut snapshot, &current.media_cache);
            (scope, snapshot, Some(position.rank))
        } else {
            let as_of = request
                .as_of
                .filter(|value| *value != 0)
                .ok_or(TodayError::InvalidRequest)?;
            let state = match load_state(storage, context, algorithm_generation).await? {
                Some(state)
                    if state.query_scope == Some(query_scope)
                        && state.visibility_digest.is_some()
                        && calendar_projection_ready(&state) =>
                {
                    state
                }
                _ => {
                    // A first local read must not need a prior relay refresh.
                    // Materialize only already admitted local events; errors
                    // remain errors rather than becoming an empty feed.
                    self.phase1_refresh_today(context, as_of, TodayProjectionUpdate::Incremental)
                        .await?;
                    load_state(storage, context, algorithm_generation)
                        .await?
                        .ok_or(TodayError::ProjectionMissing)?
                }
            };
            if state.store_generation != store_generation || state.query_scope != Some(query_scope)
            {
                return Err(CursorError::Stale.into());
            }
            let scope = CursorScope::new(
                context.id.clone().into(),
                context.generation,
                as_of,
                state.store_generation,
                state.content_generation,
                query_scope,
                first_calendar.expect("validated first-page context"),
            )?;
            let snapshot = frozen_snapshot(&state, context, &scope.calendar, query_scope)?;
            persist_snapshot(storage, algorithm_generation, &scope, &snapshot).await?;
            (scope, snapshot, None)
        };

        page_from_snapshot(snapshot, scope, after, request.limit)
    }
}
