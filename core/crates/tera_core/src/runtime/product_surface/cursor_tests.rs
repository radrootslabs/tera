use super::*;

fn scope() -> CursorScope {
    CursorScope::new(
        "nearby".into(),
        4,
        2_000_000_000,
        [7; 32],
        9,
        [6; 32],
        crate::runtime::product_surface::ViewerCalendarContext::new(2_000_000_000, "UTC")
            .expect("calendar"),
    )
    .expect("scope")
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
        "rrtc3:00030001000100066e6561726279000000000000000400000000773594000707070707070707070707070707070707070707070707070707070707070707000000000000000902030000000077359018aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa06060606060606060606060606060606060606060606060606060606060606060001000355544307f1051236c4fa496fbcb8306b555d824b0734477d9a7a91e64a4ca5895e76e1ae6a9f4a"
    );
    assert_eq!(
        TodayCursor::decode(cursor.as_str(), &scope()).expect("decode"),
        position()
    );
    assert_eq!(TodayCursor::scope(cursor.as_str()).expect("scope"), scope());
}

#[test]
fn old_unbound_cursor_is_typed_unsupported_and_query_scope_is_checked() {
    let old = "rrtc1:00010001000100066e6561726279000000000000000400000000773594000707070707070707070707070707070707070707070707070707070707070707000000000000000902030000000077359018aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaedf305be41633dfc2f7d621e067c3d33a71c3548c6a1fcf68a6707a1d8664b11";
    assert_eq!(TodayCursor::scope(old), Err(CursorError::Version));
    assert_eq!(
        TodayCursor::decode(old, &scope()),
        Err(CursorError::Version)
    );
    let cursor = TodayCursor::encode(&scope(), position()).unwrap();
    let mut changed = scope();
    changed.query_scope[0] ^= 1;
    assert_eq!(
        TodayCursor::decode(cursor.as_str(), &changed),
        Err(CursorError::ContextMismatch)
    );
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
    let other_context = CursorScope::new(
        "other".into(),
        4,
        2_000_000_000,
        [7; 32],
        9,
        [6; 32],
        crate::runtime::product_surface::ViewerCalendarContext::new(2_000_000_000, "UTC")
            .expect("calendar"),
    )
    .expect("scope");
    assert_eq!(
        TodayCursor::decode(cursor.as_str(), &other_context),
        Err(CursorError::ContextMismatch)
    );
    let other_context_generation = CursorScope::new(
        "nearby".into(),
        5,
        2_000_000_000,
        [7; 32],
        9,
        [6; 32],
        crate::runtime::product_surface::ViewerCalendarContext::new(2_000_000_000, "UTC")
            .expect("calendar"),
    )
    .expect("scope");
    assert_eq!(
        TodayCursor::decode(cursor.as_str(), &other_context_generation),
        Err(CursorError::ContextMismatch)
    );
    let other_snapshot = CursorScope::new(
        "nearby".into(),
        4,
        2_000_000_001,
        [7; 32],
        9,
        [6; 32],
        crate::runtime::product_surface::ViewerCalendarContext::new(2_000_000_001, "UTC")
            .expect("calendar"),
    )
    .expect("scope");
    assert_eq!(
        TodayCursor::decode(cursor.as_str(), &other_snapshot),
        Err(CursorError::SnapshotMismatch)
    );
    let stale = CursorScope::new(
        "nearby".into(),
        4,
        2_000_000_000,
        [8; 32],
        9,
        [6; 32],
        crate::runtime::product_surface::ViewerCalendarContext::new(2_000_000_000, "UTC")
            .expect("calendar"),
    )
    .expect("scope");
    assert_eq!(
        TodayCursor::decode(cursor.as_str(), &stale),
        Err(CursorError::Stale)
    );
    let stale_projection = CursorScope::new(
        "nearby".into(),
        4,
        2_000_000_000,
        [7; 32],
        10,
        [6; 32],
        crate::runtime::product_surface::ViewerCalendarContext::new(2_000_000_000, "UTC")
            .expect("calendar"),
    )
    .expect("scope");
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
    for malformed in ["rrtc3:0", "rrtc3:GG", "rrtc3:00"] {
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
    assert!(
        CursorScope::new(
            "".into(),
            0,
            0,
            [0; 32],
            0,
            [6; 32],
            crate::runtime::product_surface::ViewerCalendarContext::new(1, "UTC")
                .expect("calendar")
        )
        .is_err()
    );
    assert!(
        CursorScope::new(
            "x".repeat(257),
            0,
            0,
            [0; 32],
            0,
            [6; 32],
            crate::runtime::product_surface::ViewerCalendarContext::new(1, "UTC")
                .expect("calendar")
        )
        .is_err()
    );
    assert!(
        CursorScope::new(
            " nearby ".into(),
            0,
            0,
            [0; 32],
            0,
            [6; 32],
            crate::runtime::product_surface::ViewerCalendarContext::new(1, "UTC")
                .expect("calendar")
        )
        .is_err()
    );
    assert!(
        CursorScope::new(
            "near\u{7f}by".into(),
            0,
            0,
            [0; 32],
            0,
            [6; 32],
            crate::runtime::product_surface::ViewerCalendarContext::new(1, "UTC")
                .expect("calendar")
        )
        .is_err()
    );
    let cursor = TodayCursor::encode(&scope(), position()).expect("cursor");
    for version_offset in [1, 3, 5] {
        let mut unsupported = payload(&cursor);
        unsupported[version_offset] = 99;
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
    truncated_field[1] = 3;
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
