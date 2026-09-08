use sha2::{Digest, Sha256};
use thiserror::Error;

use super::{CardId, ContextRank, TODAY_RANK_SCHEMA_VERSION, TodayRank};
use crate::runtime::product_surface::ranking::TODAY_RANK_ALGORITHM_VERSION;

const CURSOR_PREFIX: &str = "rrtc1:";
const CURSOR_DOMAIN: &[u8] = b"radroots.today-cursor.v1\0";
const CURSOR_SCHEMA_VERSION: u16 = 1;
const MAX_CONTEXT_ID_BYTES: usize = 256;
const FIXED_PAYLOAD_BYTES: usize = 2 + 2 + 2 + 2 + 8 + 8 + 32 + 8 + 1 + 1 + 8 + 32;
const DIGEST_BYTES: usize = 32;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorScope {
    pub context_id: String,
    pub context_generation: u64,
    pub as_of: u64,
    pub store_generation: [u8; 32],
    pub projection_generation: u64,
}

impl CursorScope {
    pub fn new(
        context_id: String,
        context_generation: u64,
        as_of: u64,
        store_generation: [u8; 32],
        projection_generation: u64,
    ) -> Result<Self, CursorError> {
        validate_context_id(&context_id)?;
        Ok(Self {
            context_id,
            context_generation,
            as_of,
            store_generation,
            projection_generation,
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct TodayCursorPosition {
    pub rank: TodayRank,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TodayCursor(String);

#[derive(Clone, Copy, Debug, Error, Eq, PartialEq)]
pub enum CursorError {
    #[error("today cursor context id is invalid")]
    InvalidContext,
    #[error("today cursor encoding is malformed")]
    Malformed,
    #[error("today cursor integrity check failed")]
    Integrity,
    #[error("today cursor version is unsupported")]
    Version,
    #[error("today cursor belongs to another context")]
    ContextMismatch,
    #[error("today cursor belongs to another frozen snapshot")]
    SnapshotMismatch,
    #[error("today cursor belongs to a retired store or projection generation")]
    Stale,
    #[error("today cursor position is invalid")]
    InvalidPosition,
}

impl TodayCursor {
    pub fn encode(scope: &CursorScope, position: TodayCursorPosition) -> Result<Self, CursorError> {
        if position.rank.schema_version != TODAY_RANK_SCHEMA_VERSION
            || position.rank.algorithm_version != TODAY_RANK_ALGORITHM_VERSION
        {
            return Err(CursorError::Version);
        }
        if position.rank.time_relevance_rank > 4 {
            return Err(CursorError::InvalidPosition);
        }
        let context_bytes = scope.context_id.as_bytes();
        let mut payload = Vec::with_capacity(FIXED_PAYLOAD_BYTES + context_bytes.len());
        payload.extend_from_slice(&CURSOR_SCHEMA_VERSION.to_be_bytes());
        payload.extend_from_slice(&TODAY_RANK_SCHEMA_VERSION.to_be_bytes());
        payload.extend_from_slice(&TODAY_RANK_ALGORITHM_VERSION.to_be_bytes());
        payload.extend_from_slice(
            &u16::try_from(context_bytes.len())
                .expect("validated context length fits u16")
                .to_be_bytes(),
        );
        payload.extend_from_slice(context_bytes);
        payload.extend_from_slice(&scope.context_generation.to_be_bytes());
        payload.extend_from_slice(&scope.as_of.to_be_bytes());
        payload.extend_from_slice(&scope.store_generation);
        payload.extend_from_slice(&scope.projection_generation.to_be_bytes());
        payload.push(position.rank.context_rank.value());
        payload.push(position.rank.time_relevance_rank);
        payload.extend_from_slice(&position.rank.effective_at.to_be_bytes());
        payload.extend_from_slice(position.rank.card_id.as_bytes());
        let digest = cursor_digest(&payload);
        payload.extend_from_slice(&digest);
        Ok(Self(format!("{CURSOR_PREFIX}{}", hex::encode(payload))))
    }

    pub fn decode(value: &str, expected: &CursorScope) -> Result<TodayCursorPosition, CursorError> {
        let (scope, position) = decode_unbound(value)?;
        if scope.context_id != expected.context_id
            || scope.context_generation != expected.context_generation
        {
            return Err(CursorError::ContextMismatch);
        }
        if scope.as_of != expected.as_of {
            return Err(CursorError::SnapshotMismatch);
        }
        if scope.store_generation != expected.store_generation
            || scope.projection_generation != expected.projection_generation
        {
            return Err(CursorError::Stale);
        }
        Ok(position)
    }

    /// Recovers the integrity-checked frozen scope carried by an opaque cursor.
    pub fn scope(value: &str) -> Result<CursorScope, CursorError> {
        decode_unbound(value).map(|(scope, _)| scope)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

fn decode_unbound(value: &str) -> Result<(CursorScope, TodayCursorPosition), CursorError> {
    let encoded = value
        .strip_prefix(CURSOR_PREFIX)
        .ok_or(CursorError::Malformed)?;
    if encoded.len() % 2 != 0
        || !encoded
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(CursorError::Malformed);
    }
    let bytes = hex::decode(encoded).map_err(|_| CursorError::Malformed)?;
    if bytes.len() < FIXED_PAYLOAD_BYTES + DIGEST_BYTES {
        return Err(CursorError::Malformed);
    }
    let (payload, observed_digest) = bytes.split_at(bytes.len() - DIGEST_BYTES);
    if cursor_digest(payload).as_slice() != observed_digest {
        return Err(CursorError::Integrity);
    }
    decode_payload(payload)
}

fn decode_payload(payload: &[u8]) -> Result<(CursorScope, TodayCursorPosition), CursorError> {
    let mut decoder = Decoder::new(payload);
    let cursor_version = decoder.u16()?;
    let rank_schema_version = decoder.u16()?;
    let rank_algorithm_version = decoder.u16()?;
    if cursor_version != CURSOR_SCHEMA_VERSION
        || rank_schema_version != TODAY_RANK_SCHEMA_VERSION
        || rank_algorithm_version != TODAY_RANK_ALGORITHM_VERSION
    {
        return Err(CursorError::Version);
    }
    let context_len = usize::from(decoder.u16()?);
    let context_id =
        core::str::from_utf8(decoder.bytes(context_len)?).map_err(|_| CursorError::Malformed)?;
    validate_context_id(context_id)?;
    let context_generation = decoder.u64()?;
    let as_of = decoder.u64()?;
    let store_generation = decoder.array_32()?;
    let projection_generation = decoder.u64()?;
    let context_rank = ContextRank::from_value(decoder.u8()?).ok_or(CursorError::Malformed)?;
    let time_relevance_rank = decoder.u8()?;
    if time_relevance_rank > 4 {
        return Err(CursorError::Malformed);
    }
    let effective_at = decoder.u64()?;
    let card_id =
        CardId::parse(&hex::encode(decoder.array_32()?)).map_err(|_| CursorError::Malformed)?;
    if !decoder.is_finished() {
        return Err(CursorError::Malformed);
    }
    Ok((
        CursorScope {
            context_id: context_id.to_owned(),
            context_generation,
            as_of,
            store_generation,
            projection_generation,
        },
        TodayCursorPosition {
            rank: TodayRank {
                schema_version: rank_schema_version,
                algorithm_version: rank_algorithm_version,
                context_rank,
                time_relevance_rank,
                effective_at,
                card_id,
            },
        },
    ))
}

fn validate_context_id(value: &str) -> Result<(), CursorError> {
    if value.is_empty()
        || value.len() > MAX_CONTEXT_ID_BYTES
        || value.trim() != value
        || value.chars().any(char::is_control)
    {
        return Err(CursorError::InvalidContext);
    }
    Ok(())
}

fn cursor_digest(payload: &[u8]) -> [u8; 32] {
    let mut digest = Sha256::new();
    digest.update(CURSOR_DOMAIN);
    digest.update(payload);
    digest.finalize().into()
}

struct Decoder<'a> {
    remaining: &'a [u8],
}

impl<'a> Decoder<'a> {
    const fn new(value: &'a [u8]) -> Self {
        Self { remaining: value }
    }

    fn bytes(&mut self, length: usize) -> Result<&'a [u8], CursorError> {
        if self.remaining.len() < length {
            return Err(CursorError::Malformed);
        }
        let (value, remaining) = self.remaining.split_at(length);
        self.remaining = remaining;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, CursorError> {
        Ok(self.bytes(1)?[0])
    }

    fn u16(&mut self) -> Result<u16, CursorError> {
        Ok(u16::from_be_bytes(
            self.bytes(2)?.try_into().expect("exact length"),
        ))
    }

    fn u64(&mut self) -> Result<u64, CursorError> {
        Ok(u64::from_be_bytes(
            self.bytes(8)?.try_into().expect("exact length"),
        ))
    }

    fn array_32(&mut self) -> Result<[u8; 32], CursorError> {
        Ok(self.bytes(32)?.try_into().expect("exact length"))
    }

    const fn is_finished(&self) -> bool {
        self.remaining.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> CursorScope {
        CursorScope::new("nearby".into(), 4, 2_000_000_000, [7; 32], 9).expect("scope")
    }

    fn position() -> TodayCursorPosition {
        TodayCursorPosition {
            rank: TodayRank {
                schema_version: TODAY_RANK_SCHEMA_VERSION,
                algorithm_version: TODAY_RANK_ALGORITHM_VERSION,
                context_rank: ContextRank::LocalityMatch,
                time_relevance_rank: 3,
                effective_at: 1_999_999_000,
                card_id: CardId::parse(&"a".repeat(64)).expect("card"),
            },
        }
    }

    fn payload(cursor: &TodayCursor) -> Vec<u8> {
        let bytes =
            hex::decode(cursor.as_str().strip_prefix(CURSOR_PREFIX).expect("prefix")).expect("hex");
        bytes[..bytes.len() - DIGEST_BYTES].to_vec()
    }

    fn signed_payload(mut payload: Vec<u8>) -> String {
        payload.extend_from_slice(&cursor_digest(&payload));
        format!("{CURSOR_PREFIX}{}", hex::encode(payload))
    }

    #[test]
    fn cursor_vector_round_trips_and_is_fixed() {
        let cursor = TodayCursor::encode(&scope(), position()).expect("cursor");
        assert_eq!(
            cursor.as_str(),
            "rrtc1:00010001000100066e6561726279000000000000000400000000773594000707070707070707070707070707070707070707070707070707070707070707000000000000000902030000000077359018aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaedf305be41633dfc2f7d621e067c3d33a71c3548c6a1fcf68a6707a1d8664b11"
        );
        assert_eq!(
            TodayCursor::decode(cursor.as_str(), &scope()).expect("decode"),
            position()
        );
        assert_eq!(TodayCursor::scope(cursor.as_str()).expect("scope"), scope());
    }

    #[test]
    fn cursor_rejects_tamper_context_snapshot_and_stale_generations() {
        let cursor = TodayCursor::encode(&scope(), position()).expect("cursor");
        let mut tampered = cursor.as_str().as_bytes().to_vec();
        *tampered.last_mut().expect("byte") = b'0';
        assert_eq!(
            TodayCursor::decode(core::str::from_utf8(&tampered).expect("utf8"), &scope()),
            Err(CursorError::Integrity)
        );
        let other_context =
            CursorScope::new("other".into(), 4, 2_000_000_000, [7; 32], 9).expect("scope");
        assert_eq!(
            TodayCursor::decode(cursor.as_str(), &other_context),
            Err(CursorError::ContextMismatch)
        );
        let other_context_generation =
            CursorScope::new("nearby".into(), 5, 2_000_000_000, [7; 32], 9).expect("scope");
        assert_eq!(
            TodayCursor::decode(cursor.as_str(), &other_context_generation),
            Err(CursorError::ContextMismatch)
        );
        let other_snapshot =
            CursorScope::new("nearby".into(), 4, 2_000_000_001, [7; 32], 9).expect("scope");
        assert_eq!(
            TodayCursor::decode(cursor.as_str(), &other_snapshot),
            Err(CursorError::SnapshotMismatch)
        );
        let stale = CursorScope::new("nearby".into(), 4, 2_000_000_000, [8; 32], 9).expect("scope");
        assert_eq!(
            TodayCursor::decode(cursor.as_str(), &stale),
            Err(CursorError::Stale)
        );
        let stale_projection =
            CursorScope::new("nearby".into(), 4, 2_000_000_000, [7; 32], 10).expect("scope");
        assert_eq!(
            TodayCursor::decode(cursor.as_str(), &stale_projection),
            Err(CursorError::Stale)
        );
    }

    #[test]
    fn malformed_and_versioned_cursor_inputs_fail_closed() {
        assert_eq!(
            TodayCursor::decode("nope", &scope()),
            Err(CursorError::Malformed)
        );
        for malformed in ["rrtc1:0", "rrtc1:GG", "rrtc1:00"] {
            assert_eq!(
                TodayCursor::decode(malformed, &scope()),
                Err(CursorError::Malformed)
            );
        }
        assert_eq!(
            TodayCursor::decode(
                &TodayCursor::encode(&scope(), position())
                    .expect("cursor")
                    .as_str()
                    .to_uppercase(),
                &scope()
            ),
            Err(CursorError::Malformed)
        );
        assert!(CursorScope::new("".into(), 0, 0, [0; 32], 0).is_err());
        assert!(CursorScope::new("x".repeat(257), 0, 0, [0; 32], 0).is_err());
        assert!(CursorScope::new(" nearby ".into(), 0, 0, [0; 32], 0).is_err());
        assert!(CursorScope::new("near\u{7f}by".into(), 0, 0, [0; 32], 0).is_err());
        let cursor = TodayCursor::encode(&scope(), position()).expect("cursor");
        for version_offset in [1, 3, 5] {
            let mut unsupported = payload(&cursor);
            unsupported[version_offset] = 2;
            assert_eq!(
                TodayCursor::decode(&signed_payload(unsupported), &scope()),
                Err(CursorError::Version)
            );
        }
        let mut invalid_utf8 = payload(&cursor);
        invalid_utf8[8] = 0xff;
        assert_eq!(
            TodayCursor::decode(&signed_payload(invalid_utf8), &scope()),
            Err(CursorError::Malformed)
        );
        let mut invalid_context = payload(&cursor);
        invalid_context[8] = b' ';
        assert_eq!(
            TodayCursor::decode(&signed_payload(invalid_context), &scope()),
            Err(CursorError::InvalidContext)
        );
        let mut trailing = payload(&cursor);
        trailing.push(0);
        assert_eq!(
            TodayCursor::decode(&signed_payload(trailing), &scope()),
            Err(CursorError::Malformed)
        );
        let mut invalid_context_rank = payload(&cursor);
        invalid_context_rank[70] = 3;
        assert_eq!(
            TodayCursor::decode(&signed_payload(invalid_context_rank), &scope()),
            Err(CursorError::Malformed)
        );
        let mut invalid_time_rank = payload(&cursor);
        invalid_time_rank[71] = 5;
        assert_eq!(
            TodayCursor::decode(&signed_payload(invalid_time_rank), &scope()),
            Err(CursorError::Malformed)
        );
        let mut truncated_field = vec![0; FIXED_PAYLOAD_BYTES];
        truncated_field[1] = 1;
        truncated_field[3] = 1;
        truncated_field[5] = 1;
        truncated_field[6] = 1;
        assert_eq!(
            TodayCursor::decode(&signed_payload(truncated_field), &scope()),
            Err(CursorError::Malformed)
        );
        let invalid_version = TodayCursorPosition {
            rank: TodayRank {
                schema_version: 2,
                ..position().rank
            },
        };
        assert_eq!(
            TodayCursor::encode(&scope(), invalid_version),
            Err(CursorError::Version)
        );
        let invalid_algorithm = TodayCursorPosition {
            rank: TodayRank {
                algorithm_version: 2,
                ..position().rank
            },
        };
        assert_eq!(
            TodayCursor::encode(&scope(), invalid_algorithm),
            Err(CursorError::Version)
        );
        let invalid_rank = TodayCursorPosition {
            rank: TodayRank {
                time_relevance_rank: 5,
                ..position().rank
            },
        };
        assert_eq!(
            TodayCursor::encode(&scope(), invalid_rank),
            Err(CursorError::InvalidPosition)
        );
    }
}
