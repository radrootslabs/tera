//! Safe advisory status for one native transfer. It never authorizes an effect.
//! The native receipt and authored operation remain their respective authorities.

use radroots_storage::projection::{
    ProjectionDocument, ProjectionGeneration, ProjectionId, ProjectionStore,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::{Phase1DraftError as E, phase1_operation_now_unix_ms};
use crate::TeraRuntime;

pub const NATIVE_RECOVERY_STATUS_MAX_BYTES: usize = 2048;
const VERSION: u16 = 1;
const DOMAIN: &[u8] = b"tera.native_recovery_status.v1";

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeRecoveryReason {
    MissingParent,
    InvalidParent,
    AssociationMismatch,
    OutcomeUnconfirmed,
    Resolved,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct NativeRecoveryStatus {
    pub key: [u8; 32],
    pub reason: NativeRecoveryReason,
    pub revision: u64,
    pub first_observed_unix_ms: u64,
    pub updated_at_unix_ms: u64,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    schema_version: u16,
    author: [u8; 32],
    key: [u8; 32],
    reason: NativeRecoveryReason,
    revision: u64,
    first_observed_unix_ms: u64,
    updated_at_unix_ms: u64,
}

impl Wire {
    fn status(&self) -> NativeRecoveryStatus {
        NativeRecoveryStatus {
            key: self.key,
            reason: self.reason,
            revision: self.revision,
            first_observed_unix_ms: self.first_observed_unix_ms,
            updated_at_unix_ms: self.updated_at_unix_ms,
        }
    }
}

fn projection(author: [u8; 32]) -> Result<ProjectionId, E> {
    ProjectionId::parse(format!("tera.native_recovery.{}", hex::encode(author)))
        .map_err(|_| E::Corrupt)
}

fn generation() -> Result<ProjectionGeneration, E> {
    ProjectionGeneration::new(Sha256::digest(DOMAIN).into()).map_err(|_| E::Corrupt)
}

fn decode(bytes: &[u8], author: [u8; 32], key: [u8; 32]) -> Result<Wire, E> {
    if bytes.len() > NATIVE_RECOVERY_STATUS_MAX_BYTES {
        return Err(E::Corrupt);
    }
    let value: Wire = serde_json::from_slice(bytes).map_err(|_| E::Corrupt)?;
    if value.schema_version != VERSION
        || value.author != author
        || value.key != key
        || !(1..=i64::MAX as u64).contains(&value.revision)
        || !(1..=i64::MAX as u64).contains(&value.first_observed_unix_ms)
        || !(value.first_observed_unix_ms..=i64::MAX as u64).contains(&value.updated_at_unix_ms)
        || serde_json::to_vec(&value).map_err(|_| E::Corrupt)? != bytes
    {
        return Err(E::Corrupt);
    }
    Ok(value)
}

async fn load(
    store: &dyn radroots_storage::Storage,
    author: [u8; 32],
    key: [u8; 32],
) -> Result<Option<Wire>, E> {
    let document = ProjectionStore::projection_document(
        store,
        projection(author)?,
        generation()?,
        hex::encode(key),
    )
    .await
    .map_err(|_| E::Storage)?;
    document
        .map(|row| {
            if row.key() != hex::encode(key) {
                return Err(E::Corrupt);
            }
            decode(row.value(), author, key)
        })
        .transpose()
}

impl TeraRuntime {
    /// Exact lookup, independent of UI pages. No caller can choose another author.
    pub async fn native_recovery_status(
        &self,
        key: [u8; 32],
    ) -> Result<Option<NativeRecoveryStatus>, E> {
        let _command = self.lifecycle.enter()?;
        let author = self
            .store_public_key
            .ok_or(E::IdentityUnavailable)?
            .into_bytes();
        let store = self.client.storage().map_err(|_| E::Storage)?;
        Ok(load(store, author, key).await?.map(|value| value.status()))
    }

    /// Host-reported observations are advisory. They cannot modify native state,
    /// authored media, publication, or an upload's immutable attempt identity.
    pub async fn report_native_recovery_status(
        &self,
        key: [u8; 32],
        reason: NativeRecoveryReason,
    ) -> Result<Option<NativeRecoveryStatus>, E> {
        let _command = self.lifecycle.enter()?;
        let _writer = self.recovery_status_lock.lock().await;
        let author = self
            .store_public_key
            .ok_or(E::IdentityUnavailable)?
            .into_bytes();
        let store = self.client.storage().map_err(|_| E::Storage)?;
        let prior = load(store, author, key).await?;
        if let Some(value) = prior.as_ref() {
            if value.reason == reason {
                return Ok(Some(value.status()));
            }
        } else if reason == NativeRecoveryReason::Resolved {
            return Ok(None); // Successful transfers need no repair record.
        }
        let now = phase1_operation_now_unix_ms()?;
        let value = Wire {
            schema_version: VERSION,
            author,
            key,
            reason,
            revision: match prior.as_ref() {
                Some(value) => value
                    .revision
                    .checked_add(1)
                    .filter(|v| *v <= i64::MAX as u64)
                    .ok_or(E::Corrupt)?,
                None => 1,
            },
            first_observed_unix_ms: prior
                .as_ref()
                .map_or(now, |value| value.first_observed_unix_ms),
            updated_at_unix_ms: prior
                .as_ref()
                .map_or(now, |value| now.max(value.updated_at_unix_ms)),
        };
        let bytes = serde_json::to_vec(&value).map_err(|_| E::Corrupt)?;
        decode(&bytes, author, key)?;
        let document = ProjectionDocument::new(hex::encode(key), bytes).map_err(|_| E::Corrupt)?;
        ProjectionStore::put_projection_document(
            store,
            projection(author)?,
            generation()?,
            document,
        )
        .await
        .map_err(|_| E::Storage)?;
        Ok(Some(value.status()))
    }
}

#[cfg(test)]
#[path = "recovery_status_tests.rs"]
mod tests;
