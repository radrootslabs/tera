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
        coordination_retirement::resume(self).await?;
        let prior_local = sidecar::load(storage, context).await?;
        let legacy = if prior_local.is_none() {
            match load_legacy_state(storage, context, projection_generation()?).await? {
                Some(state) => Some(state),
                None => calendar_migration::legacy_state(storage, context).await?,
            }
        } else {
            None
        };
        let progress =
            match build::run(self, context, query_scope, requested_updated_at_unix_ms).await? {
                build::BuildOutcome::Complete(progress) => progress,
                build::BuildOutcome::Pending => return Err(TodayError::ProjectionRebuilding),
            };
        let event_status = EventStore::status(storage).await?;
        let rebuild =
            calendar_migration::begin(storage, requested_updated_at_unix_ms, &event_status).await?;
        let local = match (prior_local.as_ref(), legacy.as_ref()) {
            (Some(prior), _) => sidecar::LocalState::new(
                context,
                progress.source,
                progress.generation,
                query_scope,
                Some(prior.clone()),
            ),
            (None, Some(prior)) => sidecar::LocalState::from_legacy(
                context,
                progress.source,
                progress.generation,
                query_scope,
                prior,
            )?,
            _ => sidecar::LocalState::new(
                context,
                progress.source,
                progress.generation,
                query_scope,
                None,
            ),
        };
        let changed = prior_local.as_ref() != Some(&local);
        sidecar::save(self, context, &local).await?;
        // Complete existing migration tickets without allocating a second
        // checkpoint identity for ordinary row-based refreshes.
        if let Some(ticket) = rebuild {
            let id = projection_id()?;
            let source_position = (progress.source_events != 0)
                .then(|| {
                    EventSequence::new(progress.source_events)
                        .map(|sequence| EventPosition::new(progress.source.generation(), sequence))
                })
                .transpose()?;
            let checkpoint = ProjectionCheckpoint::new(
                id,
                projection_generation()?,
                source_position,
                progress.source_events,
                requested_updated_at_unix_ms.max(ticket.updated_at_unix_ms()),
            )?;
            let write = self.today_projection_lock.begin_write();
            calendar_migration::complete(storage, &ticket, checkpoint).await?;
            write.complete();
        }
        coordination_retirement::finish(self, &progress, requested_updated_at_unix_ms).await?;
        if EventStore::source_token(storage).await? != progress.source {
            return Err(CursorError::Stale.into());
        }
        let mut receipt = progress.receipt(update, changed);
        receipt.content_generation =
            row_state::content_generation(progress.generation, local.revision()?)?;
        Ok(receipt)
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
            && (sidecar::load(storage, context).await?.is_some()
                || !calendar_migration::ready(storage).await?
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
