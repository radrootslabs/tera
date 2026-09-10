use radroots_transport::source::{FETCH_CURSOR_MAX_BYTES, FetchCursor};
use sha2::{Digest, Sha256};

use super::super::CursorError;

const PREFIX: &str = "ttbf1:";
const VERSION: u8 = 1;
const HEADER_BYTES: usize = 1 + 32 + 32 + 1 + 2;
const DIGEST_BYTES: usize = 32;
const _: () = assert!(FETCH_CURSOR_MAX_BYTES <= u16::MAX as usize);
pub(super) const MAX_BYTES: usize =
    PREFIX.len() + 2 * (HEADER_BYTES + FETCH_CURSOR_MAX_BYTES + DIGEST_BYTES);

pub(super) struct BackfillCursor {
    query_scope: [u8; 32],
    store_generation: [u8; 32],
    pub(super) had_incomplete_responses: bool,
    shared: FetchCursor,
}

impl BackfillCursor {
    pub(super) fn encode(
        shared: &FetchCursor,
        query_scope: [u8; 32],
        store_generation: [u8; 32],
        had_incomplete_responses: bool,
    ) -> String {
        let mut bytes = Vec::with_capacity(HEADER_BYTES + shared.as_str().len() + DIGEST_BYTES);
        bytes.push(VERSION);
        bytes.extend_from_slice(&query_scope);
        bytes.extend_from_slice(&store_generation);
        bytes.push(u8::from(had_incomplete_responses));
        // FetchCursor's validated 2048-byte maximum fits this length field.
        bytes.extend_from_slice(&(shared.as_str().len() as u16).to_be_bytes());
        bytes.extend_from_slice(shared.as_str().as_bytes());
        let checksum = digest(&bytes);
        bytes.extend_from_slice(&checksum);
        format!("{PREFIX}{}", hex::encode(bytes))
    }

    pub(super) fn decode(value: &str) -> Result<Self, CursorError> {
        // Check before prefix scans, hex allocation and every store read.
        if value.len() > MAX_BYTES {
            return Err(CursorError::Malformed);
        }
        let encoded = value.strip_prefix(PREFIX).ok_or(CursorError::Version)?;
        if encoded.len() < 2 * (HEADER_BYTES + DIGEST_BYTES)
            || encoded.len() % 2 != 0
            || !encoded
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(CursorError::Malformed);
        }
        let bytes = hex::decode(encoded).map_err(|_| CursorError::Malformed)?;
        let (payload, checksum) = bytes.split_at(bytes.len() - DIGEST_BYTES);
        if digest(payload).as_slice() != checksum {
            return Err(CursorError::Integrity);
        }
        if payload[0] != VERSION {
            return Err(CursorError::Version);
        }
        let shared_len = usize::from(u16::from_be_bytes([payload[66], payload[67]]));
        if shared_len > FETCH_CURSOR_MAX_BYTES || payload.len() != HEADER_BYTES + shared_len {
            return Err(CursorError::Malformed);
        }
        let had_incomplete_responses = match payload[65] {
            0 => false,
            1 => true,
            _ => return Err(CursorError::Malformed),
        };
        let mut query_scope = [0; 32];
        query_scope.copy_from_slice(&payload[1..33]);
        let mut store_generation = [0; 32];
        store_generation.copy_from_slice(&payload[33..65]);
        let shared =
            std::str::from_utf8(&payload[HEADER_BYTES..]).map_err(|_| CursorError::Malformed)?;
        let shared = FetchCursor::parse(shared).map_err(|_| CursorError::Malformed)?;
        Ok(Self {
            query_scope,
            store_generation,
            had_incomplete_responses,
            shared,
        })
    }

    pub(super) fn validate(
        self,
        query_scope: [u8; 32],
        store_generation: [u8; 32],
    ) -> Result<FetchCursor, CursorError> {
        if self.query_scope != query_scope {
            return Err(CursorError::ContextMismatch);
        }
        if self.store_generation != store_generation {
            return Err(CursorError::Stale);
        }
        Ok(self.shared)
    }
}

fn digest(bytes: &[u8]) -> [u8; 32] {
    let mut hash = Sha256::new();
    hash.update(b"tera.today-backfill-cursor.v1\0");
    hash.update(bytes);
    hash.finalize().into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_maximum_round_trips_and_excess_or_corruption_fails_closed() {
        let shared = FetchCursor::parse("x".repeat(FETCH_CURSOR_MAX_BYTES)).unwrap();
        let encoded = BackfillCursor::encode(&shared, [1; 32], [2; 32], true);
        assert_eq!(encoded.len(), MAX_BYTES);
        let decoded = BackfillCursor::decode(&encoded).unwrap();
        assert!(decoded.had_incomplete_responses);
        assert_eq!(decoded.validate([1; 32], [2; 32]).unwrap(), shared);
        for invalid in [
            format!("{encoded}0"),
            "x".repeat(MAX_BYTES + 1),
            encoded.to_uppercase(),
            format!("{PREFIX}0"),
            encoded.replace(PREFIX, "ttbf2:"),
        ] {
            assert!(BackfillCursor::decode(&invalid).is_err());
        }
        let mut corrupted = encoded.into_bytes();
        corrupted[PREFIX.len() + 2] = b'f';
        assert!(matches!(
            BackfillCursor::decode(std::str::from_utf8(&corrupted).unwrap()),
            Err(CursorError::Integrity)
        ));
    }

    #[test]
    fn context_and_store_binding_are_independent_of_cursor_integrity() {
        let shared = FetchCursor::parse("opaque-shared-position").unwrap();
        let cursor = BackfillCursor::encode(&shared, [1; 32], [2; 32], false);
        assert!(
            !BackfillCursor::decode(&cursor)
                .unwrap()
                .had_incomplete_responses
        );
        assert!(matches!(
            BackfillCursor::decode(&cursor)
                .unwrap()
                .validate([3; 32], [2; 32]),
            Err(CursorError::ContextMismatch)
        ));
        assert!(matches!(
            BackfillCursor::decode(&cursor)
                .unwrap()
                .validate([1; 32], [3; 32]),
            Err(CursorError::Stale)
        ));
    }

    #[test]
    fn valid_checksums_do_not_admit_invalid_payload_fields() {
        let shared = FetchCursor::parse("opaque").unwrap();
        let cursor = BackfillCursor::encode(&shared, [1; 32], [2; 32], false);
        let mut payload = hex::decode(cursor.strip_prefix(PREFIX).unwrap()).unwrap();
        payload.truncate(payload.len() - DIGEST_BYTES);
        for (offset, value) in [(0, 2), (65, 2), (66, 255), (67, 0), (HEADER_BYTES, 255)] {
            let mut invalid = payload.clone();
            invalid[offset] = value;
            let checksum = digest(&invalid);
            invalid.extend_from_slice(&checksum);
            let encoded = format!("{PREFIX}{}", hex::encode(invalid));
            assert!(BackfillCursor::decode(&encoded).is_err());
        }
        payload.truncate(HEADER_BYTES);
        payload[66] = 0;
        payload[67] = 0;
        let checksum = digest(&payload);
        payload.extend_from_slice(&checksum);
        assert!(BackfillCursor::decode(&format!("{PREFIX}{}", hex::encode(payload))).is_err());
    }
}
