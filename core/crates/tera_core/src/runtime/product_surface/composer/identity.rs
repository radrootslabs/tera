use radroots_identity::PublicKey;
use radroots_storage::authored_draft::{AuthoredDraftId, AuthoredDraftRevision};
use serde::{Deserialize, Serialize};

use super::ComposerError;
use crate::runtime::product_surface::LocalNetworkId;

/// Persistent editing identity. It is never an authored operation or command ID.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Hash, PartialEq, Serialize)]
#[serde(try_from = "[u8; 16]", into = "[u8; 16]")]
pub struct ComposerId(AuthoredDraftId);

impl ComposerId {
    /// Reserves editing identity before the first save, independently of operation IDs.
    pub fn generate() -> Result<Self, ComposerError> {
        Self::new(*uuid::Uuid::new_v4().as_bytes())
    }

    pub fn new(bytes: [u8; 16]) -> Result<Self, ComposerError> {
        AuthoredDraftId::new(bytes)
            .map(Self)
            .map_err(|_| ComposerError::InvalidIdentity)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        self.0.as_bytes()
    }
}

impl TryFrom<[u8; 16]> for ComposerId {
    type Error = ComposerError;
    fn try_from(value: [u8; 16]) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ComposerId> for [u8; 16] {
    fn from(value: ComposerId) -> Self {
        *value.as_bytes()
    }
}

/// Revision representable by the actual shared SQLite owner, with checked increment.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct ComposerRevision(AuthoredDraftRevision);

impl ComposerRevision {
    pub const INITIAL: Self = Self(AuthoredDraftRevision::INITIAL);
    pub const MAX: u64 = i64::MAX as u64;

    pub fn new(value: u64) -> Result<Self, ComposerError> {
        if value > Self::MAX {
            return Err(ComposerError::InvalidRevision);
        }
        AuthoredDraftRevision::new(value)
            .map(Self)
            .map_err(|_| ComposerError::InvalidRevision)
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub fn next(self) -> Result<Self, ComposerError> {
        self.0
            .next()
            .map_err(|_| ComposerError::InvalidRevision)
            .and_then(|value| Self::new(value.get()))
    }
}

impl TryFrom<u64> for ComposerRevision {
    type Error = ComposerError;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ComposerRevision> for u64 {
    fn from(value: ComposerRevision) -> Self {
        value.get()
    }
}

/// Client editing order, independent of durable revision and transient generation.
#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(try_from = "u64", into = "u64")]
pub struct ComposerEditSequence(core::num::NonZeroU64);

impl ComposerEditSequence {
    pub const INITIAL: Self = Self(core::num::NonZeroU64::MIN);

    pub fn new(value: u64) -> Result<Self, ComposerError> {
        core::num::NonZeroU64::new(value)
            .map(Self)
            .ok_or(ComposerError::InvalidEditSequence)
    }

    pub const fn get(self) -> u64 {
        self.0.get()
    }

    pub fn next(self) -> Result<Self, ComposerError> {
        self.get()
            .checked_add(1)
            .ok_or(ComposerError::InvalidEditSequence)
            .and_then(Self::new)
    }
}

impl TryFrom<u64> for ComposerEditSequence {
    type Error = ComposerError;
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        Self::new(value)
    }
}

impl From<ComposerEditSequence> for u64 {
    fn from(value: ComposerEditSequence) -> Self {
        value.get()
    }
}

/// Stable local account/context. Session generations and mutable relay lists are absent.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ComposerScope {
    author: PublicKey,
    local_network: LocalNetworkId,
}

impl ComposerScope {
    pub fn new(author: PublicKey, local_network: LocalNetworkId) -> Self {
        Self {
            author,
            local_network,
        }
    }

    pub const fn author(&self) -> PublicKey {
        self.author
    }
    pub fn local_network(&self) -> &LocalNetworkId {
        &self.local_network
    }
}
