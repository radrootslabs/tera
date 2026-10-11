//! A bounded resumable directory inventory held by the existing file mutex.
use std::{ffi::OsString, fs::ReadDir};

use super::*;

#[derive(Default)]
pub(crate) struct InboundMediaFiles {
    scan: Option<DirectoryScan>,
}
struct DirectoryScan {
    path: PathBuf,
    identity: DirectoryIdentity,
    entries: ReadDir,
}
#[derive(Clone, Copy, Eq, PartialEq)]
struct DirectoryIdentity(u64, u64);

pub(crate) struct OrphanFile {
    pub(crate) artifact_id: Phase1MediaArtifactId,
    name: OsString,
}
pub(crate) struct OrphanBatch {
    pub(crate) files: Vec<OrphanFile>,
    pub(crate) scan_pending: bool,
    identity: Option<DirectoryIdentity>,
}

impl InboundMediaFiles {
    pub(crate) fn inventory(
        &mut self,
        directory: &Path,
        budget: usize,
    ) -> Result<OrphanBatch, Phase1InboundMediaError> {
        let identity = match directory_identity(directory) {
            Ok(identity) => identity,
            Err(Phase1InboundMediaError::CacheUnavailable) => {
                self.scan = None;
                return Ok(OrphanBatch {
                    files: Vec::new(),
                    scan_pending: false,
                    identity: None,
                });
            }
            Err(error) => return Err(error),
        };
        if budget == 0 {
            return Ok(OrphanBatch {
                files: Vec::new(),
                scan_pending: true,
                identity: Some(identity),
            });
        }
        if self
            .scan
            .as_ref()
            .is_none_or(|scan| scan.path != directory || scan.identity != identity)
        {
            self.scan = Some(DirectoryScan {
                path: directory.to_path_buf(),
                identity,
                entries: std::fs::read_dir(directory).map_err(cache_io_error)?,
            });
        }
        let mut files = Vec::new();
        let mut exhausted = false;
        if let Some(scan) = self.scan.as_mut() {
            for _ in 0..budget {
                let Some(entry) = scan.entries.next() else {
                    exhausted = true;
                    break;
                };
                let entry = entry.map_err(cache_io_error)?;
                if entry.file_type().map_err(cache_io_error)?.is_file()
                    && let Some(artifact_id) = cache_identity(&entry.file_name())
                {
                    files.push(OrphanFile {
                        artifact_id,
                        name: entry.file_name(),
                    });
                }
            }
        }
        if exhausted {
            self.scan = None;
        }
        let batch = OrphanBatch {
            files,
            scan_pending: !exhausted,
            identity: Some(identity),
        };
        batch.confirm_root(directory)?;
        Ok(batch)
    }
}

impl OrphanBatch {
    pub(crate) fn confirm_root(&self, directory: &Path) -> Result<(), Phase1InboundMediaError> {
        if let Some(expected) = self.identity
            && directory_identity(directory)? != expected
        {
            return Err(Phase1InboundMediaError::CorruptArtifact);
        }
        Ok(())
    }
    pub(crate) fn remove(
        &self,
        directory: &Path,
        file: &OrphanFile,
    ) -> Result<bool, Phase1InboundMediaError> {
        self.confirm_root(directory)?;
        let removed = remove_regular_file(&directory.join(&file.name))?;
        if removed {
            sync_cache_directory(directory)?;
        }
        Ok(removed)
    }
}

fn cache_identity(name: &std::ffi::OsStr) -> Option<Phase1MediaArtifactId> {
    let name = name.to_str()?;
    let (hash, suffix) = if let Some(temporary) = name.strip_prefix('.') {
        let temporary = temporary.strip_suffix(".tmp")?;
        let (hash, nonce) = temporary.split_once('.')?;
        if nonce.len() != 32
            || !nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return None;
        }
        (hash, None)
    } else {
        let (hash, extension) = name.split_once('.')?;
        (hash, Some(extension))
    };
    if hash.len() != 64
        || suffix.is_some_and(|extension| !MEDIA_CACHE_EXTENSIONS.contains(&extension))
    {
        return None;
    }
    let artifact = Phase1MediaArtifactId::parse(hash).ok()?;
    (artifact.to_hex() == hash).then_some(artifact)
}

#[cfg(unix)]
fn directory_identity(directory: &Path) -> Result<DirectoryIdentity, Phase1InboundMediaError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = validate_directory(directory)?;
    Ok(DirectoryIdentity(metadata.dev(), metadata.ino()))
}
#[cfg(not(unix))]
fn directory_identity(_: &Path) -> Result<DirectoryIdentity, Phase1InboundMediaError> {
    // The current native runtime matrix is Apple/Linux. No unsupported root
    // identity can grant physical deletion authority on another filesystem.
    Err(Phase1InboundMediaError::CacheUnavailable)
}
