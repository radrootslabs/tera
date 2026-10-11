//! Join only selected card/profile rows; no full-feed compatibility reader.
use super::*;

pub(super) async fn row(
    runtime: &TeraRuntime,
    scope: &page_scope::Scope,
    key: String,
    budget: &mut page_budget::Budget,
) -> Result<Option<rows::TodayRow>, TodayError> {
    let storage = runtime
        .client
        .storage()
        .map_err(|_| TodayError::RuntimeUnavailable)?;
    let document = budget
        .document(
            storage,
            ProjectionId::parse(build::progress::ID)?,
            scope.local.generation,
            key,
            Some(scope.source),
        )
        .await?;
    document
        .as_ref()
        .map(rows::TodayRow::from_document)
        .transpose()
}

pub(super) async fn card(
    runtime: &TeraRuntime,
    scope: &page_scope::Scope,
    identity: CardId,
    budget: &mut page_budget::Budget,
) -> Result<ProjectedCard, TodayError> {
    match row(
        runtime,
        scope,
        format!("card.{}", identity.to_hex()),
        budget,
    )
    .await?
    {
        Some(rows::TodayRow::Card(card)) if card.card.card_id == identity => Ok(card),
        _ => Err(TodayError::CorruptProjection),
    }
}

pub(super) async fn profile(
    runtime: &TeraRuntime,
    scope: &page_scope::Scope,
    author: &str,
    state: &mut TodayProjectionState,
    budget: &mut page_budget::Budget,
) -> Result<(), TodayError> {
    if state.profiles.contains_key(author) {
        return Ok(());
    }
    match row(runtime, scope, format!("profile.{author}"), budget).await? {
        Some(rows::TodayRow::Profile { summary, .. }) if summary.author_pubkey == author => {
            state.profiles.insert(author.into(), summary);
            Ok(())
        }
        None => Ok(()),
        _ => Err(TodayError::CorruptProjection),
    }
}

pub(super) fn ranked_card(
    projected: &ProjectedCard,
    context: &LocalNetwork,
    calendar: &super::super::ViewerCalendarContext,
) -> Result<Option<ClassifiedCard>, TodayError> {
    let as_of = calendar.as_of();
    let evidence = locality_evidence(context.locality.as_deref(), &projected.locality);
    let admission = match context.admit(evidence) {
        super::super::LocalNetworkAdmission::Included(admission) => admission,
        super::super::LocalNetworkAdmission::Excluded { .. } => return Ok(None),
    };
    let mut card = projected.card.clone();
    card.context_rank = admission.rank;
    card.inclusion_reason = admission.reason.to_owned();
    let time = match card.card_type {
        TodayCardType::Update | TodayCardType::PhotoUpdate | TodayCardType::Ask => {
            TimeRelevance::Published
        }
        TodayCardType::FoodAvailability => TimeRelevance::FoodAvailability {
            active: card.lifecycle == CardLifecycleState::Active,
        },
        TodayCardType::Event => match card
            .calendar_timing
            .as_ref()
            .ok_or(TodayError::CorruptProjection)?
        {
            CalendarTiming::TimeBased(timing) => TimeRelevance::TimeBased {
                start: timing.start(),
                end: timing.end_exclusive(),
            },
            CalendarTiming::DateBased(event) => {
                let as_of_date = calendar.civil_date().clone();
                TimeRelevance::DateBased {
                    event: event.clone(),
                    as_of_date,
                }
            }
        },
    };
    let rank = TodayRank::derive(TodayRankInput {
        card_type: card.card_type,
        context_rank: card.context_rank,
        as_of,
        effective_at: card.effective_at,
        time,
        card_id: card.card_id,
    })
    .map_err(|_| TodayError::CorruptProjection)?;
    if card.card_type == TodayCardType::Event && rank.time_relevance_rank == 0 {
        card.lifecycle = CardLifecycleState::Past;
    }
    card.rank = Some(rank);
    Ok(Some(card))
}
