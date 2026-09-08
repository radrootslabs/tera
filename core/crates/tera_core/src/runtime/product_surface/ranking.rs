use core::cmp::Ordering;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::{CardId, ContextRank, TodayCardType};

pub const TODAY_RANK_SCHEMA_VERSION: u16 = 1;
pub const TODAY_RANK_ALGORITHM_VERSION: u16 = 1;
const RANK_DIGEST_DOMAIN: &[u8] = b"radroots.today-rank.v1\0";
const UPCOMING_EVENT_WINDOW_SECONDS: u64 = 7 * 24 * 60 * 60;

/// Exact time inputs used by the deliberately small Phase 1 ranking function.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TimeRelevance {
    Published,
    Event { start: u64, end: Option<u64> },
    FoodAvailability { active: bool },
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TodayRankInput {
    pub card_type: TodayCardType,
    pub context_rank: ContextRank,
    pub as_of: u64,
    pub effective_at: u64,
    pub time: TimeRelevance,
    pub card_id: CardId,
}

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum RankError {
    #[error("card type and time-relevance input do not match")]
    MismatchedTimeProfile,
    #[error("event end must be later than its start")]
    InvalidEventRange,
}

/// Lexicographic Today order key.
///
/// Its [`Ord`] implementation sorts directly into feed order: higher context,
/// time relevance, and effective time first, then lower card ID.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TodayRank {
    pub schema_version: u16,
    pub algorithm_version: u16,
    pub context_rank: ContextRank,
    pub time_relevance_rank: u8,
    pub effective_at: u64,
    pub card_id: CardId,
}

impl TodayRank {
    pub fn derive(input: TodayRankInput) -> Result<Self, RankError> {
        let time_relevance_rank = time_relevance_rank(input)?;
        Ok(Self {
            schema_version: TODAY_RANK_SCHEMA_VERSION,
            algorithm_version: TODAY_RANK_ALGORITHM_VERSION,
            context_rank: input.context_rank,
            time_relevance_rank,
            effective_at: input.effective_at,
            card_id: input.card_id,
        })
    }

    pub fn digest(self) -> [u8; 32] {
        let mut digest = Sha256::new();
        digest.update(RANK_DIGEST_DOMAIN);
        digest.update(self.schema_version.to_be_bytes());
        digest.update(self.algorithm_version.to_be_bytes());
        digest.update([self.context_rank.value()]);
        digest.update([self.time_relevance_rank]);
        digest.update(self.effective_at.to_be_bytes());
        digest.update(self.card_id.as_bytes());
        digest.finalize().into()
    }

    pub fn digest_hex(self) -> String {
        hex::encode(self.digest())
    }
}

impl Ord for TodayRank {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .context_rank
            .cmp(&self.context_rank)
            .then_with(|| other.time_relevance_rank.cmp(&self.time_relevance_rank))
            .then_with(|| other.effective_at.cmp(&self.effective_at))
            .then_with(|| self.card_id.cmp(&other.card_id))
    }
}

impl PartialOrd for TodayRank {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn time_relevance_rank(input: TodayRankInput) -> Result<u8, RankError> {
    match (input.card_type, input.time) {
        (
            TodayCardType::Update | TodayCardType::PhotoUpdate | TodayCardType::Ask,
            TimeRelevance::Published,
        ) => Ok(1),
        (TodayCardType::FoodAvailability, TimeRelevance::FoodAvailability { active }) => {
            Ok(if active { 3 } else { 0 })
        }
        (TodayCardType::Event, TimeRelevance::Event { start, end }) => {
            if end.is_some_and(|end| end <= start) {
                return Err(RankError::InvalidEventRange);
            }
            if end.is_some_and(|end| input.as_of >= end) {
                return Ok(0);
            }
            if input.as_of >= start {
                return Ok(4);
            }
            if start.saturating_sub(input.as_of) <= UPCOMING_EVENT_WINDOW_SECONDS {
                Ok(3)
            } else {
                Ok(2)
            }
        }
        _ => Err(RankError::MismatchedTimeProfile),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(value: char) -> CardId {
        CardId::parse(&value.to_string().repeat(64)).expect("card id")
    }

    fn input(card_type: TodayCardType, time: TimeRelevance) -> TodayRankInput {
        TodayRankInput {
            card_type,
            context_rank: ContextRank::LocalityMatch,
            as_of: 2_000_000_000,
            effective_at: 1_999_999_900,
            time,
            card_id: id('a'),
        }
    }

    #[test]
    fn time_relevance_boundaries_are_exact() {
        assert_eq!(
            TodayRank::derive(input(TodayCardType::Update, TimeRelevance::Published))
                .expect("update")
                .time_relevance_rank,
            1
        );
        assert_eq!(
            TodayRank::derive(input(
                TodayCardType::FoodAvailability,
                TimeRelevance::FoodAvailability { active: true }
            ))
            .expect("food")
            .time_relevance_rank,
            3
        );
        assert_eq!(
            TodayRank::derive(input(
                TodayCardType::FoodAvailability,
                TimeRelevance::FoodAvailability { active: false }
            ))
            .expect("sold food")
            .time_relevance_rank,
            0
        );
        for (start, end, expected) in [
            (1_999_999_900, Some(2_000_000_100), 4),
            (1_999_999_900, None, 4),
            (2_000_604_800, Some(2_000_604_900), 3),
            (2_000_604_801, None, 2),
            (1_999_999_000, Some(2_000_000_000), 0),
        ] {
            assert_eq!(
                TodayRank::derive(input(
                    TodayCardType::Event,
                    TimeRelevance::Event { start, end }
                ))
                .expect("event")
                .time_relevance_rank,
                expected
            );
        }
    }

    #[test]
    fn tuple_sorts_in_locked_feed_order_and_has_a_fixed_digest() {
        let exact = TodayRank::derive(input(TodayCardType::Update, TimeRelevance::Published))
            .expect("rank");
        assert_eq!(
            exact.digest_hex(),
            "c7792876c8177f6f5420cc0f9aa84fb3c478f0bc6555c94ea5a7288502d6e4db"
        );
        let fallback = TodayRank {
            context_rank: ContextRank::MissingLocalityFallback,
            time_relevance_rank: 4,
            effective_at: exact.effective_at + 100,
            card_id: id('e'),
            ..exact
        };
        let lower_time = TodayRank {
            time_relevance_rank: 0,
            effective_at: exact.effective_at + 200,
            card_id: id('d'),
            ..exact
        };
        let older = TodayRank {
            effective_at: exact.effective_at - 1,
            card_id: id('c'),
            ..exact
        };
        let tie_high_id = TodayRank {
            effective_at: exact.effective_at + 1,
            card_id: id('b'),
            ..exact
        };
        let tie_low_id = TodayRank {
            effective_at: exact.effective_at + 1,
            card_id: id('a'),
            ..exact
        };
        let mut values = vec![fallback, lower_time, older, tie_high_id, tie_low_id];
        values.sort();
        assert_eq!(
            values,
            vec![tie_low_id, tie_high_id, older, lower_time, fallback]
        );
    }

    #[test]
    fn mismatched_and_invalid_time_inputs_fail_closed() {
        assert_eq!(
            TodayRank::derive(input(
                TodayCardType::Update,
                TimeRelevance::FoodAvailability { active: true }
            )),
            Err(RankError::MismatchedTimeProfile)
        );
        assert_eq!(
            TodayRank::derive(input(
                TodayCardType::Event,
                TimeRelevance::Event {
                    start: 10,
                    end: Some(10),
                }
            )),
            Err(RankError::InvalidEventRange)
        );
    }
}
