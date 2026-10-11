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
    /// Selects a bounded page from one current promoted generation and frozen clock.
    pub async fn phase1_today_page(
        &self,
        context: &LocalNetwork,
        request: TodayPageRequest,
    ) -> Result<TodayPage, TodayError> {
        let _command = self.lifecycle.enter()?;
        let decoded = validate_request(context, self.store_public_key, &request)?;
        let calendar = match &decoded {
            Some(decoded) => decoded.scope.calendar.clone(),
            None => super::super::ViewerCalendarContext::new(
                request.as_of.ok_or(TodayError::InvalidRequest)?,
                request
                    .viewer_time_zone
                    .as_deref()
                    .ok_or(TodayError::InvalidRequest)?,
            )?,
        };
        let mut budget = page_budget::Budget::default();
        let mut guard = self.today_projection_lock.lock().await;
        let mut scope =
            page_scope::Scope::load(self, context, calendar.clone(), &mut budget).await?;
        if scope.is_none() && decoded.is_none() {
            drop(guard);
            self.phase1_refresh_today(
                context,
                calendar.as_of(),
                TodayProjectionUpdate::Incremental,
            )
            .await?;
            guard = self.today_projection_lock.lock().await;
            scope = page_scope::Scope::load(self, context, calendar, &mut budget).await?;
        }
        let scope = scope.ok_or(CursorError::Stale)?;
        let mut finish = budget.reserve_scope()?;
        let seen = decoded.as_ref().map_or(0, |decoded| decoded.seen);
        if seen > scope.header.cards {
            return Err(CursorError::InvalidPosition.into());
        }
        let mut after = None;
        if let Some(decoded) = &decoded {
            scope.check_cursor(decoded)?;
            validate_position(self, &scope, decoded.rank, &mut budget).await?;
            after = Some(decoded.rank);
        }
        let mut state = scope.state(context);
        let starting_after = after;
        let mut thread_cursors = BTreeMap::new();
        let mut next = None;
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        loop {
            if !budget.can_select() {
                if after == starting_after {
                    return Err(radroots_storage::Error::SpaceInsufficient.into());
                }
                next = after;
                break;
            }
            let page = budget
                .query(
                    storage,
                    ProjectionId::parse(build::progress::ID)?,
                    scope.local.generation,
                    (page_index::FIRST.into(), page_index::LAST.into()),
                    after.map(page_index::key),
                    Some(scope.source),
                )
                .await?;
            let Some(record) = page.records().first() else {
                break;
            };
            let reference = page_index::Reference::from_document(
                record.document().ok_or(TodayError::CorruptProjection)?,
            )?;
            let projected =
                match page_hydration::card(self, &scope, reference.rank.card_id, &mut budget).await
                {
                    Err(TodayError::Storage(radroots_storage::Error::SpaceInsufficient))
                        if after != starting_after =>
                    {
                        next = after;
                        break;
                    }
                    result => result?,
                };
            let card = page_hydration::ranked_card(&projected, context, &scope.cursor.calendar)?
                .ok_or(TodayError::CorruptProjection)?;
            let actual = card.rank.ok_or(TodayError::CorruptProjection)?;
            if actual.card_id != reference.rank.card_id
                || actual.context_rank != reference.rank.context_rank
                || actual.effective_at != reference.rank.effective_at
            {
                return Err(TodayError::CorruptProjection);
            }
            if actual.time_relevance_rank == reference.rank.time_relevance_rank {
                // Charge the lookahead card, but never advance past an eligible
                // card that has not been returned to the caller.
                if state.cards.len() == usize::from(request.limit) {
                    next = after;
                    break;
                }
                let joined = async {
                    page_hydration::profile(
                        self,
                        &scope,
                        &card.author_pubkey,
                        &mut state,
                        &mut budget,
                    )
                    .await?;
                    page_threads::page(self, &scope, &card, None, 8, &mut budget).await
                }
                .await;
                let (thread, thread_after) = match joined {
                    Err(TodayError::Storage(radroots_storage::Error::SpaceInsufficient))
                        if after != starting_after =>
                    {
                        next = after;
                        break;
                    }
                    result => result?,
                };
                if let Some(thread_after) = thread_after {
                    thread_cursors.insert(
                        card.card_id,
                        page_cursor::encode(
                            &scope.cursor,
                            actual,
                            scope.fingerprint,
                            Some(thread_after),
                            0,
                        )?,
                    );
                }
                for entry in &thread {
                    page_hydration::profile(
                        self,
                        &scope,
                        &entry.author_pubkey,
                        &mut state,
                        &mut budget,
                    )
                    .await?;
                }
                state.thread.extend(thread);
                state.cards.push(projected);
            }
            after = Some(reference.rank);
            if page.next_cursor().is_none() {
                break;
            }
        }
        let seen = seen
            .checked_add(state.cards.len() as u64)
            .ok_or(TodayError::CorruptProjection)?;
        if seen > scope.header.cards || (next.is_none() && seen != scope.header.cards) {
            return Err(TodayError::CorruptProjection);
        }
        scope.local.apply(&mut state);
        validate_media_state(&state)?;
        state
            .thread
            .sort_by(|left, right| left.event_id.cmp(&right.event_id));
        let mut items = ranked_cards(&state, context, &scope.cursor.calendar)?;
        for item in &mut items {
            item.thread_next_cursor = thread_cursors.remove(&item.card.card_id);
        }
        paging_scope::validate_order(&items)?;
        check_current(self, context, &scope, &mut finish).await?;
        drop(guard);
        Ok(TodayPage {
            projection_generation: scope.cursor.projection_generation,
            as_of: scope.cursor.as_of,
            calendar: scope.cursor.calendar.clone(),
            items,
            next_cursor: next
                .map(|rank| page_cursor::encode(&scope.cursor, rank, scope.fingerprint, None, seen))
                .transpose()?,
        })
    }

    /// Continues the selected card's thread without copying the remaining thread.
    pub async fn phase1_today_thread_page(
        &self,
        context: &LocalNetwork,
        limit: u16,
        cursor: &str,
    ) -> Result<super::super::TodayThreadPage, TodayError> {
        let _command = self.lifecycle.enter()?;
        if limit == 0 || limit > TODAY_PAGE_LIMIT_MAX {
            return Err(TodayError::InvalidRequest);
        }
        let query_scope = paging_scope::query_scope(context, self.store_public_key)?;
        let decoded = page_cursor::decode(cursor)?;
        validate_context(context, query_scope, &decoded.scope)?;
        let after = decoded.thread_after.ok_or(TodayError::InvalidRequest)?;
        let _guard = self.today_projection_lock.lock().await;
        let mut budget = page_budget::Budget::default();
        let scope =
            page_scope::Scope::load(self, context, decoded.scope.calendar.clone(), &mut budget)
                .await?
                .ok_or(CursorError::Stale)?;
        scope.check_cursor(&decoded)?;
        let mut finish = budget.reserve_scope()?;
        validate_position(self, &scope, decoded.rank, &mut budget).await?;
        let projected =
            page_hydration::card(self, &scope, decoded.rank.card_id, &mut budget).await?;
        let card = page_hydration::ranked_card(&projected, context, &scope.cursor.calendar)?
            .ok_or(TodayError::CorruptProjection)?;
        if card.rank != Some(decoded.rank) {
            return Err(CursorError::InvalidPosition.into());
        }
        page_threads::validate_after(self, &scope, &card, after, &mut budget).await?;
        let (items, next) =
            page_threads::page(self, &scope, &card, Some(after), limit, &mut budget).await?;
        let mut state = scope.state(context);
        for entry in &items {
            page_hydration::profile(self, &scope, &entry.author_pubkey, &mut state, &mut budget)
                .await?;
        }
        state.thread = items;
        scope.local.apply(&mut state);
        validate_media_state(&state)?;
        check_current(self, context, &scope, &mut finish).await?;
        Ok(super::super::TodayThreadPage {
            card_id: card.card_id,
            items: state.thread,
            next_cursor: next
                .map(|after| {
                    page_cursor::encode(
                        &scope.cursor,
                        decoded.rank,
                        scope.fingerprint,
                        Some(after),
                        0,
                    )
                })
                .transpose()?,
        })
    }
}

fn validate_context(
    context: &LocalNetwork,
    query_scope: [u8; 32],
    scope: &CursorScope,
) -> Result<(), TodayError> {
    if scope.context_id != context.id
        || scope.context_generation != context.generation
        || scope.query_scope != query_scope
    {
        return Err(CursorError::ContextMismatch.into());
    }
    Ok(())
}

fn validate_request(
    context: &LocalNetwork,
    owner: Option<radroots_identity::PublicKey>,
    request: &TodayPageRequest,
) -> Result<Option<page_cursor::Decoded>, TodayError> {
    if request.limit == 0 || request.limit > TODAY_PAGE_LIMIT_MAX {
        return Err(TodayError::InvalidRequest);
    }
    let query_scope = paging_scope::query_scope(context, owner)?;
    if let Some(cursor) = &request.cursor {
        if request.as_of.is_some() || request.viewer_time_zone.is_some() {
            return Err(TodayError::InvalidRequest);
        }
        let decoded = page_cursor::decode(cursor)?;
        if decoded.thread_after.is_some() {
            return Err(TodayError::InvalidRequest);
        }
        validate_context(context, query_scope, &decoded.scope)?;
        Ok(Some(decoded))
    } else {
        if request.as_of.is_none_or(|as_of| as_of == 0) || request.viewer_time_zone.is_none() {
            return Err(TodayError::InvalidRequest);
        }
        Ok(None)
    }
}

async fn validate_position(
    runtime: &TeraRuntime,
    scope: &page_scope::Scope,
    rank: TodayRank,
    budget: &mut page_budget::Budget,
) -> Result<(), TodayError> {
    let storage = runtime
        .client
        .storage()
        .map_err(|_| TodayError::RuntimeUnavailable)?;
    let document = budget
        .document(
            storage,
            ProjectionId::parse(build::progress::ID)?,
            scope.local.generation,
            page_index::key(rank),
            Some(scope.source),
        )
        .await?
        .ok_or(TodayError::CursorPositionMissing)?;
    if page_index::Reference::from_document(&document)?.rank != rank {
        return Err(TodayError::CursorPositionMissing);
    }
    Ok(())
}

async fn check_current(
    runtime: &TeraRuntime,
    context: &LocalNetwork,
    scope: &page_scope::Scope,
    budget: &mut page_budget::Budget,
) -> Result<(), TodayError> {
    let current = page_scope::Scope::load(runtime, context, scope.cursor.calendar.clone(), budget)
        .await?
        .ok_or(CursorError::Stale)?;
    if current.fingerprint != scope.fingerprint {
        return Err(CursorError::Stale.into());
    }
    Ok(())
}
