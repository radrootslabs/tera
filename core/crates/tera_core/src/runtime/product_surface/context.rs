use std::collections::BTreeSet;

use radroots_transport_nostr::{RelayUrl, RelayUrlPolicy};
use serde::{Deserialize, Serialize};
use thiserror::Error;

const CONTEXT_TEXT_MAX_BYTES: usize = 256;
const RELAY_URL_MAX_BYTES: usize = 2_048;

/// A validated local query/composer context. It has no Nostr event identity.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LocalNetwork {
    pub id: String,
    pub label: String,
    pub relay_urls: Vec<String>,
    pub locality: Option<String>,
    pub followed_authors: Vec<String>,
    pub generation: u64,
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum LocalNetworkError {
    #[error("local network {field} is invalid")]
    InvalidText { field: &'static str },
    #[error("local network requires at least one relay")]
    MissingRelay,
    #[error("local network relay URL is invalid")]
    InvalidRelay,
    #[error("local network relay URLs must be unique")]
    DuplicateRelay,
    #[error("local network followed author is invalid")]
    InvalidAuthor,
    #[error("local network followed authors must be unique")]
    DuplicateAuthor,
}

/// Host environment whose destination policy governs LocalNetwork relays.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum LocalNetworkRelayPolicy {
    Public,
    Simulator,
    Device,
}

impl LocalNetwork {
    pub fn new(
        id: String,
        label: String,
        relay_urls: Vec<String>,
        locality: Option<String>,
        followed_authors: Vec<String>,
        generation: u64,
    ) -> Result<Self, LocalNetworkError> {
        Self::new_for_relay_policy(
            id,
            label,
            relay_urls,
            locality,
            followed_authors,
            generation,
            LocalNetworkRelayPolicy::Public,
        )
    }

    /// Constructs a context under the exact host relay destination policy.
    #[allow(clippy::too_many_arguments)]
    pub fn new_for_relay_policy(
        id: String,
        label: String,
        relay_urls: Vec<String>,
        locality: Option<String>,
        followed_authors: Vec<String>,
        generation: u64,
        relay_policy: LocalNetworkRelayPolicy,
    ) -> Result<Self, LocalNetworkError> {
        validate_text(&id, "id")?;
        validate_text(&label, "label")?;
        if let Some(locality) = locality.as_deref() {
            validate_text(locality, "locality")?;
        }
        if relay_urls.is_empty() {
            return Err(LocalNetworkError::MissingRelay);
        }
        let mut relays = BTreeSet::new();
        let mut canonical_relay_urls = Vec::with_capacity(relay_urls.len());
        for relay in relay_urls {
            if relay.is_empty() || relay.len() > RELAY_URL_MAX_BYTES {
                return Err(LocalNetworkError::InvalidRelay);
            }
            let relay = RelayUrl::parse(
                relay,
                match relay_policy {
                    LocalNetworkRelayPolicy::Public => RelayUrlPolicy::Public,
                    LocalNetworkRelayPolicy::Simulator => RelayUrlPolicy::Local,
                    LocalNetworkRelayPolicy::Device => RelayUrlPolicy::PrivateNetwork,
                },
            )
            .map_err(|_| LocalNetworkError::InvalidRelay)?;
            if !relays.insert(relay.clone()) {
                return Err(LocalNetworkError::DuplicateRelay);
            }
            canonical_relay_urls.push(relay.to_string());
        }
        let mut authors = BTreeSet::new();
        for author in &followed_authors {
            if author.len() != 64
                || !author
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(LocalNetworkError::InvalidAuthor);
            }
            if !authors.insert(author) {
                return Err(LocalNetworkError::DuplicateAuthor);
            }
        }
        Ok(Self {
            id,
            label,
            relay_urls: canonical_relay_urls,
            locality,
            followed_authors,
            generation,
        })
    }

    /// Applies the locked locality policy to the selected local context.
    pub const fn admit(&self, evidence: LocalityEvidence) -> LocalNetworkAdmission {
        match evidence {
            LocalityEvidence::Match => LocalNetworkAdmission::Included(ContextAdmission {
                rank: ContextRank::LocalityMatch,
                reason: "locality_match",
            }),
            LocalityEvidence::Missing => LocalNetworkAdmission::Included(ContextAdmission {
                rank: ContextRank::MissingLocalityFallback,
                reason: "locality_missing_fallback",
            }),
            LocalityEvidence::Nonmatch => LocalNetworkAdmission::Excluded {
                reason: "locality_nonmatch",
            },
        }
    }
}

fn validate_text(value: &str, field: &'static str) -> Result<(), LocalNetworkError> {
    if value.is_empty()
        || value.trim() != value
        || value.len() > CONTEXT_TEXT_MAX_BYTES
        || value.chars().any(char::is_control)
    {
        return Err(LocalNetworkError::InvalidText { field });
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum LocalityEvidence {
    Match,
    Missing,
    Nonmatch,
}

/// The only admitted context-rank values.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum ContextRank {
    MissingLocalityFallback = 1,
    LocalityMatch = 2,
}

impl ContextRank {
    pub const fn value(self) -> u8 {
        self as u8
    }

    pub const fn from_value(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::MissingLocalityFallback),
            2 => Some(Self::LocalityMatch),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextAdmission {
    pub rank: ContextRank,
    pub reason: &'static str,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalNetworkAdmission {
    Included(ContextAdmission),
    Excluded { reason: &'static str },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn network() -> LocalNetwork {
        LocalNetwork::new(
            "local-network".into(),
            "Near me".into(),
            vec!["wss://relay.example".into()],
            Some("u10h".into()),
            vec!["a".repeat(64)],
            7,
        )
        .expect("network")
    }

    #[test]
    fn locality_policy_has_exact_rank_and_exclusion_outcomes() {
        assert_eq!(
            network().admit(LocalityEvidence::Match),
            LocalNetworkAdmission::Included(ContextAdmission {
                rank: ContextRank::LocalityMatch,
                reason: "locality_match",
            })
        );
        assert_eq!(
            network().admit(LocalityEvidence::Missing),
            LocalNetworkAdmission::Included(ContextAdmission {
                rank: ContextRank::MissingLocalityFallback,
                reason: "locality_missing_fallback",
            })
        );
        assert!(matches!(
            network().admit(LocalityEvidence::Nonmatch),
            LocalNetworkAdmission::Excluded { .. }
        ));
    }

    #[test]
    fn local_network_fields_are_bounded_and_unique() {
        assert_eq!(network().generation, 7);
        for invalid in [
            LocalNetwork::new(
                "".into(),
                "label".into(),
                vec!["wss://r".into()],
                None,
                vec![],
                0,
            ),
            LocalNetwork::new("id".into(), "label".into(), vec![], None, vec![], 0),
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec!["".into()],
                None,
                vec![],
                0,
            ),
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec![format!("wss://{}", "r".repeat(RELAY_URL_MAX_BYTES))],
                None,
                vec![],
                0,
            ),
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec!["wss://relay example".into()],
                None,
                vec![],
                0,
            ),
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec!["wss://relay\u{7f}".into()],
                None,
                vec![],
                0,
            ),
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec!["https://r".into()],
                None,
                vec![],
                0,
            ),
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec!["wss://".into()],
                None,
                vec![],
                0,
            ),
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec!["wss://user@relay.example".into()],
                None,
                vec![],
                0,
            ),
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec!["wss://relay.example#fragment".into()],
                None,
                vec![],
                0,
            ),
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec!["wss://r".into(), "wss://r".into()],
                None,
                vec![],
                0,
            ),
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec!["wss://r".into()],
                None,
                vec!["A".repeat(64)],
                0,
            ),
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec!["wss://r".into()],
                None,
                vec!["a".repeat(63)],
                0,
            ),
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec!["wss://r".into()],
                None,
                vec!["a".repeat(64), "a".repeat(64)],
                0,
            ),
            LocalNetwork::new(
                " id ".into(),
                "label".into(),
                vec!["wss://r".into()],
                None,
                vec![],
                0,
            ),
            LocalNetwork::new(
                "i".repeat(CONTEXT_TEXT_MAX_BYTES + 1),
                "label".into(),
                vec!["wss://r".into()],
                None,
                vec![],
                0,
            ),
            LocalNetwork::new(
                "id".into(),
                "la\u{7f}bel".into(),
                vec!["wss://r".into()],
                None,
                vec![],
                0,
            ),
        ] {
            assert!(invalid.is_err());
        }
        let canonical = LocalNetwork::new(
            "id".into(),
            "label".into(),
            vec!["WSS://RELAY.EXAMPLE:443/".into()],
            None,
            vec![],
            0,
        )
        .expect("canonical relay");
        assert_eq!(canonical.relay_urls, vec!["wss://relay.example"]);
        assert!(matches!(
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec![
                    "wss://relay.example".into(),
                    "WSS://RELAY.EXAMPLE:443/".into(),
                ],
                None,
                vec![],
                0,
            ),
            Err(LocalNetworkError::DuplicateRelay)
        ));
        assert!(matches!(
            LocalNetwork::new(
                "id".into(),
                "label".into(),
                vec!["wss://127.0.0.1:7447".into()],
                None,
                vec![],
                0,
            ),
            Err(LocalNetworkError::InvalidRelay)
        ));
        assert!(
            LocalNetwork::new_for_relay_policy(
                "id".into(),
                "label".into(),
                vec!["ws://127.0.0.1:7447".into()],
                None,
                vec![],
                0,
                LocalNetworkRelayPolicy::Simulator,
            )
            .is_ok()
        );
        assert!(
            LocalNetwork::new_for_relay_policy(
                "id".into(),
                "label".into(),
                vec!["wss://192.168.1.7:7447".into()],
                None,
                vec![],
                0,
                LocalNetworkRelayPolicy::Device,
            )
            .is_ok()
        );
        assert!(
            LocalNetwork::new_for_relay_policy(
                "id".into(),
                "label".into(),
                vec!["ws://192.168.1.7:7447".into()],
                None,
                vec![],
                0,
                LocalNetworkRelayPolicy::Device,
            )
            .is_err()
        );
    }
}
