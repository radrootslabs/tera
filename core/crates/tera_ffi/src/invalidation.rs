//! Versioned owned notification DTOs; durable records remain query-owned.

use tera_core::runtime::invalidation::{InvalidationDomain, RuntimeInvalidation};

use crate::FfiLocalNetworkRecord;

pub const RUNTIME_CHANGE_SCHEMA_VERSION: u16 = 2;

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRuntimeChangeKind {
    Initial,
    Identity,
    Settings,
    Profile,
    Today,
    Drafts,
    Relay,
    Media,
    Lifecycle,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRuntimeChangeScope {
    pub public_key: String,
    pub source_generation: String,
    pub context: Option<FfiLocalNetworkRecord>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiInvalidationRevision {
    Current { value: u64 },
    Exhausted,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRuntimeChangeRecord {
    pub schema_version: u16,
    pub scope: FfiRuntimeChangeScope,
    pub epoch: String,
    pub revision: FfiInvalidationRevision,
    pub kind: FfiRuntimeChangeKind,
    pub entity_id: Option<String>,
}

impl From<RuntimeInvalidation> for FfiRuntimeChangeRecord {
    fn from(value: RuntimeInvalidation) -> Self {
        Self {
            schema_version: RUNTIME_CHANGE_SCHEMA_VERSION,
            scope: FfiRuntimeChangeScope {
                public_key: value.scope.public_key().to_hex(),
                source_generation: hex::encode(value.scope.source_generation().as_bytes()),
                context: value.scope.context().cloned().map(Into::into),
            },
            epoch: format!("{:032x}", value.epoch),
            revision: match value.revision.value() {
                Some(value) => FfiInvalidationRevision::Current { value },
                None => FfiInvalidationRevision::Exhausted,
            },
            kind: value.domain.into(),
            entity_id: value.entity_id,
        }
    }
}

impl From<FfiRuntimeChangeKind> for InvalidationDomain {
    fn from(value: FfiRuntimeChangeKind) -> Self {
        match value {
            FfiRuntimeChangeKind::Initial => Self::Initial,
            FfiRuntimeChangeKind::Identity => Self::Identity,
            FfiRuntimeChangeKind::Settings => Self::Settings,
            FfiRuntimeChangeKind::Profile => Self::Profile,
            FfiRuntimeChangeKind::Today => Self::Today,
            FfiRuntimeChangeKind::Drafts => Self::Drafts,
            FfiRuntimeChangeKind::Relay => Self::Relay,
            FfiRuntimeChangeKind::Media => Self::Media,
            FfiRuntimeChangeKind::Lifecycle => Self::Lifecycle,
        }
    }
}

impl From<InvalidationDomain> for FfiRuntimeChangeKind {
    fn from(value: InvalidationDomain) -> Self {
        match value {
            InvalidationDomain::Initial => Self::Initial,
            InvalidationDomain::Identity => Self::Identity,
            InvalidationDomain::Settings => Self::Settings,
            InvalidationDomain::Profile => Self::Profile,
            InvalidationDomain::Today => Self::Today,
            InvalidationDomain::Drafts => Self::Drafts,
            InvalidationDomain::Relay => Self::Relay,
            InvalidationDomain::Media => Self::Media,
            InvalidationDomain::Lifecycle => Self::Lifecycle,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_domains_and_both_revision_states_round_trip_through_the_generated_codec() {
        for domain in [
            InvalidationDomain::Initial,
            InvalidationDomain::Identity,
            InvalidationDomain::Settings,
            InvalidationDomain::Profile,
            InvalidationDomain::Today,
            InvalidationDomain::Drafts,
            InvalidationDomain::Relay,
            InvalidationDomain::Media,
            InvalidationDomain::Lifecycle,
        ] {
            let kind = FfiRuntimeChangeKind::from(domain);
            assert_eq!(InvalidationDomain::from(kind), domain);
            for revision in [
                FfiInvalidationRevision::Current { value: u64::MAX },
                FfiInvalidationRevision::Exhausted,
            ] {
                let record = FfiRuntimeChangeRecord {
                    schema_version: RUNTIME_CHANGE_SCHEMA_VERSION,
                    scope: FfiRuntimeChangeScope {
                        public_key: "a".repeat(64),
                        source_generation: "b".repeat(64),
                        context: Some(FfiLocalNetworkRecord {
                            schema_version: 1,
                            id: "local".into(),
                            label: "Market".into(),
                            relay_urls: vec!["wss://relay.example".into()],
                            locality: Some("Town".into()),
                            followed_authors: vec!["c".repeat(64)],
                            generation: u64::MAX,
                        }),
                    },
                    epoch: "d".repeat(32),
                    revision,
                    kind,
                    entity_id: Some("draft".into()),
                };
                let mut bytes = Vec::new();
                <FfiRuntimeChangeRecord as uniffi::FfiConverter<crate::UniFfiTag>>::write(
                    record.clone(),
                    &mut bytes,
                );
                let mut input = bytes.as_slice();
                let decoded =
                    <FfiRuntimeChangeRecord as uniffi::FfiConverter<crate::UniFfiTag>>::try_read(
                        &mut input,
                    )
                    .unwrap();
                assert_eq!(decoded, record);
                assert!(input.is_empty());
                let mut truncated = &bytes[..bytes.len() - 1];
                assert!(
                    <FfiRuntimeChangeRecord as uniffi::FfiConverter<crate::UniFfiTag>>::try_read(
                        &mut truncated
                    )
                    .is_err()
                );
            }
        }
    }
}
