use super::*;

fn scope(context: &str) -> CursorScope {
    CursorScope::new(
        context.into(),
        u64::MAX,
        2_000_000_000,
        [0xff; 32],
        u64::MAX,
        [0xff; 32],
        crate::runtime::product_surface::ViewerCalendarContext::new(2_000_000_000, "UTC")
            .expect("calendar"),
    )
    .expect("valid scope")
}

fn position() -> TodayCursorPosition {
    TodayCursorPosition {
        rank: TodayRank {
            schema_version: TODAY_RANK_SCHEMA_VERSION,
            algorithm_version: TODAY_RANK_ALGORITHM_VERSION,
            context_rank: ContextRank::LocalityMatch,
            time_relevance_rank: 4,
            effective_at: u64::MAX,
            card_id: CardId::parse(&"f".repeat(64)).expect("card"),
        },
    }
}

fn assert_error(value: &str, error: CursorError) {
    assert_eq!(TodayCursor::scope(value), Err(error));
    assert_eq!(TodayCursor::decode(value, &scope("nearby")), Err(error));
}

fn rehashed(mut payload: Vec<u8>) -> String {
    payload.extend_from_slice(&cursor_digest(&payload));
    format!("{CURSOR_PREFIX}{}", hex::encode(payload))
}

#[test]
fn calendar_payload_version_date_and_zone_are_validated_after_integrity() {
    let cursor = TodayCursor::encode(&scope("nearby"), position()).unwrap();
    let mut payload = hex::decode(&cursor.as_str()[6..]).unwrap();
    payload.truncate(payload.len() - DIGEST_BYTES);
    let calendar_start = 138 + "nearby".len();
    let mut invalid = payload.clone();
    invalid[calendar_start + 1] = 2;
    assert_error(&rehashed(invalid), CursorError::Version);
    let mut invalid = payload.clone();
    *invalid.last_mut().unwrap() = 19;
    assert_error(&rehashed(invalid), CursorError::SnapshotMismatch);
    let mut invalid = payload.clone();
    invalid[calendar_start + 4] = b'?';
    assert_error(&rehashed(invalid), CursorError::Malformed);
    payload[calendar_start + 2..calendar_start + 4].copy_from_slice(&256_u16.to_be_bytes());
    assert_error(&rehashed(payload), CursorError::Malformed);
}

#[test]
fn maximum_cursor_round_trips_ascii_and_multibyte_contexts() {
    assert_eq!(FIXED_PAYLOAD_BYTES, 146);
    assert_eq!(MAX_CURSOR_BYTES, 1384);
    assert_eq!(LEGACY_MAX_CURSOR_BYTES, 794);
    assert_eq!(V2_MAX_CURSOR_BYTES, 858);
    for context in ["x".repeat(256), "é".repeat(128)] {
        let scope = scope(&context);
        let cursor = TodayCursor::encode(&scope, position()).expect("cursor");
        assert!(cursor.as_str().is_ascii());
        assert_eq!(cursor.as_str().len(), 6 + 2 * (146 + 256 + 3 + 32));
        assert!(cursor.as_str().len() <= MAX_CURSOR_BYTES);
        assert_eq!(TodayCursor::scope(cursor.as_str()), Ok(scope.clone()));
        assert_eq!(TodayCursor::decode(cursor.as_str(), &scope), Ok(position()));
    }
}

#[test]
fn oversized_hex_is_rejected_before_integrity_decoding() {
    assert_error(
        &format!("rrtc1:{}", "0".repeat(LEGACY_MAX_CURSOR_BYTES + 1 - 6)),
        CursorError::Malformed,
    );
    // At the cap, well-shaped hex reaches integrity validation. One byte over
    // the cap must fail before allocation, even if the payload would hash badly.
    assert_error(
        &format!("{CURSOR_PREFIX}{}", "0".repeat(MAX_CURSOR_BYTES - 6)),
        CursorError::Integrity,
    );
    assert_error(
        &format!("{CURSOR_PREFIX}{}", "0".repeat(MAX_CURSOR_BYTES + 1 - 6)),
        CursorError::Malformed,
    );
    for (prefix, cap) in [
        ("rrtc1:", LEGACY_MAX_CURSOR_BYTES),
        ("rrtc2:", V2_MAX_CURSOR_BYTES),
    ] {
        assert_error(
            &format!("{prefix}{}", "0".repeat(cap - 6)),
            CursorError::Version,
        );
        assert_error(
            &format!("{prefix}{}", "0".repeat(cap + 1 - 6)),
            CursorError::Malformed,
        );
    }
    // All bytes are valid hex. Without the length gate these reach hashing and
    // return Integrity, after allocating a decoded buffer proportional to input.
    for bytes in [MAX_CURSOR_BYTES + 2, 8 * 1024 * 1024] {
        let malicious = format!("{CURSOR_PREFIX}{}", "0".repeat(bytes - CURSOR_PREFIX.len()));
        assert_eq!(malicious.len(), bytes);
        assert_error(&malicious, CursorError::Malformed);
    }
}

#[test]
fn both_cursor_entry_points_reject_malformed_shapes_and_checksums() {
    for malformed in [
        "", "rrtc3:", "rrtc3:0", "rrtc3:00", "rrtc3:GG", "rrtc3:é", "rrtc4:00",
    ] {
        assert_error(malformed, CursorError::Malformed);
    }
    let cursor = TodayCursor::encode(&scope("nearby"), position()).expect("cursor");
    let encoded = cursor.as_str().strip_prefix(CURSOR_PREFIX).expect("prefix");
    let mut bytes = hex::decode(encoded).expect("hex");
    *bytes.last_mut().expect("digest") ^= 1;
    assert_error(
        &format!("{CURSOR_PREFIX}{}", hex::encode(bytes)),
        CursorError::Integrity,
    );
}

#[test]
fn integrity_valid_payloads_reject_versions_lengths_and_trailing_bytes() {
    let cursor = TodayCursor::encode(&scope("nearby"), position()).expect("cursor");
    let mut payload =
        hex::decode(cursor.as_str().strip_prefix(CURSOR_PREFIX).expect("prefix")).expect("hex");
    payload.truncate(payload.len() - DIGEST_BYTES);
    for offset in [1, 3, 5] {
        let mut invalid = payload.clone();
        invalid[offset] = 99;
        assert_error(&rehashed(invalid), CursorError::Version);
    }
    for length in [257_u16, u16::MAX] {
        let mut invalid = payload.clone();
        invalid[6..8].copy_from_slice(&length.to_be_bytes());
        assert_error(&rehashed(invalid), CursorError::Malformed);
    }
    payload.push(0);
    assert_error(&rehashed(payload), CursorError::Malformed);
}

#[test]
fn scope_construction_and_deserialization_cannot_admit_invalid_contexts() {
    for context in [
        "".into(),
        "x".repeat(257),
        "é".repeat(129),
        " nearby".into(),
        "x\u{7f}".into(),
    ] {
        assert_eq!(
            CursorScope::new(
                context.clone(),
                0,
                0,
                [0; 32],
                0,
                [0; 32],
                crate::runtime::product_surface::ViewerCalendarContext::new(1, "UTC")
                    .expect("calendar")
            ),
            Err(CursorError::InvalidContext)
        );
        let wire = serde_json::to_string(&context).expect("wire");
        assert!(serde_json::from_str::<LocalNetworkId>(&wire).is_err());
    }
    // Public scope fields still require the validated, representation-private ID.
    let id = serde_json::from_str::<LocalNetworkId>("\"nearby\"").expect("validated ID");
    let scope = CursorScope {
        context_id: id,
        context_generation: 0,
        as_of: 1,
        calendar: ViewerCalendarContext::new(1, "UTC").unwrap(),
        store_generation: [0; 32],
        projection_generation: 0,
        query_scope: [0; 32],
    };
    let cursor = TodayCursor::encode(&scope, position()).expect("cursor");
    assert_eq!(TodayCursor::decode(cursor.as_str(), &scope), Ok(position()));
}
