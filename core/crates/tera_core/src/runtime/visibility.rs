//! Account-local reading preferences, never a remote blocking or deletion claim.

use std::collections::BTreeMap;

use radroots_identity::PublicKey;
use radroots_storage::projection::{
    ProjectionDocument, ProjectionGeneration, ProjectionId, ProjectionStore,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{TeraRuntime, product_surface::TodayError};

pub const AUTHOR_VISIBILITY_MAX_ENTRIES: usize =
    radroots_storage::projection::document_query::PROJECTION_DOCUMENT_QUERY_LIMIT_MAX as usize;
// 64 hex bytes, enum, JSON punctuation and envelope; validate before decoding.
pub const AUTHOR_VISIBILITY_MAX_BYTES: usize = AUTHOR_VISIBILITY_MAX_ENTRIES * 128 + 128;
const ID: &str = "tera.author_visibility.v1";
const KEY: &str = "policy";

#[cfg(test)]
#[path = "visibility_tests.rs"]
mod tests;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorVisibility {
    Visible,
    Muted,
    Blocked,
}

#[derive(Clone, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AuthorVisibilityPolicy {
    schema_version: u16,
    revision: u64,
    entries: BTreeMap<String, AuthorVisibility>,
}

impl Default for AuthorVisibilityPolicy {
    fn default() -> Self {
        Self {
            schema_version: 1,
            revision: 0,
            entries: BTreeMap::new(),
        }
    }
}

impl std::fmt::Debug for AuthorVisibilityPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AuthorVisibilityPolicy")
            .field("revision", &self.revision)
            .field("entry_count", &self.entries.len())
            .finish()
    }
}

impl AuthorVisibilityPolicy {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn entries(&self) -> &BTreeMap<String, AuthorVisibility> {
        &self.entries
    }
    pub fn allows(&self, author: &str) -> bool {
        !self.entries.contains_key(author)
    }

    fn validate(&self) -> Result<(), TodayError> {
        if self.schema_version != 1 {
            return Err(TodayError::UnsupportedProjectionVersion);
        }
        if self.entries.len() > AUTHOR_VISIBILITY_MAX_ENTRIES
            || (self.revision == 0 && !self.entries.is_empty())
            || self
                .entries
                .iter()
                .any(|(key, mode)| !valid_key(key) || *mode == AuthorVisibility::Visible)
        {
            return Err(TodayError::CorruptProjection);
        }
        Ok(())
    }

    /// Bind cached queries to the entire bounded policy without event scans.
    pub(crate) fn cache_digest(&self) -> Result<Option<[u8; 32]>, TodayError> {
        if self.revision == 0 && self.entries.is_empty() {
            return Ok(None);
        }
        let encoded = serde_json::to_vec(self).map_err(|_| TodayError::Serialization)?;
        Ok(Some(Sha256::digest(encoded).into()))
    }

    fn changed(&self, author: &str, mode: AuthorVisibility) -> Result<Self, TodayError> {
        if !valid_key(author) {
            return Err(TodayError::InvalidRequest);
        }
        let mut next = self.clone();
        if mode == AuthorVisibility::Visible {
            next.entries.remove(author);
        } else {
            if !next.entries.contains_key(author)
                && next.entries.len() == AUTHOR_VISIBILITY_MAX_ENTRIES
            {
                return Err(TodayError::InvalidRequest);
            }
            next.entries.insert(author.to_owned(), mode);
        }
        if next.entries != self.entries {
            next.revision = self
                .revision
                .checked_add(1)
                .ok_or(TodayError::InvalidRequest)?;
        }
        Ok(next)
    }

    /// Keep original empty-policy projections readable. Once changed, even an
    /// empty restored policy has a revision and invalidates earlier snapshots.
    pub(crate) fn visibility_digest(&self, events: [u8; 32]) -> Result<[u8; 32], TodayError> {
        if self.revision == 0 && self.entries.is_empty() {
            return Ok(events);
        }
        let mut digest = Sha256::new();
        digest.update(b"tera.author_visibility.digest.v1\0");
        digest.update(events);
        digest.update(serde_json::to_vec(self).map_err(|_| TodayError::Serialization)?);
        Ok(digest.finalize().into())
    }
}

fn valid_key(value: &str) -> bool {
    value.len() == 64 && PublicKey::from_hex(value).is_ok_and(|key| key.to_hex() == value)
}

fn identity() -> Result<(ProjectionId, ProjectionGeneration), TodayError> {
    Ok((
        ProjectionId::parse(ID)?,
        ProjectionGeneration::new(Sha256::digest(ID).into())?,
    ))
}

impl TeraRuntime {
    pub(crate) async fn projection_visibility_digest(&self) -> Result<[u8; 32], TodayError> {
        let policy = self.load_author_visibility().await?;
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let events = radroots_storage::EventStore::rebuild_visibility(storage).await?;
        policy.visibility_digest(*events.digest().as_bytes())
    }

    pub async fn author_visibility(&self) -> Result<AuthorVisibilityPolicy, TodayError> {
        let _command = self.lifecycle.enter()?;
        self.load_author_visibility().await
    }

    /// Serialize the complete read/modify/write with projection changes. An
    /// uncertain dispatched write fences reads and further edits until reopen.
    pub async fn set_author_visibility(
        &self,
        author: &str,
        mode: AuthorVisibility,
    ) -> Result<AuthorVisibilityPolicy, TodayError> {
        let _command = self.lifecycle.enter()?;
        if !valid_key(author) {
            return Err(TodayError::InvalidRequest);
        }
        let _projection = self.today_projection_lock.lock().await;
        let prior = self.load_author_visibility().await?;
        let next = prior.changed(author, mode)?;
        if prior == next {
            return Ok(next);
        }
        let bytes = serde_json::to_vec(&next).map_err(|_| TodayError::Serialization)?;
        if bytes.len() > AUTHOR_VISIBILITY_MAX_BYTES {
            return Err(TodayError::InvalidRequest);
        }
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let (id, generation) = identity()?;
        let document = ProjectionDocument::new(KEY.to_owned(), bytes)?;
        let write = self.author_visibility_fence.begin_write();
        ProjectionStore::put_projection_document(storage, id, generation, document).await?;
        write.complete();
        Ok(next)
    }

    pub(crate) async fn load_author_visibility(
        &self,
    ) -> Result<AuthorVisibilityPolicy, TodayError> {
        if !self.author_visibility_fence.can_collect() {
            return Err(TodayError::RuntimeUnavailable);
        }
        let storage = self
            .client
            .storage()
            .map_err(|_| TodayError::RuntimeUnavailable)?;
        let (id, generation) = identity()?;
        let document =
            ProjectionStore::projection_document(storage, id, generation, KEY.to_owned()).await?;
        let policy = match document {
            None => AuthorVisibilityPolicy::default(),
            Some(document) => {
                if document.value().len() > AUTHOR_VISIBILITY_MAX_BYTES {
                    return Err(TodayError::CorruptProjection);
                }
                serde_json::from_slice::<AuthorVisibilityPolicy>(document.value())
                    .map_err(|_| TodayError::CorruptProjection)?
            }
        };
        policy.validate()?;
        if !self.author_visibility_fence.can_collect() {
            return Err(TodayError::RuntimeUnavailable);
        }
        Ok(policy)
    }
}
