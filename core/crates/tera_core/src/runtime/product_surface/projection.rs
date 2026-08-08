use radroots_event::food::availability::FoodAvailabilityStatus;
use radroots_event_codec::{
    admission::RadrootsAdmittedEvent, decode::post::RadrootsPostClassification,
};
use serde::{Deserialize, Serialize};

use super::{
    CardId, CardLifecycleState, CardSourceIdentity, ClassifiedCard, ContextAdmission,
    LocalNetworkAdmission, MediaReference, MediaVerificationState, SupportingProfile,
    TodayCardType,
};

const CLASSIFIED_CARD_SCHEMA_VERSION: u16 = 1;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProductEventClassification {
    Card(Box<ClassifiedCard>),
    Supporting(SupportingProfile),
    Excluded(ProductEventExclusion),
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum ProductEventExclusion {
    LocalityNonmatch,
    UnsupportedProfile,
    InvalidSourceIdentity,
}

/// Classifies an already signature/id-verified and standard-profile-admitted event.
///
/// The caller must supply the result of the selected LocalNetwork admission.
/// Replacement and deletion are applied by storage before the event reaches
/// this boundary. No content prose or remote media retrieval influences type.
pub fn classify_admitted_event(
    admitted: &RadrootsAdmittedEvent,
    context: LocalNetworkAdmission,
) -> ProductEventClassification {
    let context = match context {
        LocalNetworkAdmission::Included(context) => context,
        LocalNetworkAdmission::Excluded { .. } => {
            return ProductEventClassification::Excluded(ProductEventExclusion::LocalityNonmatch);
        }
    };

    match admitted {
        RadrootsAdmittedEvent::Profile(_) => supporting(SupportingProfile::Profile),
        RadrootsAdmittedEvent::Reply(_) => supporting(SupportingProfile::Reply),
        RadrootsAdmittedEvent::Comment(_) => supporting(SupportingProfile::Comment),
        RadrootsAdmittedEvent::DeletionRequest(_) => supporting(SupportingProfile::Deletion),
        RadrootsAdmittedEvent::RootPost(event) => {
            let card_type = match event.projection().classification() {
                RadrootsPostClassification::Update => TodayCardType::Update,
                RadrootsPostClassification::PhotoUpdate => TodayCardType::PhotoUpdate,
                RadrootsPostClassification::Ask => TodayCardType::Ask,
                RadrootsPostClassification::ThreadExcluded => {
                    return ProductEventClassification::Excluded(
                        ProductEventExclusion::UnsupportedProfile,
                    );
                }
                _ => {
                    return ProductEventClassification::Excluded(
                        ProductEventExclusion::UnsupportedProfile,
                    );
                }
            };
            card(
                admitted,
                card_type,
                context,
                post_media(event.projection()),
                CardLifecycleState::Active,
            )
        }
        RadrootsAdmittedEvent::FoodAvailability(event) => {
            let lifecycle = match event.projection().status() {
                FoodAvailabilityStatus::Active => CardLifecycleState::Active,
                FoodAvailabilityStatus::Sold => CardLifecycleState::Sold,
            };
            card(
                admitted,
                TodayCardType::FoodAvailability,
                context,
                food_media(event.projection()),
                lifecycle,
            )
        }
        RadrootsAdmittedEvent::ContractValidated(event) => match event.contract_id() {
            "radroots.calendar.date_event.v1" | "radroots.calendar.time_event.v1" => card(
                admitted,
                TodayCardType::Event,
                context,
                calendar_media(event.event().tags_as_vec()),
                CardLifecycleState::Active,
            ),
            _ => ProductEventClassification::Excluded(ProductEventExclusion::UnsupportedProfile),
        },
        _ => ProductEventClassification::Excluded(ProductEventExclusion::UnsupportedProfile),
    }
}

const fn supporting(profile: SupportingProfile) -> ProductEventClassification {
    ProductEventClassification::Supporting(profile)
}

fn card(
    admitted: &RadrootsAdmittedEvent,
    card_type: TodayCardType,
    context: ContextAdmission,
    media: Vec<MediaReference>,
    lifecycle: CardLifecycleState,
) -> ProductEventClassification {
    let event = admitted.event();
    let source = match card_type {
        TodayCardType::Update | TodayCardType::PhotoUpdate | TodayCardType::Ask => {
            CardSourceIdentity::Event(*event.id())
        }
        TodayCardType::Event | TodayCardType::FoodAvailability => {
            let identifier = event
                .tags_as_vec()
                .into_iter()
                .find(|tag| tag.first().map(String::as_str) == Some("d"))
                .and_then(|tag| tag.get(1).cloned());
            let Some(identifier) = identifier else {
                return ProductEventClassification::Excluded(
                    ProductEventExclusion::InvalidSourceIdentity,
                );
            };
            let Ok(source) =
                CardSourceIdentity::address(event.kind_u32(), event.author().to_hex(), identifier)
            else {
                return ProductEventClassification::Excluded(
                    ProductEventExclusion::InvalidSourceIdentity,
                );
            };
            source
        }
    };
    let source_address = match &source {
        CardSourceIdentity::Event(_) => None,
        CardSourceIdentity::Address {
            kind,
            author_pubkey,
            identifier,
        } => Some(format!("{kind}:{author_pubkey}:{identifier}")),
    };
    let tags = event.tags_as_vec();
    let title = tag_value(&tags, &["title", "name"]);
    let location = matches!(
        card_type,
        TodayCardType::Event | TodayCardType::FoodAvailability
    )
    .then(|| tag_value(&tags, &["location"]))
    .flatten();
    let price = matches!(card_type, TodayCardType::FoodAvailability)
        .then(|| tag_values(&tags, "price"))
        .flatten();
    let price_amount = price.as_ref().and_then(|values| values.first().cloned());
    let price_currency = price.as_ref().and_then(|values| values.get(1).cloned());
    let price_unit = matches!(card_type, TodayCardType::FoodAvailability)
        .then(|| tag_value(&tags, &["radroots:price_unit"]))
        .flatten();
    let quantity = matches!(card_type, TodayCardType::FoodAvailability)
        .then(|| tag_values(&tags, "radroots:quantity"))
        .flatten()
        .and_then(|values| values.first().cloned());
    let (effective_at, event_start, event_end) = match card_type {
        TodayCardType::Event => {
            let start = tag_time(&tags, "start").unwrap_or_else(|| event.created_at_u64());
            (start, Some(start), tag_time(&tags, "end"))
        }
        TodayCardType::FoodAvailability => (
            tag_time(&tags, "published_at").unwrap_or_else(|| event.created_at_u64()),
            None,
            None,
        ),
        TodayCardType::Update | TodayCardType::PhotoUpdate | TodayCardType::Ask => {
            (event.created_at_u64(), None, None)
        }
    };
    ProductEventClassification::Card(Box::new(ClassifiedCard {
        schema_version: CLASSIFIED_CARD_SCHEMA_VERSION,
        card_id: CardId::derive(card_type, &source),
        card_type,
        source_event_id: event.id_hex(),
        source_address,
        author_pubkey: event.author().to_hex(),
        contract_id: admitted.contract_id().to_owned(),
        title,
        content: event.content().to_owned(),
        authored_at: event.created_at_u64(),
        effective_at,
        event_start,
        event_end,
        location,
        price_amount,
        price_currency,
        price_unit,
        quantity,
        context_rank: context.rank,
        inclusion_reason: context.reason.to_owned(),
        media,
        lifecycle,
        rank: None,
    }))
}

fn tag_value(tags: &[Vec<String>], names: &[&str]) -> Option<String> {
    tags.iter().find_map(|tag| {
        names
            .contains(&tag.first()?.as_str())
            .then(|| tag.get(1).cloned())
            .flatten()
    })
}

fn tag_values(tags: &[Vec<String>], name: &str) -> Option<Vec<String>> {
    tags.iter().find_map(|tag| {
        (tag.first().map(String::as_str) == Some(name) && tag.len() > 1).then(|| tag[1..].to_vec())
    })
}

fn tag_time(tags: &[Vec<String>], name: &str) -> Option<u64> {
    let value = tag_value(tags, &[name])?;
    value.parse().ok().or_else(|| {
        chrono::NaiveDate::parse_from_str(&value, "%Y-%m-%d")
            .ok()?
            .and_hms_opt(0, 0, 0)?
            .and_utc()
            .timestamp()
            .try_into()
            .ok()
    })
}

fn post_media(
    projection: &radroots_event_codec::decode::post::RadrootsInboundPostProjection,
) -> Vec<MediaReference> {
    projection
        .imeta()
        .iter()
        .filter_map(|media| {
            let dimensions = media.dimensions();
            Some(MediaReference {
                url: media.url()?.to_owned(),
                sha256: media.sha256().map(str::to_owned),
                media_type: media.media_type().map(str::to_owned),
                width: dimensions.map(|value| value.width()),
                height: dimensions.map(|value| value.height()),
                byte_size: media.size(),
                alt: media.alt().map(str::to_owned),
                verification: MediaVerificationState::Unavailable,
            })
        })
        .collect()
}

fn food_media(
    projection: &radroots_event_codec::decode::food_availability::RadrootsInboundFoodAvailabilityProjection,
) -> Vec<MediaReference> {
    projection
        .images()
        .iter()
        .filter_map(|media| {
            let dimensions = media.dimensions();
            Some(MediaReference {
                url: media.url()?.to_owned(),
                sha256: blossom_digest(media.url()?),
                media_type: None,
                width: dimensions.map(|value| value.width()),
                height: dimensions.map(|value| value.height()),
                byte_size: None,
                alt: None,
                verification: MediaVerificationState::Unavailable,
            })
        })
        .collect()
}

fn calendar_media(tags: Vec<Vec<String>>) -> Vec<MediaReference> {
    tags.into_iter()
        .find(|tag| tag.first().map(String::as_str) == Some("image"))
        .and_then(|tag| tag.get(1).cloned())
        .map(|url| MediaReference {
            sha256: blossom_digest(&url),
            url,
            media_type: None,
            width: None,
            height: None,
            byte_size: None,
            alt: None,
            verification: MediaVerificationState::Unavailable,
        })
        .into_iter()
        .collect()
}

fn blossom_digest(url: &str) -> Option<String> {
    let path = url.split_once("://")?.1.split_once('/')?.1;
    let candidate = path.split(['.', '/', '?', '#']).next()?;
    (candidate.len() == 64
        && candidate
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)))
    .then(|| candidate.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nostr::secp256k1::Message;
    use nostr::{Keys, SECP256K1};
    use radroots_event::{
        Event, envelope::EventEnvelopeParts, wire::compute_canonical_nip01_event_id,
    };
    use radroots_event_codec::{admission::admit_verified_event, verify::verify_nip01_event};

    const SECRET: &str = "10c5304d6c9ae3a1a16f7860f1cc8f5e3a76225a2663b3a989a0d775919b7df5";

    fn admitted(kind: u32, tags: Vec<Vec<&str>>, content: &str) -> RadrootsAdmittedEvent {
        admitted_owned(
            kind,
            tags.into_iter()
                .map(|tag| tag.into_iter().map(str::to_owned).collect())
                .collect(),
            content,
        )
    }

    fn admitted_owned(kind: u32, tags: Vec<Vec<String>>, content: &str) -> RadrootsAdmittedEvent {
        let keys = Keys::parse(SECRET).expect("key");
        let author = keys.public_key().to_string();
        let created_at = 2_000_000_000;
        let id = compute_canonical_nip01_event_id(&author, created_at, kind, &tags, content)
            .expect("id");
        let message = Message::from_digest(*id.as_bytes());
        let signature = SECP256K1.sign_schnorr_no_aux_rand(
            &message,
            &nostr::secp256k1::Keypair::from_secret_key(SECP256K1, keys.secret_key()),
        );
        let event = Event::new(EventEnvelopeParts {
            id: id.to_hex(),
            author,
            created_at,
            kind,
            tags,
            content: content.into(),
            sig: signature.to_string(),
        })
        .expect("event");
        let verified = verify_nip01_event(event).expect("verified");
        admit_verified_event(verified).expect("admitted")
    }

    const fn context() -> LocalNetworkAdmission {
        LocalNetworkAdmission::Included(ContextAdmission {
            rank: super::super::ContextRank::MissingLocalityFallback,
            reason: "locality_missing_fallback",
        })
    }

    fn card_type(event: RadrootsAdmittedEvent) -> TodayCardType {
        match classify_admitted_event(&event, context()) {
            ProductEventClassification::Card(card) => card.card_type,
            other => panic!("expected card, got {other:?}"),
        }
    }

    fn card(event: RadrootsAdmittedEvent) -> ClassifiedCard {
        match classify_admitted_event(&event, context()) {
            ProductEventClassification::Card(card) => *card,
            other => panic!("expected card, got {other:?}"),
        }
    }

    #[test]
    fn exact_five_card_classifier_precedence_is_protocol_structural() {
        assert_eq!(
            card_type(admitted(1, vec![], "ordinary note")),
            TodayCardType::Update
        );
        let digest_tag = format!("x {}", "a".repeat(64));
        assert_eq!(
            card_type(admitted(
                1,
                vec![vec![
                    "imeta",
                    "url https://media.example/a.jpg",
                    &digest_tag,
                    "m image/jpeg",
                    "dim 10x20",
                    "size 123",
                    "alt field photo"
                ]],
                "photo https://media.example/a.jpg",
            )),
            TodayCardType::PhotoUpdate
        );
        assert_eq!(
            card_type(admitted(
                1,
                vec![vec!["t", " RADROOTS-ASK "], vec!["imeta", "broken"]],
                "Anyone have carrots",
            )),
            TodayCardType::Ask
        );
        assert_eq!(
            card_type(admitted(
                31_923,
                vec![
                    vec!["d", "market-2026"],
                    vec!["title", "Saturday market"],
                    vec!["start", "2000000100"],
                    vec!["end", "2000000200"],
                    vec!["D", "23148"],
                ],
                "Farm market",
            )),
            TodayCardType::Event
        );
        assert_eq!(
            card_type(admitted(
                30_402,
                vec![
                    vec!["d", "carrots"],
                    vec!["title", "Carrots"],
                    vec!["summary", "Fresh bunches"],
                    vec!["published_at", "1999999999"],
                    vec!["location", "Saanich"],
                    vec!["price", "3", "CAD"],
                    vec!["radroots:price_unit", "lb"],
                    vec!["status", "active"],
                ],
                "Carrots available",
            )),
            TodayCardType::FoodAvailability
        );
    }

    #[test]
    fn event_and_food_cards_preserve_required_rendering_fields() {
        let event = card(admitted(
            31_923,
            vec![
                vec!["d", "market-2026"],
                vec!["title", "Saturday market"],
                vec!["start", "2000000100"],
                vec!["D", "23148"],
                vec!["location", "Town square"],
            ],
            "Farm market",
        ));
        assert_eq!(event.location.as_deref(), Some("Town square"));
        assert_eq!(event.price_amount, None);

        let food = card(admitted(
            30_402,
            vec![
                vec!["d", "carrots"],
                vec!["title", "Carrots"],
                vec!["summary", "Fresh bunches"],
                vec!["published_at", "1999999999"],
                vec!["location", "Saanich"],
                vec!["price", "3", "CAD"],
                vec!["radroots:price_unit", "lb"],
                vec!["radroots:quantity", "12", "lb"],
                vec!["status", "active"],
            ],
            "Carrots available",
        ));
        assert_eq!(food.location.as_deref(), Some("Saanich"));
        assert_eq!(food.price_amount.as_deref(), Some("3"));
        assert_eq!(food.price_currency.as_deref(), Some("CAD"));
        assert_eq!(food.price_unit.as_deref(), Some("lb"));
        assert_eq!(food.quantity.as_deref(), Some("12"));
    }

    #[test]
    fn ordinary_standard_kind_one_needs_no_product_marker() {
        let event = admitted(1, vec![vec!["t", "gardening"]], "Seedlings are ready");
        let ProductEventClassification::Card(card) = classify_admitted_event(&event, context())
        else {
            panic!("standard kind-1 must remain admitted");
        };
        assert_eq!(card.card_type, TodayCardType::Update);
        assert_eq!(
            card.context_rank,
            super::super::ContextRank::MissingLocalityFallback
        );
    }

    #[test]
    fn supporting_profiles_never_become_cards_and_nonmatches_are_excluded() {
        let profile = admitted(0, vec![], r#"{"name":"Farm"}"#);
        let reply = admitted(1, vec![vec!["e", &"a".repeat(64), "", "root"]], "Reply");
        let author = Keys::parse(SECRET).expect("key").public_key().to_string();
        let root_id = "a".repeat(64);
        let comment = admitted_owned(
            1_111,
            vec![
                vec!["E".into(), root_id.clone(), String::new(), author.clone()],
                vec!["K".into(), "30402".into()],
                vec!["P".into(), author.clone()],
                vec!["e".into(), root_id.clone(), String::new(), author.clone()],
                vec!["k".into(), "30402".into()],
                vec!["p".into(), author],
            ],
            "Comment",
        );
        let deletion = admitted(5, vec![vec!["e", &root_id]], "Superseded");
        for (event, expected) in [
            (profile, SupportingProfile::Profile),
            (reply, SupportingProfile::Reply),
            (comment, SupportingProfile::Comment),
            (deletion, SupportingProfile::Deletion),
        ] {
            assert_eq!(
                classify_admitted_event(&event, context()),
                ProductEventClassification::Supporting(expected)
            );
        }

        let nip98 = admitted(
            27_235,
            vec![
                vec!["u", "https://media.example/upload"],
                vec!["method", "GET"],
            ],
            "{}",
        );
        assert_eq!(
            classify_admitted_event(&nip98, context()),
            ProductEventClassification::Excluded(ProductEventExclusion::UnsupportedProfile)
        );

        let update = admitted(1, vec![], "ordinary note");
        assert_eq!(
            classify_admitted_event(
                &update,
                LocalNetworkAdmission::Excluded {
                    reason: "locality_nonmatch"
                }
            ),
            ProductEventClassification::Excluded(ProductEventExclusion::LocalityNonmatch)
        );
    }

    #[test]
    fn addressable_replacements_keep_stable_card_identity() {
        let first = admitted(
            30_402,
            vec![
                vec!["d", "carrots"],
                vec!["title", "Carrots"],
                vec!["summary", "Fresh"],
                vec!["published_at", "1999999999"],
                vec!["location", "Saanich"],
                vec!["price", "3", "CAD"],
                vec!["radroots:price_unit", "lb"],
                vec!["status", "active"],
            ],
            "First",
        );
        let second = admitted(
            30_402,
            vec![
                vec!["d", "carrots"],
                vec!["title", "Carrots"],
                vec!["summary", "Fresh"],
                vec!["published_at", "1999999999"],
                vec!["location", "Saanich"],
                vec!["price", "4", "CAD"],
                vec!["radroots:price_unit", "lb"],
                vec!["status", "active"],
            ],
            "Second",
        );
        let ids = [first, second].map(|event| match classify_admitted_event(&event, context()) {
            ProductEventClassification::Card(card) => card.card_id,
            other => panic!("expected card, got {other:?}"),
        });
        assert_eq!(ids[0], ids[1]);
    }
}
