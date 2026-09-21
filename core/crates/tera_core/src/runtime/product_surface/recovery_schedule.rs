//! Account-bound advisory traversal only. This cursor cannot authorize effects.
use radroots_storage::projection::{
    ProjectionDocument, ProjectionGeneration, ProjectionId, ProjectionStore,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::Phase1DraftError as E;
use crate::TeraRuntime;

const DOMAIN: &[u8] = b"tera.native_recovery_schedule.v1";
const KEY: &str = "continuation";
const MAX_BYTES: usize = 1024;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NativeRecoverySchedule {
    pub schema_version: u16,
    pub author: [u8; 32],
    pub revision: u64,
    /// SHA-256 of the native identifier, ordered lexicographically by the host.
    pub after: Option<[u8; 32]>,
}

fn projection(author: [u8; 32]) -> Result<ProjectionId, E> {
    ProjectionId::parse(format!(
        "tera.native_recovery_schedule.{}",
        hex::encode(author)
    ))
    .map_err(|_| E::Corrupt)
}

fn generation() -> Result<ProjectionGeneration, E> {
    ProjectionGeneration::new(Sha256::digest(DOMAIN).into()).map_err(|_| E::Corrupt)
}

fn decode(bytes: &[u8], author: [u8; 32]) -> Result<NativeRecoverySchedule, E> {
    if bytes.len() > MAX_BYTES {
        return Err(E::Corrupt);
    }
    let value: NativeRecoverySchedule = serde_json::from_slice(bytes).map_err(|_| E::Corrupt)?;
    if value.schema_version != 1
        || value.author != author
        || !(1..=i64::MAX as u64).contains(&value.revision)
        || serde_json::to_vec(&value).map_err(|_| E::Corrupt)? != bytes
    {
        return Err(E::Corrupt);
    }
    Ok(value)
}

async fn load(
    store: &dyn radroots_storage::Storage,
    author: [u8; 32],
) -> Result<NativeRecoverySchedule, E> {
    let row =
        ProjectionStore::projection_document(store, projection(author)?, generation()?, KEY.into())
            .await
            .map_err(|_| E::Storage)?;
    match row {
        Some(row) if row.key() == KEY => decode(row.value(), author),
        Some(_) => Err(E::Corrupt),
        None => Ok(NativeRecoverySchedule {
            schema_version: 1,
            author,
            revision: 0,
            after: None,
        }),
    }
}

impl TeraRuntime {
    pub async fn native_recovery_schedule(&self) -> Result<NativeRecoverySchedule, E> {
        let _command = self.lifecycle.enter()?;
        let author = self
            .store_public_key
            .ok_or(E::IdentityUnavailable)?
            .into_bytes();
        load(self.client.storage().map_err(|_| E::Storage)?, author).await
    }

    /// Compare the entire prior observation under the runtime's single writer.
    /// Cursor write failures leave receipts intact for idempotent replay.
    pub async fn advance_native_recovery_schedule(
        &self,
        expected: NativeRecoverySchedule,
        after: Option<[u8; 32]>,
    ) -> Result<NativeRecoverySchedule, E> {
        let _command = self.lifecycle.enter()?;
        let _writer = self.recovery_status_lock.lock().await;
        let author = self
            .store_public_key
            .ok_or(E::IdentityUnavailable)?
            .into_bytes();
        if expected.schema_version != 1 || expected.author != author {
            return Err(E::InvalidInventoryCursor);
        }
        let store = self.client.storage().map_err(|_| E::Storage)?;
        let prior = load(store, author).await?;
        if expected != prior {
            return Err(E::InvalidInventoryCursor);
        }
        if after == prior.after {
            return Ok(prior);
        }
        let value = NativeRecoverySchedule {
            revision: prior
                .revision
                .checked_add(1)
                .filter(|v| *v <= i64::MAX as u64)
                .ok_or(E::Corrupt)?,
            after,
            ..prior
        };
        let bytes = serde_json::to_vec(&value).map_err(|_| E::Corrupt)?;
        decode(&bytes, author)?;
        let document = ProjectionDocument::new(KEY.into(), bytes).map_err(|_| E::Corrupt)?;
        ProjectionStore::put_projection_document(
            store,
            projection(author)?,
            generation()?,
            document,
        )
        .await
        .map_err(|_| E::Storage)?;
        Ok(value)
    }
}

#[cfg(test)]
#[path = "recovery_schedule_tests.rs"]
mod tests;
