use sha2::{Digest, Sha256};
use thiserror::Error;

use super::local_network_id::LOCAL_NETWORK_ID_MAX_BYTES;
use super::{CardId, ContextRank, LocalNetworkId, TODAY_RANK_SCHEMA_VERSION, TodayRank};
use super::{VIEWER_CALENDAR_VERSION, VIEWER_TIME_ZONE_MAX_BYTES, ViewerCalendarContext};
use crate::runtime::product_surface::ranking::TODAY_RANK_ALGORITHM_VERSION;

const CURSOR_PREFIX: &str = "rrtc3:";
const CURSOR_DOMAIN: &[u8] = b"tera.today-cursor.v3\0";
const CURSOR_SCHEMA_VERSION: u16 = 3;
const FIXED_PAYLOAD_BYTES: usize =
    2 + 2 + 2 + 2 + 8 + 8 + 32 + 8 + 1 + 1 + 8 + 32 + 32 + 2 + 2 + 2 + 1 + 1;
const DIGEST_BYTES: usize = 32;
const MAX_CURSOR_BYTES: usize = CURSOR_PREFIX.len()
    + 2 * (FIXED_PAYLOAD_BYTES
        + LOCAL_NETWORK_ID_MAX_BYTES
        + VIEWER_TIME_ZONE_MAX_BYTES
        + DIGEST_BYTES);
const LEGACY_MAX_CURSOR_BYTES: usize = 6 + 2 * (106 + LOCAL_NETWORK_ID_MAX_BYTES + DIGEST_BYTES);
const V2_MAX_CURSOR_BYTES: usize = 6 + 2 * (138 + LOCAL_NETWORK_ID_MAX_BYTES + DIGEST_BYTES);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CursorScope {
    pub context_id: LocalNetworkId,
    pub context_generation: u64,
    pub as_of: u64,
    pub store_generation: [u8; 32],
    pub projection_generation: u64,
    pub query_scope: [u8; 32],
    pub calendar: ViewerCalendarContext,
}

impl CursorScope {
    pub fn new(
        context_id: String,
        context_generation: u64,
        as_of: u64,
        store_generation: [u8; 32],
        projection_generation: u64,
        query_scope: [u8; 32],
        calendar: ViewerCalendarContext,
    ) -> Result<Self, CursorError> {
        let context_id =
            LocalNetworkId::new(context_id).map_err(|_| CursorError::InvalidContext)?;
        if calendar.as_of() != as_of {
            return Err(CursorError::SnapshotMismatch);
        }
        Ok(Self {
            context_id,
            context_generation,
            as_of,
            store_generation,
            projection_generation,
            query_scope,
            calendar,
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
        if scope.calendar.as_of() != scope.as_of {
            return Err(CursorError::SnapshotMismatch);
        }
        if position.rank.schema_version != TODAY_RANK_SCHEMA_VERSION
            || position.rank.algorithm_version != TODAY_RANK_ALGORITHM_VERSION
        {
            return Err(CursorError::Version);
        }
        if position.rank.time_relevance_rank > 4 {
            return Err(CursorError::InvalidPosition);
        }
        let context_bytes = scope.context_id.as_bytes();
        let context_len =
            u16::try_from(context_bytes.len()).map_err(|_| CursorError::InvalidContext)?;
        let mut payload = Vec::with_capacity(
            FIXED_PAYLOAD_BYTES + context_bytes.len() + scope.calendar.time_zone().len(),
        );
        payload.extend_from_slice(&CURSOR_SCHEMA_VERSION.to_be_bytes());
        payload.extend_from_slice(&TODAY_RANK_SCHEMA_VERSION.to_be_bytes());
        payload.extend_from_slice(&TODAY_RANK_ALGORITHM_VERSION.to_be_bytes());
        payload.extend_from_slice(&context_len.to_be_bytes());
        payload.extend_from_slice(context_bytes);
        payload.extend_from_slice(&scope.context_generation.to_be_bytes());
        payload.extend_from_slice(&scope.as_of.to_be_bytes());
        payload.extend_from_slice(&scope.store_generation);
        payload.extend_from_slice(&scope.projection_generation.to_be_bytes());
        payload.push(position.rank.context_rank.value());
        payload.push(position.rank.time_relevance_rank);
        payload.extend_from_slice(&position.rank.effective_at.to_be_bytes());
        payload.extend_from_slice(position.rank.card_id.as_bytes());
        payload.extend_from_slice(&scope.query_scope);
        payload.extend_from_slice(&scope.calendar.version().to_be_bytes());
        let zone = scope.calendar.time_zone().as_bytes();
        payload.extend_from_slice(&(zone.len() as u16).to_be_bytes());
        payload.extend_from_slice(zone);
        let date = scope.calendar.civil_date().as_str();
        // CalendarDate guarantees the canonical ten-byte Gregorian layout.
        payload.extend_from_slice(
            &date[..4]
                .parse::<u16>()
                .expect("validated year")
                .to_be_bytes(),
        );
        payload.push(date[5..7].parse().expect("validated month"));
        payload.push(date[8..].parse().expect("validated day"));
        let digest = cursor_digest(&payload);
        payload.extend_from_slice(&digest);
        Ok(Self(format!("{CURSOR_PREFIX}{}", hex::encode(payload))))
    }

    pub fn decode(value: &str, expected: &CursorScope) -> Result<TodayCursorPosition, CursorError> {
        let (scope, position) = decode_unbound(value)?;
        if scope.context_id != expected.context_id
            || scope.context_generation != expected.context_generation
            || scope.query_scope != expected.query_scope
        {
            return Err(CursorError::ContextMismatch);
        }
        if scope.as_of != expected.as_of || scope.calendar != expected.calendar {
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
    // The v3 token is bounded before any content scan, hex allocation or hash.
    if value.len() > MAX_CURSOR_BYTES {
        return Err(CursorError::Malformed);
    }
    if value.starts_with("rrtc1:") {
        return Err(if value.len() > LEGACY_MAX_CURSOR_BYTES {
            CursorError::Malformed
        } else {
            CursorError::Version
        });
    }
    if value.starts_with("rrtc2:") {
        return Err(if value.len() > V2_MAX_CURSOR_BYTES {
            CursorError::Malformed
        } else {
            CursorError::Version
        });
    }
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
    let context_id =
        LocalNetworkId::new(context_id.to_owned()).map_err(|_| CursorError::InvalidContext)?;
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
    let query_scope = decoder.array_32()?;
    if decoder.u16()? != VIEWER_CALENDAR_VERSION {
        return Err(CursorError::Version);
    }
    let zone_len = usize::from(decoder.u16()?);
    if zone_len > VIEWER_TIME_ZONE_MAX_BYTES {
        return Err(CursorError::Malformed);
    }
    let zone =
        core::str::from_utf8(decoder.bytes(zone_len)?).map_err(|_| CursorError::Malformed)?;
    let calendar = ViewerCalendarContext::new(as_of, zone).map_err(|_| CursorError::Malformed)?;
    let date = format!(
        "{:04}-{:02}-{:02}",
        decoder.u16()?,
        decoder.u8()?,
        decoder.u8()?
    );
    if calendar.civil_date().as_str() != date {
        return Err(CursorError::SnapshotMismatch);
    }
    if !decoder.is_finished() {
        return Err(CursorError::Malformed);
    }
    Ok((
        CursorScope {
            context_id,
            context_generation,
            as_of,
            store_generation,
            projection_generation,
            query_scope,
            calendar,
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
#[path = "cursor_boundary_tests.rs"]
mod boundary_tests;

#[cfg(test)]
#[path = "cursor_tests.rs"]
mod tests;
