use std::collections::BTreeSet;

use radroots_identity::PublicKey;
use sha2::{Digest, Sha256};

use super::{LocalNetwork, TodayCard, TodayError};
use crate::runtime::product_surface::ranking::{
    TODAY_RANK_ALGORITHM_VERSION, TODAY_RANK_SCHEMA_VERSION,
};

/// Binds the host's selected query to the runtime's authenticated store owner.
/// The digest detects scope changes; it is not a secret or an authorization token.
pub(super) fn query_scope(
    context: &LocalNetwork,
    owner: Option<PublicKey>,
) -> Result<[u8; 32], TodayError> {
    let mut digest = Sha256::new();
    digest.update(b"tera.today-query-scope.v1\0");
    // Serialize an unambiguous tuple of the full context and runtime identity.
    // No field supplied by the cursor participates in this independent binding.
    digest.update(super::encode(&(context, owner.map(|key| key.to_hex())))?);
    Ok(digest.finalize().into())
}

/// Reject corrupt persisted order before a keyset boundary can skip or repeat data.
pub(super) fn validate_order(items: &[TodayCard]) -> Result<(), TodayError> {
    let mut identities = BTreeSet::new();
    let mut previous = None;
    for item in items {
        let rank = item.card.rank.ok_or(TodayError::CorruptProjection)?;
        if rank.schema_version != TODAY_RANK_SCHEMA_VERSION
            || rank.algorithm_version != TODAY_RANK_ALGORITHM_VERSION
            || rank.time_relevance_rank > 4
            || rank.card_id != item.card.card_id
            || !identities.insert(item.card.card_id)
            || previous.is_some_and(|previous| previous >= rank)
        {
            return Err(TodayError::CorruptProjection);
        }
        previous = Some(rank);
    }
    Ok(())
}
