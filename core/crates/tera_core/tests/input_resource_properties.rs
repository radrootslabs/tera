#![cfg(feature = "mobile-social")]

use tera_core::runtime::product_surface::{
    CANONICAL_ADD_COMMAND_TYPES, COMPOSER_CONTENT_MAX_BYTES, COMPOSER_FORM_MAX_BYTES,
    COMPOSER_TEXT_MAX_BYTES, CardId, ComposerFormInput, ComposerPartialForm, ContextRank,
    CursorScope, ProfileMetadataCommand, TODAY_RANK_SCHEMA_VERSION, TodayCursor,
    TodayCursorPosition, TodayRank, ViewerCalendarContext,
};

const UNICODE: [&str; 6] = ["a", "é", "e\u{301}", "👩🏽‍🌾", "\u{202e}ab", "\0\"\\\n"];

#[test]
fn used_profile_fields_preserve_unicode_maxima_and_reject_every_maximum_plus_one() {
    for sample in &UNICODE[..4] {
        let values = [
            bytes_at_limit(sample, 256),
            bytes_at_limit(sample, 512),
            bytes_at_limit(sample, 8192),
        ];
        let command = ProfileMetadataCommand::new(
            values[0].clone(),
            Some(values[1].clone()),
            Some(values[2].clone()),
            None,
            None,
            None,
            Some(false),
        )
        .unwrap();
        assert_eq!(command.authored().name(), values[0]);
        assert_eq!(command.authored().display_name(), Some(values[1].as_str()));
        assert_eq!(command.authored().about(), Some(values[2].as_str()));
        for field in 0..3 {
            let mut excessive = values.clone();
            excessive[field].push('a');
            let error = ProfileMetadataCommand::new(
                excessive[0].clone(),
                Some(excessive[1].clone()),
                Some(excessive[2].clone()),
                None,
                None,
                None,
                None,
            )
            .unwrap_err();
            assert_eq!(
                error.code(),
                [
                    "invalid_profile_name",
                    "invalid_profile_display_name",
                    "invalid_profile_about"
                ][field]
            );
        }
    }
    for field in 0..3 {
        let mut values = [
            "grower".to_owned(),
            "Display".to_owned(),
            "Biography".to_owned(),
        ];
        values[field].push('\0');
        assert!(
            ProfileMetadataCommand::new(
                values[0].clone(),
                Some(values[1].clone()),
                Some(values[2].clone()),
                None,
                None,
                None,
                None
            )
            .is_err()
        );
    }
}

fn bytes_at_limit(sample: &str, maximum: usize) -> String {
    let mut value = sample.repeat(maximum / sample.len());
    value.extend(std::iter::repeat_n('a', maximum - value.len()));
    assert_eq!(value.len(), maximum);
    value
}

#[test]
fn every_creation_family_preserves_hostile_unicode_at_declared_byte_limits() {
    for family in CANONICAL_ADD_COMMAND_TYPES {
        for sample in UNICODE {
            let mut input = ComposerFormInput::empty(family);
            input.content = bytes_at_limit(sample, COMPOSER_CONTENT_MAX_BYTES);
            input.title = Some(bytes_at_limit(sample, COMPOSER_TEXT_MAX_BYTES));
            let original = input.clone();
            let admitted = ComposerPartialForm::new(input).unwrap();
            let bytes = admitted.to_json().unwrap();
            assert!(bytes.len() <= COMPOSER_FORM_MAX_BYTES);
            let restored = ComposerPartialForm::from_json(&bytes).unwrap();
            assert_eq!(restored.input(), &original);
            assert_eq!(restored.to_json().unwrap(), bytes);
            let mut too_large = original.clone();
            too_large.content.push('a');
            assert!(ComposerPartialForm::new(too_large).is_err());
            let mut too_large = original;
            too_large.title.as_mut().unwrap().push('a');
            assert!(ComposerPartialForm::new(too_large).is_err());
        }
    }
}

#[test]
fn finite_malformed_wire_corpus_never_retains_an_invalid_editing_value() {
    // Exercise the public bounded decoder, including incomplete UTF-8, JSON
    // escapes and structural bytes. No new fuzzer, dependency or limit.
    let replacements = [0, 0xff, b'"', b'\\', b']', b'}', b'0', b':'];
    for family in CANONICAL_ADD_COMMAND_TYPES {
        let mut input = ComposerFormInput::empty(family);
        input.content = UNICODE.join("/");
        input.title = Some("unfinished e\u{301}".into());
        let baseline = ComposerPartialForm::new(input).unwrap().to_json().unwrap();
        for offset in 0..baseline.len() {
            for byte in replacements {
                let mut mutated = baseline.clone();
                mutated[offset] = byte;
                if let Ok(admitted) = ComposerPartialForm::from_json(&mutated) {
                    assert!(ComposerPartialForm::new(admitted.input().clone()).is_ok());
                    let retained = admitted.to_json().unwrap();
                    assert!(retained.len() <= COMPOSER_FORM_MAX_BYTES);
                    assert_eq!(ComposerPartialForm::from_json(&retained).unwrap(), admitted);
                }
            }
        }
        for end in 0..baseline.len() {
            assert!(ComposerPartialForm::from_json(&baseline[..end]).is_err());
        }
    }
    let oversized = vec![b' '; COMPOSER_FORM_MAX_BYTES + 1];
    assert!(ComposerPartialForm::from_json(&oversized).is_err());
}

#[test]
fn unicode_cursor_contexts_cannot_cross_any_captured_query_or_owner_generation() {
    let position = TodayCursorPosition {
        rank: TodayRank {
            schema_version: TODAY_RANK_SCHEMA_VERSION,
            algorithm_version: 1,
            context_rank: ContextRank::LocalityMatch,
            time_relevance_rank: 4,
            effective_at: u64::MAX,
            card_id: CardId::parse(&"f".repeat(64)).unwrap(),
        },
    };
    for sample in &UNICODE[..4] {
        for bytes in [1, 63, 127, 255, 256] {
            let scope = CursorScope::new(
                bytes_at_limit(sample, bytes),
                u64::MAX,
                2_000_000_000,
                [0xff; 32],
                u64::MAX,
                [1; 32],
                ViewerCalendarContext::new(2_000_000_000, "UTC").unwrap(),
            )
            .unwrap();
            let cursor = TodayCursor::encode(&scope, position).unwrap();
            assert!(cursor.as_str().is_ascii() && cursor.as_str().len() <= 1384);
            assert_eq!(TodayCursor::scope(cursor.as_str()).unwrap(), scope);
            assert_eq!(
                TodayCursor::decode(cursor.as_str(), &scope).unwrap(),
                position
            );
            for mutation in 0..6 {
                let mut other = scope.clone();
                match mutation {
                    0 => other.context_generation -= 1,
                    1 => other.store_generation[0] ^= 1,
                    2 => other.projection_generation -= 1,
                    3 => other.query_scope[0] ^= 1,
                    4 => {
                        other.as_of -= 1;
                        other.calendar = ViewerCalendarContext::new(other.as_of, "UTC").unwrap();
                    }
                    _ => {
                        other.calendar =
                            ViewerCalendarContext::new(other.as_of, "America/Vancouver").unwrap()
                    }
                }
                assert!(TodayCursor::decode(cursor.as_str(), &other).is_err());
            }
            assert_eq!(
                TodayCursor::decode(cursor.as_str(), &scope).unwrap(),
                position
            );
        }
    }
}
