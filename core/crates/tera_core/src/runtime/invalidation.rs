//! Scope-bound query invalidations, independent of native presentation sessions.
//!
//! These revisions describe hints, not durable mutation receipts. They are
//! comparable only within the same runtime epoch and domain. A new runtime has
//! a new epoch and requires a snapshot; reconnecting an observer keeps the epoch
//! and revisions. Reading or subscribing never advances a revision.

use std::{num::NonZeroU128, sync::Mutex};

use radroots_identity::PublicKey;
use radroots_storage::event::SourceGeneration;
use uuid::Uuid;

use super::product_surface::LocalNetwork;

/// Explicit entropy boundary for runtime composition, never a durable command ID.
pub fn new_runtime_epoch() -> NonZeroU128 {
    NonZeroU128::new(Uuid::new_v4().as_u128())
        .expect("UUID v4 always contains nonzero version and variant bits")
}

/// Bounded domains sharing one application runtime and its durable query owner.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum InvalidationDomain {
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

impl InvalidationDomain {
    const fn index(self) -> usize {
        match self {
            Self::Initial => 0,
            Self::Identity => 1,
            Self::Settings => 2,
            Self::Profile => 3,
            Self::Today => 4,
            Self::Drafts => 5,
            Self::Relay => 6,
            Self::Media => 7,
            Self::Lifecycle => 8,
        }
    }
}

/// Exhaustion is terminal for comparison and always requires a fresh query.
/// It cannot wrap, suppress a committed effect, or become a false success.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InvalidationRevision(Option<u64>);

impl InvalidationRevision {
    const INITIAL: Self = Self(Some(0));

    pub const fn value(self) -> Option<u64> {
        self.0
    }

    fn advance(&mut self) -> Self {
        self.0 = self.0.and_then(|value| value.checked_add(1));
        *self
    }
}

/// Stable authenticated storage scope, with an optional exact query context.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct InvalidationScope {
    public_key: PublicKey,
    source_generation: SourceGeneration,
    context: Option<LocalNetwork>,
}

impl InvalidationScope {
    pub const fn public_key(&self) -> PublicKey {
        self.public_key
    }

    pub const fn source_generation(&self) -> SourceGeneration {
        self.source_generation
    }

    pub fn context(&self) -> Option<&LocalNetwork> {
        self.context.as_ref()
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimeInvalidation {
    pub scope: InvalidationScope,
    pub epoch: NonZeroU128,
    pub domain: InvalidationDomain,
    pub revision: InvalidationRevision,
    pub entity_id: Option<String>,
}

/// One bounded revision authority per runtime, independent of observer count.
/// No storage, worker, callback or network call occurs while its mutex is held.
pub struct RuntimeInvalidations {
    public_key: PublicKey,
    source_generation: SourceGeneration,
    epoch: NonZeroU128,
    revisions: Mutex<[InvalidationRevision; 9]>,
}

impl RuntimeInvalidations {
    /// The composition owner supplies a fresh epoch; tests may inject exact values.
    pub fn new(
        public_key: PublicKey,
        source_generation: SourceGeneration,
        epoch: NonZeroU128,
    ) -> Self {
        Self {
            public_key,
            source_generation,
            epoch,
            revisions: Mutex::new([InvalidationRevision::INITIAL; 9]),
        }
    }

    /// Returns the current hint revision without causing or authorizing a write.
    pub fn snapshot(
        &self,
        domain: InvalidationDomain,
        context: Option<&LocalNetwork>,
    ) -> RuntimeInvalidation {
        let revision = match self.revisions.lock() {
            Ok(revisions) => revisions[domain.index()],
            Err(_) => InvalidationRevision(None),
        };
        self.record(domain, context, revision, None)
    }

    /// Called after an operation's existing owner reports its outcome. Counter
    /// exhaustion invalidates comparison; it never changes that durable outcome.
    pub fn advance(
        &self,
        domain: InvalidationDomain,
        context: Option<&LocalNetwork>,
        entity_id: Option<String>,
    ) -> RuntimeInvalidation {
        let revision = match self.revisions.lock() {
            Ok(mut revisions) => revisions[domain.index()].advance(),
            Err(_) => InvalidationRevision(None),
        };
        self.record(domain, context, revision, entity_id)
    }

    fn record(
        &self,
        domain: InvalidationDomain,
        context: Option<&LocalNetwork>,
        revision: InvalidationRevision,
        entity_id: Option<String>,
    ) -> RuntimeInvalidation {
        RuntimeInvalidation {
            scope: InvalidationScope {
                public_key: self.public_key,
                source_generation: self.source_generation,
                context: context.cloned(),
            },
            epoch: self.epoch,
            domain,
            revision,
            entity_id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(epoch: u128) -> RuntimeInvalidations {
        RuntimeInvalidations::new(
            PublicKey::from_hex("79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798")
                .unwrap(),
            SourceGeneration::new([4; 32]).unwrap(),
            NonZeroU128::new(epoch).unwrap(),
        )
    }

    fn context(relay: &str) -> LocalNetwork {
        LocalNetwork::new(
            "default".into(),
            "Local network".into(),
            vec![relay.into()],
            None,
            vec![],
            1,
        )
        .unwrap()
    }

    #[test]
    fn domain_revisions_are_monotonic_and_reads_do_not_advance_them() {
        let source = source(1);
        let first = source.advance(InvalidationDomain::Drafts, None, Some("draft".into()));
        source.advance(InvalidationDomain::Media, None, None);
        for _ in 0..32 {
            let snapshot = source.snapshot(InvalidationDomain::Drafts, None);
            assert_eq!(snapshot.epoch, first.epoch);
            assert_eq!(snapshot.revision, first.revision);
            assert_eq!(snapshot.entity_id, None);
        }
        let second = source.advance(InvalidationDomain::Drafts, None, None);
        assert_eq!(first.revision.value(), Some(1));
        assert_eq!(second.revision.value(), Some(2));
        assert_eq!(
            source
                .snapshot(InvalidationDomain::Media, None)
                .revision
                .value(),
            Some(1)
        );
    }

    #[test]
    fn scope_retains_full_context_and_reopened_runtime_changes_only_epoch() {
        let first = source(1);
        let second = source(2);
        let context_a = context("wss://first.example");
        let context_b = context("wss://second.example");
        let a = first.snapshot(InvalidationDomain::Today, Some(&context_a));
        let b = first.snapshot(InvalidationDomain::Today, Some(&context_b));
        let reopened = second.snapshot(InvalidationDomain::Today, Some(&context_a));
        assert_ne!(a.scope, b.scope);
        assert_eq!(a.epoch, b.epoch);
        assert_eq!(a.scope, reopened.scope);
        assert_ne!(a.epoch, reopened.epoch);
        assert_eq!(a.scope.context(), Some(&context_a));
    }

    #[test]
    fn revision_exhaustion_and_poison_cannot_wrap_or_recover_comparison() {
        let source = source(1);
        source.revisions.lock().unwrap()[InvalidationDomain::Today.index()] =
            InvalidationRevision(Some(u64::MAX - 1));
        assert_eq!(
            source
                .advance(InvalidationDomain::Today, None, None)
                .revision
                .value(),
            Some(u64::MAX)
        );
        for _ in 0..2 {
            assert_eq!(
                source
                    .advance(InvalidationDomain::Today, None, None)
                    .revision
                    .value(),
                None
            );
            assert_eq!(
                source
                    .snapshot(InvalidationDomain::Today, None)
                    .revision
                    .value(),
                None
            );
        }
        let _ = std::panic::catch_unwind(|| {
            let _guard = source.revisions.lock().unwrap();
            panic!("poisoned hint authority");
        });
        assert_eq!(
            source
                .advance(InvalidationDomain::Drafts, None, None)
                .revision
                .value(),
            None
        );
        assert_eq!(
            source
                .snapshot(InvalidationDomain::Drafts, None)
                .revision
                .value(),
            None
        );
    }
}
