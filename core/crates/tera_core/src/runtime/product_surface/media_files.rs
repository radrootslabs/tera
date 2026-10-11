//! Caller-owned physical cache mutations; no dispatched write can escape its fence.
use std::io::{Read, Write};

use super::*;

#[path = "media_orphans.rs"]
mod orphans;
pub(crate) use orphans::InboundMediaFiles;

pub(crate) async fn write_verified_artifact(
    directory: &Path,
    receipt: &Phase1VerifiedMediaReceipt,
    bytes: &[u8],
) -> Result<Phase1LocalMediaArtifact, Phase1InboundMediaError> {
    // Every mutating operation finishes in this poll while the caller owns its
    // runtime file/lifecycle guards. Cancellation cannot detach file work.
    receipt.validate_intrinsic()?;
    if bytes.len() as u64 != receipt.byte_size
        || Sha256::digest(bytes).to_hex() != receipt.observed_sha256
    {
        return Err(Phase1InboundMediaError::CorruptArtifact);
    }
    ensure_cache_directory(directory)?;
    let final_path = artifact_path(directory, receipt.artifact_id, receipt.extension.as_str())?;
    match std::fs::symlink_metadata(&final_path) {
        Ok(_) => {
            let verified_bytes = verify_artifact_file_sync(&final_path, receipt)?;
            return Ok(local_artifact(final_path, receipt, verified_bytes));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(cache_io_error(error)),
    }
    let temporary_path = directory.join(format!(
        ".{}.{}.tmp",
        receipt.artifact_id.to_hex(),
        uuid::Uuid::new_v4().simple()
    ));
    let mut temporary = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temporary_path)
        .map_err(cache_io_error)?;
    let cleanup = TemporaryFile(&temporary_path);
    temporary.write_all(bytes).map_err(cache_io_error)?;
    temporary.flush().map_err(cache_io_error)?;
    temporary.sync_all().map_err(cache_io_error)?;
    drop(temporary);
    match std::fs::hard_link(&temporary_path, &final_path) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            verify_artifact_file_sync(&final_path, receipt)?;
        }
        Err(error) => return Err(cache_io_error(error)),
    }
    std::fs::remove_file(&temporary_path).map_err(cache_io_error)?;
    drop(cleanup);
    sync_cache_directory(directory)?;
    let verified_bytes = verify_artifact_file_sync(&final_path, receipt)?;
    Ok(local_artifact(final_path, receipt, verified_bytes))
}

struct TemporaryFile<'a>(&'a Path);
impl Drop for TemporaryFile<'_> {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0);
    }
}

pub(crate) fn remove_artifact_files(
    directory: &Path,
    artifact_id: Phase1MediaArtifactId,
) -> Result<(), Phase1InboundMediaError> {
    match validate_directory(directory) {
        Err(Phase1InboundMediaError::CacheUnavailable) => return Ok(()),
        result => result?,
    };
    for extension in MEDIA_CACHE_EXTENSIONS {
        remove_regular_file(&artifact_path(directory, artifact_id, extension)?)?;
    }
    sync_cache_directory(directory)
}

pub(crate) async fn verified_artifact(
    directory: &Path,
    receipt: &Phase1VerifiedMediaReceipt,
) -> Result<Phase1LocalMediaArtifact, Phase1InboundMediaError> {
    receipt.validate_intrinsic()?;
    ensure_cache_directory(directory)?;
    let path = artifact_path(directory, receipt.artifact_id, receipt.extension.as_str())?;
    let metadata = tokio::fs::symlink_metadata(&path)
        .await
        .map_err(|_| Phase1InboundMediaError::CorruptArtifact)?;
    validate_file(&metadata, receipt)?;
    // Read-only worker work conveys no custody or deletion authority. The
    // caller revalidates its current receipt after this await.
    let bytes = tokio::fs::read(&path).await.map_err(cache_io_error)?;
    validate_bytes(&bytes, receipt)?;
    Ok(local_artifact(path, receipt, bytes))
}

fn ensure_cache_directory(directory: &Path) -> Result<(), Phase1InboundMediaError> {
    match std::fs::create_dir(directory) {
        Ok(()) => {}
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(error) => return Err(cache_io_error(error)),
    }
    validate_directory(directory).map(|_| ())
}

fn validate_directory(directory: &Path) -> Result<std::fs::Metadata, Phase1InboundMediaError> {
    let metadata = std::fs::symlink_metadata(directory).map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            Phase1InboundMediaError::CacheUnavailable
        } else {
            cache_io_error(error)
        }
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err(Phase1InboundMediaError::CorruptArtifact);
    }
    Ok(metadata)
}

fn remove_regular_file(path: &Path) -> Result<bool, Phase1InboundMediaError> {
    match std::fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => {
            Err(Phase1InboundMediaError::CorruptArtifact)
        }
        Ok(_) => {
            std::fs::remove_file(path).map_err(cache_io_error)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(cache_io_error(error)),
    }
}

fn artifact_path(
    directory: &Path,
    artifact_id: Phase1MediaArtifactId,
    extension: &str,
) -> Result<PathBuf, Phase1InboundMediaError> {
    if !MEDIA_CACHE_EXTENSIONS.contains(&extension) {
        return Err(Phase1InboundMediaError::CorruptArtifact);
    }
    Ok(directory.join(format!("{}.{}", artifact_id.to_hex(), extension)))
}

fn verify_artifact_file_sync(
    path: &Path,
    receipt: &Phase1VerifiedMediaReceipt,
) -> Result<Vec<u8>, Phase1InboundMediaError> {
    let metadata =
        std::fs::symlink_metadata(path).map_err(|_| Phase1InboundMediaError::CorruptArtifact)?;
    validate_file(&metadata, receipt)?;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(cache_io_error)?
        .take(receipt.byte_size.saturating_add(1))
        .read_to_end(&mut bytes)
        .map_err(cache_io_error)?;
    validate_bytes(&bytes, receipt)?;
    Ok(bytes)
}

fn validate_file(
    metadata: &std::fs::Metadata,
    receipt: &Phase1VerifiedMediaReceipt,
) -> Result<(), Phase1InboundMediaError> {
    if metadata.file_type().is_symlink()
        || !metadata.is_file()
        || metadata.len() != receipt.byte_size
    {
        return Err(Phase1InboundMediaError::CorruptArtifact);
    }
    Ok(())
}

fn validate_bytes(
    bytes: &[u8],
    receipt: &Phase1VerifiedMediaReceipt,
) -> Result<(), Phase1InboundMediaError> {
    if bytes.len() as u64 != receipt.byte_size
        || Sha256::digest(bytes).to_hex() != receipt.observed_sha256
    {
        return Err(Phase1InboundMediaError::CorruptArtifact);
    }
    Ok(())
}

fn sync_cache_directory(directory: &Path) -> Result<(), Phase1InboundMediaError> {
    std::fs::File::open(directory)
        .and_then(|directory| directory.sync_all())
        .map_err(cache_io_error)
}

fn local_artifact(
    local_path: PathBuf,
    receipt: &Phase1VerifiedMediaReceipt,
    bytes: Vec<u8>,
) -> Phase1LocalMediaArtifact {
    Phase1LocalMediaArtifact {
        artifact_id: receipt.artifact_id,
        local_path,
        bytes,
        byte_size: receipt.byte_size,
        media_type: receipt.media_type.clone(),
        width: receipt.width,
        height: receipt.height,
    }
}
