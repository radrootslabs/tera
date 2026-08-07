use core::fmt;

use radroots_event::EventId;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

use super::TodayCardType;

pub const CARD_ID_SCHEMA_VERSION: u16 = 1;
const CARD_ID_DOMAIN: &[u8] = b"radroots.today-card.v1\0";

/// Canonical source identity used to derive a stable card identifier.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum CardSourceIdentity {
    Event(EventId),
    Address {
        kind: u32,
        author_pubkey: String,
        identifier: String,
    },
}

#[derive(Clone, Debug, Error, Eq, PartialEq)]
pub enum CardIdError {
    #[error("card address kind must be parameterized replaceable")]
    InvalidAddressKind,
    #[error("card address author must be canonical lowercase hexadecimal")]
    InvalidAuthor,
    #[error("card address identifier is invalid")]
    InvalidIdentifier,
    #[error("card identifier must be 64 lowercase hexadecimal characters")]
    InvalidCardId,
}

impl CardSourceIdentity {
    pub fn address(
        kind: u32,
        author_pubkey: impl Into<String>,
        identifier: impl Into<String>,
    ) -> Result<Self, CardIdError> {
        if !(30_000..40_000).contains(&kind) {
            return Err(CardIdError::InvalidAddressKind);
        }
        let author_pubkey = author_pubkey.into();
        if author_pubkey.len() != 64
            || !author_pubkey
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(CardIdError::InvalidAuthor);
        }
        let identifier = identifier.into();
        if identifier.is_empty()
            || identifier.len() > 512
            || identifier.chars().any(char::is_control)
        {
            return Err(CardIdError::InvalidIdentifier);
        }
        Ok(Self::Address {
            kind,
            author_pubkey,
            identifier,
        })
    }

    pub fn canonical_string(&self) -> String {
        match self {
            Self::Event(event_id) => format!("event:{}", event_id.to_hex()),
            Self::Address {
                kind,
                author_pubkey,
                identifier,
            } => format!("address:{kind}:{author_pubkey}:{identifier}"),
        }
    }
}

/// Lowercase SHA-256 stable identity for one top-level Today card.
#[derive(Clone, Copy, Debug, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct CardId([u8; 32]);

impl CardId {
    pub fn derive(card_type: TodayCardType, source: &CardSourceIdentity) -> Self {
        let mut digest = Sha256::new();
        digest.update(CARD_ID_DOMAIN);
        digest.update(card_type.label().as_bytes());
        digest.update(b"\0");
        digest.update(source.canonical_string().as_bytes());
        Self(digest.finalize().into())
    }

    pub fn parse(value: &str) -> Result<Self, CardIdError> {
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(CardIdError::InvalidCardId);
        }
        let mut bytes = [0; 32];
        hex::decode_to_slice(value, &mut bytes).map_err(|_| CardIdError::InvalidCardId)?;
        Ok(Self(bytes))
    }

    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    pub fn to_hex(self) -> String {
        hex::encode(self.0)
    }
}

impl fmt::Display for CardId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&hex::encode(self.0))
    }
}

impl Serialize for CardId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_hex())
    }
}

impl<'de> Deserialize<'de> for CardId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_card_id_vectors_cover_regular_and_addressable_sources() {
        let event = EventId::parse("a".repeat(64)).expect("event");
        assert_eq!(
            CardId::derive(TodayCardType::Update, &CardSourceIdentity::Event(event)).to_hex(),
            "36bf89dc7a6759143986b1f339870ec792f7bee865c9738b0adb50ac9c5197be"
        );
        let address = CardSourceIdentity::address(31_923, "b".repeat(64), "farmers-market-2026")
            .expect("address");
        assert_eq!(
            CardId::derive(TodayCardType::Event, &address).to_hex(),
            "75f6127161783c583368b6c76ed779ae5e02cd7bc9f71ef55ff39e32a59274fc"
        );
        let replacement = CardId::derive(TodayCardType::Event, &address);
        assert_eq!(replacement, CardId::derive(TodayCardType::Event, &address));
    }

    #[test]
    fn card_identity_rejects_noncanonical_addresses_and_ids() {
        assert!(CardSourceIdentity::address(1, "a".repeat(64), "id").is_err());
        assert!(CardSourceIdentity::address(40_000, "a".repeat(64), "id").is_err());
        assert!(CardSourceIdentity::address(30_402, "a".repeat(63), "id").is_err());
        assert!(CardSourceIdentity::address(30_402, "A".repeat(64), "id").is_err());
        assert!(CardSourceIdentity::address(30_402, "a".repeat(64), "").is_err());
        assert!(CardSourceIdentity::address(30_402, "a".repeat(64), "i".repeat(513)).is_err());
        assert!(CardSourceIdentity::address(30_402, "a".repeat(64), "bad\nid").is_err());
        let opaque = CardSourceIdentity::address(31_923, "a".repeat(64), " market day ")
            .expect("opaque d value");
        assert!(opaque.canonical_string().ends_with(": market day "));
        assert!(CardId::parse(&"a".repeat(63)).is_err());
        assert!(CardId::parse(&"A".repeat(64)).is_err());

        let card = CardId::parse(&"c".repeat(64)).expect("card");
        assert_eq!(card.to_string(), "c".repeat(64));
        let encoded = serde_json::to_string(&card).expect("serialize");
        assert_eq!(
            serde_json::from_str::<CardId>(&encoded).expect("deserialize"),
            card
        );
        assert!(serde_json::from_str::<CardId>(&format!("\"{}\"", "G".repeat(64))).is_err());
    }
}
