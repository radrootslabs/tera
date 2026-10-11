//! Compact scope binding; a digest is integrity, never authorization.
use super::*;
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};

const PREFIX: &str = "rrtc4:";
const DOMAIN: &[u8] = b"tera.today-keyset-cursor.v4\0";
const MAX_BYTES: usize = 1384;

pub(in super::super) struct Decoded {
    pub(in super::super) inner: String,
    pub(super) scope: CursorScope,
    pub(super) rank: TodayRank,
    pub(super) fingerprint: [u8; 32],
    pub(super) thread_after: Option<CardId>,
    pub(super) seen: u64,
}

pub(super) fn encode(
    scope: &CursorScope,
    rank: TodayRank,
    fingerprint: [u8; 32],
    thread_after: Option<CardId>,
    seen: u64,
) -> Result<String, TodayError> {
    let inner = TodayCursor::encode(scope, TodayCursorPosition { rank })?;
    let inner = hex::decode(inner.as_str().strip_prefix("rrtc3:").expect("v3 encoding"))
        .map_err(|_| CursorError::Malformed)?;
    let mut bytes = Vec::with_capacity(inner.len() + 100);
    bytes.extend_from_slice(&(inner.len() as u16).to_be_bytes());
    bytes.extend_from_slice(&inner);
    bytes.extend_from_slice(&fingerprint);
    bytes.extend_from_slice(&seen.to_be_bytes());
    bytes.push(u8::from(thread_after.is_some()));
    if let Some(after) = thread_after {
        bytes.extend_from_slice(after.as_bytes());
    }
    let digest = Sha256::digest([DOMAIN, bytes.as_slice()].concat());
    bytes.extend_from_slice(&digest);
    let value = format!("{PREFIX}{}", URL_SAFE_NO_PAD.encode(bytes));
    if value.len() > MAX_BYTES {
        return Err(CursorError::Malformed.into());
    }
    Ok(value)
}

pub(in super::super) fn decode(value: &str) -> Result<Decoded, CursorError> {
    if value.len() > MAX_BYTES {
        return Err(CursorError::Malformed);
    }
    if value.starts_with("rrtc1:") || value.starts_with("rrtc2:") || value.starts_with("rrtc3:") {
        return Err(CursorError::Version);
    }
    let encoded = value.strip_prefix(PREFIX).ok_or(CursorError::Malformed)?;
    let bytes = URL_SAFE_NO_PAD
        .decode(encoded)
        .map_err(|_| CursorError::Malformed)?;
    if bytes.len() < 2 + 32 + 1 + 32 {
        return Err(CursorError::Malformed);
    }
    let end = bytes.len() - 32;
    if Sha256::digest([DOMAIN, &bytes[..end]].concat()).as_slice() != &bytes[end..] {
        return Err(CursorError::Integrity);
    }
    let inner_len = u16::from_be_bytes([bytes[0], bytes[1]]) as usize;
    let inner_end = 2 + inner_len;
    if inner_end + 41 > end {
        return Err(CursorError::Malformed);
    }
    let fingerprint = bytes[inner_end..inner_end + 32]
        .try_into()
        .expect("bounded digest");
    let seen = u64::from_be_bytes(
        bytes[inner_end + 32..inner_end + 40]
            .try_into()
            .expect("bounded count"),
    );
    let thread_after = match bytes[inner_end + 40] {
        0 if inner_end + 41 == end => None,
        1 if inner_end + 73 == end => Some(
            CardId::parse(&hex::encode(&bytes[inner_end + 41..end]))
                .map_err(|_| CursorError::InvalidPosition)?,
        ),
        _ => return Err(CursorError::Malformed),
    };
    let inner = format!("rrtc3:{}", hex::encode(&bytes[2..inner_end]));
    let scope = TodayCursor::scope(&inner)?;
    let rank = TodayCursor::decode(&inner, &scope)?.rank;
    Ok(Decoded {
        inner,
        scope,
        rank,
        fingerprint,
        thread_after,
        seen,
    })
}
