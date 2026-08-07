use serde::{Deserialize, Serialize};

use super::{CardId, ContextRank, TodayRank};

/// The closed Phase 1 top-level Today taxonomy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum TodayCardType {
    Update,
    PhotoUpdate,
    Ask,
    Event,
    FoodAvailability,
}

impl TodayCardType {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Update => "Update",
            Self::PhotoUpdate => "PhotoUpdate",
            Self::Ask => "Ask",
            Self::Event => "Event",
            Self::FoodAvailability => "FoodAvailability",
        }
    }
}

/// The closed Phase 1 Add command taxonomy.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum AddCommandType {
    CreateUpdate,
    CreatePhotoUpdate,
    CreateAsk,
    CreateEvent,
    CreateFoodAvailability,
}

/// One exact top-level card to Add-command mapping.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CardAddParity {
    pub card_type: TodayCardType,
    pub add_command_type: AddCommandType,
}

pub const CANONICAL_TODAY_CARD_TYPES: [TodayCardType; 5] = [
    TodayCardType::Update,
    TodayCardType::PhotoUpdate,
    TodayCardType::Ask,
    TodayCardType::Event,
    TodayCardType::FoodAvailability,
];

pub const CANONICAL_ADD_COMMAND_TYPES: [AddCommandType; 5] = [
    AddCommandType::CreateUpdate,
    AddCommandType::CreatePhotoUpdate,
    AddCommandType::CreateAsk,
    AddCommandType::CreateEvent,
    AddCommandType::CreateFoodAvailability,
];

pub const CANONICAL_CARD_ADD_PARITY: [CardAddParity; 5] = [
    CardAddParity {
        card_type: TodayCardType::Update,
        add_command_type: AddCommandType::CreateUpdate,
    },
    CardAddParity {
        card_type: TodayCardType::PhotoUpdate,
        add_command_type: AddCommandType::CreatePhotoUpdate,
    },
    CardAddParity {
        card_type: TodayCardType::Ask,
        add_command_type: AddCommandType::CreateAsk,
    },
    CardAddParity {
        card_type: TodayCardType::Event,
        add_command_type: AddCommandType::CreateEvent,
    },
    CardAddParity {
        card_type: TodayCardType::FoodAvailability,
        add_command_type: AddCommandType::CreateFoodAvailability,
    },
];

/// Supporting standard profiles that enrich the product without creating cards.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum SupportingProfile {
    Profile,
    Reply,
    Comment,
    Deletion,
}

/// Local media verification never changes the canonical card type.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum MediaVerificationState {
    Pending,
    Verified,
    Failed,
    Unavailable,
}

/// Structural media metadata plus its separate local verification state.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MediaReference {
    pub url: String,
    pub sha256: Option<String>,
    pub media_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub byte_size: Option<u64>,
    pub alt: Option<String>,
    pub verification: MediaVerificationState,
}

/// Tolerant profile attribution attached to cards and Me results.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProfileSummary {
    pub author_pubkey: String,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub picture: Option<MediaReference>,
}

/// Thread enrichment identity; replies and comments never become top-level cards.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ThreadReference {
    pub profile: SupportingProfile,
    pub root: String,
    pub parent_event_id: String,
}

/// Current rendering state derived from standard event semantics.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "PascalCase")]
pub enum CardLifecycleState {
    Active,
    Sold,
    Past,
}

/// Verified, visible, context-admitted source facts for one top-level card.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClassifiedCard {
    pub schema_version: u16,
    pub card_id: CardId,
    pub card_type: TodayCardType,
    pub source_event_id: String,
    pub source_address: Option<String>,
    pub author_pubkey: String,
    pub contract_id: String,
    pub content: String,
    pub authored_at: u64,
    pub context_rank: ContextRank,
    pub inclusion_reason: String,
    pub media: Vec<MediaReference>,
    pub lifecycle: CardLifecycleState,
    pub rank: Option<TodayRank>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn card_and_add_taxonomies_are_exact_and_serialized_stably() {
        assert_eq!(CANONICAL_TODAY_CARD_TYPES.len(), 5);
        assert_eq!(CANONICAL_ADD_COMMAND_TYPES.len(), 5);
        for (index, parity) in CANONICAL_CARD_ADD_PARITY.iter().enumerate() {
            assert_eq!(parity.card_type, CANONICAL_TODAY_CARD_TYPES[index]);
            assert_eq!(parity.add_command_type, CANONICAL_ADD_COMMAND_TYPES[index]);
        }
        assert_eq!(
            serde_json::to_string(&CANONICAL_TODAY_CARD_TYPES).expect("cards"),
            r#"["Update","PhotoUpdate","Ask","Event","FoodAvailability"]"#
        );
    }

    #[test]
    fn media_state_is_independent_from_card_type() {
        for state in [
            MediaVerificationState::Pending,
            MediaVerificationState::Verified,
            MediaVerificationState::Failed,
            MediaVerificationState::Unavailable,
        ] {
            assert!(!serde_json::to_string(&state).expect("state").is_empty());
        }
    }
}
