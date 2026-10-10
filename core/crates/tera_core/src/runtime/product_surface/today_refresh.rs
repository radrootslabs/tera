use super::*;

impl TeraRuntime {
    /// Materializes current visible event truth for one LocalNetwork.
    pub async fn phase1_refresh_today(
        &self,
        context: &LocalNetwork,
        now_unix_seconds: u64,
        update: TodayProjectionUpdate,
    ) -> Result<TodayRefreshReceipt, TodayError> {
        let _command = self.lifecycle.enter()?;
        if now_unix_seconds == 0 {
            return Err(TodayError::InvalidRequest);
        }
        let query_scope = paging_scope::query_scope(context, self.store_public_key)?;
        let requested_updated_at_unix_ms = now_unix_seconds
            .checked_mul(1_000)
            .ok_or(TodayError::InvalidRequest)?;
        let _projection = self.today_projection_lock.lock().await;
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        // Reject an unsupported reader before rebuilding any owner metadata.
        calendar_migration::ready(storage).await?;
        let policy = self.load_author_visibility().await?;
        let visibility_digest = policy.visibility_digest(
            *EventStore::rebuild_visibility(storage)
                .await?
                .digest()
                .as_bytes(),
        )?;
        let event_status = EventStore::status(storage).await?;
        let generation = projection_generation()?;
        let projection_id = projection_id()?;
        let key = projection_document_key(context);
        let prior = match load_state(storage, context, generation).await? {
            Some(state) => Some(state),
            None => calendar_migration::legacy_state(storage, context).await?,
        };
        if update == TodayProjectionUpdate::Incremental
            && prior.as_ref().is_some_and(|state| {
                state.source_events == event_status.raw_events()
                    && state.query_scope == Some(query_scope)
                    && state.visibility_digest == Some(visibility_digest)
                    && calendar_projection_ready(state)
                    && state.schema_version == TODAY_PROJECTION_DOCUMENT_SCHEMA_VERSION
            })
        {
            let state = prior.expect("checked present");
            return Ok(refresh_receipt(update, &state, false));
        }

        let rebuild =
            calendar_migration::begin(storage, requested_updated_at_unix_ms, &event_status).await?;
        let mut visible = query_all_visible(storage).await?;
        visible.retain(|event| policy.allows(&event.event().envelope().author().to_hex()));
        let local_media = prior.as_ref().map(local_media_evidence).unwrap_or_default();
        let overlays = prior
            .as_ref()
            .map_or_else(BTreeMap::new, |state| state.overlays.clone());
        let overlay_sources = submission_overlay::sources(prior.as_ref());
        let media_cache =
            prior.map_or_else(Phase1MediaCacheIndex::default, |state| state.media_cache);
        let mut state = project_state(
            context,
            event_status.generation().as_bytes(),
            event_status.raw_events(),
            visible,
            overlays,
        )?;
        submission_overlay::retain_sources(&mut state, &overlay_sources);
        state.query_scope = Some(query_scope);
        state.visibility_digest = Some(visibility_digest);
        state.author_visibility_digest = policy.cache_digest()?;
        state.media_cache = media_cache;
        apply_local_media_evidence(&mut state, &local_media);
        state.content_generation = content_generation(&state)?;
        let encoded = encode(&state)?;
        let changed = ProjectionStore::projection_document(
            storage,
            projection_id.clone(),
            generation,
            key.clone(),
        )
        .await?
        .is_none_or(|document| document.value() != encoded);
        let document = ProjectionDocument::new(key, encoded)?;
        let write = self.today_projection_lock.begin_write();
        ProjectionStore::put_projection_document(
            storage,
            projection_id.clone(),
            generation,
            document,
        )
        .await?;
        write.complete();

        let source_position = if event_status.raw_events() == 0 {
            None
        } else {
            Some(EventPosition::new(
                event_status.generation(),
                EventSequence::new(event_status.raw_events())?,
            ))
        };
        let prior_updated_at = ProjectionStore::status(storage, projection_id.clone())
            .await?
            .and_then(|status| {
                status
                    .checkpoint()
                    .map(ProjectionCheckpoint::updated_at_unix_ms)
            })
            .unwrap_or(0);
        let updated_at_unix_ms = requested_updated_at_unix_ms.max(prior_updated_at);
        let checkpoint = ProjectionCheckpoint::new(
            projection_id,
            generation,
            source_position,
            event_status.raw_events(),
            updated_at_unix_ms,
        )?;
        if let Some(ticket) = rebuild {
            calendar_migration::complete(storage, &ticket, checkpoint).await?;
        } else {
            ProjectionStore::checkpoint(storage, checkpoint).await?;
        }
        Ok(refresh_receipt(update, &state, changed))
    }

    pub(super) async fn calendar_state_for_read(
        &self,
        storage: &dyn radroots_storage::Storage,
        context: &LocalNetwork,
        as_of: u64,
    ) -> Result<Option<TodayProjectionState>, TodayError> {
        let state = load_state(storage, context, projection_generation()?).await?;
        let policy_digest = self.load_author_visibility().await?.cache_digest()?;
        let needs_upgrade = state.as_ref().is_some_and(|value| {
            !calendar_projection_ready(value) || value.author_visibility_digest != policy_digest
        }) || (state.is_none()
            && (!calendar_migration::ready(storage).await?
                || calendar_migration::legacy_state(storage, context)
                    .await?
                    .is_some()));
        if needs_upgrade {
            self.phase1_refresh_today(context, as_of, TodayProjectionUpdate::Rebuild)
                .await?;
            load_state(storage, context, projection_generation()?).await
        } else {
            Ok(state)
        }
    }
}
