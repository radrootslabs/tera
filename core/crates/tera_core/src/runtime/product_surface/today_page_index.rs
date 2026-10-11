//! Ordered references retain every possible frozen-calendar bucket, without
//! copying cards or creating a cache generation for each caller's clock.
use super::*;
use crate::runtime::product_surface::ranking::{
    TODAY_RANK_ALGORITHM_VERSION, TODAY_RANK_SCHEMA_VERSION,
};

pub(super) const SCHEMA: u16 = 1;
pub(super) const FIRST: &str = "rank.";
pub(super) const LAST: &str = "rank.~";

pub(super) fn key(rank: TodayRank) -> String {
    Reference {
        schema_version: SCHEMA,
        rank,
    }
    .key()
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub(super) struct Reference {
    schema_version: u16,
    pub(super) rank: TodayRank,
}

impl Reference {
    pub(super) fn key(&self) -> String {
        format!(
            "rank.{:02x}.{:02x}.{:016x}.{}",
            u8::MAX - self.rank.context_rank.value(),
            u8::MAX - self.rank.time_relevance_rank,
            u64::MAX - self.rank.effective_at,
            self.rank.card_id.to_hex()
        )
    }

    pub(super) fn from_document(document: &ProjectionDocument) -> Result<Self, TodayError> {
        let value: Self = decode(document.value())?;
        if value.schema_version != SCHEMA
            || value.rank.schema_version != TODAY_RANK_SCHEMA_VERSION
            || value.rank.algorithm_version != TODAY_RANK_ALGORITHM_VERSION
            || value.rank.time_relevance_rank > 4
            || value.key() != document.key()
        {
            return Err(TodayError::CorruptProjection);
        }
        Ok(value)
    }
}

pub(super) fn documents(card: &ProjectedCard) -> Result<Vec<ProjectionDocument>, TodayError> {
    let buckets: &[u8] = match card.card.card_type {
        TodayCardType::Update | TodayCardType::PhotoUpdate | TodayCardType::Ask => &[1],
        TodayCardType::FoodAvailability => {
            if card.card.lifecycle == CardLifecycleState::Active {
                &[3]
            } else {
                &[0]
            }
        }
        TodayCardType::Event => &[0, 2, 3, 4],
    };
    buckets
        .iter()
        .map(|bucket| {
            let value = Reference {
                schema_version: SCHEMA,
                rank: TodayRank {
                    schema_version: TODAY_RANK_SCHEMA_VERSION,
                    algorithm_version: TODAY_RANK_ALGORITHM_VERSION,
                    context_rank: card.card.context_rank,
                    time_relevance_rank: *bucket,
                    effective_at: card.card.effective_at,
                    card_id: card.card.card_id,
                },
            };
            Ok(ProjectionDocument::new(value.key(), encode(&value)?)?)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::product_surface::ContextRank;

    #[test]
    fn ordered_reference_keys_preserve_every_rank_component_and_stable_ties() {
        let mut values = Vec::new();
        for context in [
            ContextRank::MissingLocalityFallback,
            ContextRank::LocalityMatch,
        ] {
            for bucket in 0..=4 {
                for effective_at in [0, 1, u64::MAX] {
                    for identity in ['a', 'b'] {
                        values.push(Reference {
                            schema_version: SCHEMA,
                            rank: TodayRank {
                                schema_version: TODAY_RANK_SCHEMA_VERSION,
                                algorithm_version: TODAY_RANK_ALGORITHM_VERSION,
                                context_rank: context,
                                time_relevance_rank: bucket,
                                effective_at,
                                card_id: CardId::parse(&identity.to_string().repeat(64)).unwrap(),
                            },
                        });
                    }
                }
            }
        }
        let mut ranked: Vec<_> = values.iter().map(|value| value.rank).collect();
        ranked.sort();
        values.sort_by_key(Reference::key);
        assert_eq!(
            ranked,
            values.iter().map(|value| value.rank).collect::<Vec<_>>()
        );
        for reference in values {
            let document =
                ProjectionDocument::new(reference.key(), encode(&reference).unwrap()).unwrap();
            assert!(document.value().len() <= 1024);
            assert_eq!(
                Reference::from_document(&document).unwrap().rank,
                reference.rank
            );
            let changed =
                ProjectionDocument::new("rank.foreign".into(), document.value().into()).unwrap();
            assert!(Reference::from_document(&changed).is_err());
        }
    }
}
