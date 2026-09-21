//! Bounded projection of the original destinations, never transport authority.

use tera_core::runtime::product_surface::{PublicationTargetDetails, PublicationTargetPolicy};

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiPublicationTargetPolicy {
    Any,
    All,
    Quorum { threshold: u16 },
    Required { fingerprints: Vec<String> },
}

#[derive(Clone, Eq, PartialEq, uniffi::Record)]
pub struct FfiPublicationTargetEvidence {
    pub fingerprint: String,
    pub endpoint: String,
    pub attempted: bool,
    pub accepted: bool,
    pub delivered: bool,
    pub rejected: bool,
    pub uncertain: bool,
    pub read_back_observed_at_unix_ms: Option<u64>,
}

#[derive(Clone, Eq, PartialEq, uniffi::Record)]
pub struct FfiPublicationTargetDetails {
    pub requires_delivery: bool,
    pub policy: FfiPublicationTargetPolicy,
    pub targets: Vec<FfiPublicationTargetEvidence>,
    pub read_back_available: bool,
    pub read_back_complete: bool,
}

impl From<&PublicationTargetDetails> for FfiPublicationTargetDetails {
    fn from(value: &PublicationTargetDetails) -> Self {
        Self {
            requires_delivery: value.requires_delivery,
            policy: match &value.policy {
                PublicationTargetPolicy::Any => FfiPublicationTargetPolicy::Any,
                PublicationTargetPolicy::All => FfiPublicationTargetPolicy::All,
                PublicationTargetPolicy::Quorum(threshold) => FfiPublicationTargetPolicy::Quorum {
                    threshold: *threshold,
                },
                PublicationTargetPolicy::Required(fingerprints) => {
                    FfiPublicationTargetPolicy::Required {
                        fingerprints: fingerprints.clone(),
                    }
                }
            },
            targets: value
                .targets
                .iter()
                .map(|target| FfiPublicationTargetEvidence {
                    fingerprint: target.fingerprint.clone(),
                    endpoint: target.endpoint.clone(),
                    attempted: target.attempted,
                    accepted: target.accepted,
                    delivered: target.delivered,
                    rejected: target.rejected,
                    uncertain: target.uncertain,
                    read_back_observed_at_unix_ms: target.read_back_observed_at_unix_ms,
                })
                .collect(),
            read_back_available: value.read_back_available,
            read_back_complete: value.read_back_complete,
        }
    }
}
