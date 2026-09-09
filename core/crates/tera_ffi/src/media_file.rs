//! Synchronous admission of a native file into an independently owned resource.
//! Async callers carry this object, never a borrowed descriptor number.

use std::fs::File;

#[cfg(unix)]
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd, RawFd};
#[cfg(unix)]
use std::os::unix::fs::FileExt;

use crate::TeraAppError;

pub(crate) const MEDIA_FILE_MAX_BYTES: u64 = 10 * 1024 * 1024;

#[cfg(all(test, unix))]
#[path = "media_file_tests.rs"]
mod tests;

/// An immutable owner. Dropping a foreign reference cannot close a file still
/// retained by an admitted Rust future. No public operation closes other owners.
#[derive(uniffi::Object)]
pub struct FfiMediaFile {
    file: File,
    byte_size: u64,
}

impl std::fmt::Debug for FfiMediaFile {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FfiMediaFile")
            .field("byte_size", &self.byte_size)
            .finish_non_exhaustive()
    }
}

#[uniffi::export]
impl FfiMediaFile {
    /// Acquire while the caller still owns the original descriptor. This is
    /// deliberately synchronous; no file bytes are read or hashed at admission.
    #[uniffi::constructor]
    pub fn new(file_descriptor: u64, byte_size: u64) -> Result<Self, TeraAppError> {
        if byte_size == 0 || byte_size > MEDIA_FILE_MAX_BYTES {
            return Err(TeraAppError::invalid_argument("invalid_media_reference"));
        }
        let file = acquire(file_descriptor, byte_size)?;
        Ok(Self { file, byte_size })
    }
}

impl FfiMediaFile {
    #[cfg(unix)]
    pub(crate) fn read(&self, expected_size: u64) -> Result<Vec<u8>, TeraAppError> {
        // File contents can change through another descriptor. Recheck the
        // exact size and retain the existing downstream digest/image validation.
        if expected_size != self.byte_size {
            return Err(TeraAppError::invalid_argument("media_size_mismatch"));
        }
        validate_size(&self.file, expected_size)?;
        let count = usize::try_from(expected_size)
            .map_err(|_| TeraAppError::invalid_argument("media_size_mismatch"))?;
        let mut bytes = vec![0; count];
        self.file
            .read_exact_at(&mut bytes, 0)
            .map_err(|_| TeraAppError::invalid_argument("media_read_failed"))?;
        Ok(bytes)
    }

    #[cfg(not(unix))]
    pub(crate) fn read(&self, _expected_size: u64) -> Result<Vec<u8>, TeraAppError> {
        Err(unsupported())
    }
}

#[cfg(unix)]
fn acquire(file_descriptor: u64, byte_size: u64) -> Result<File, TeraAppError> {
    let original = RawFd::try_from(file_descriptor)
        .map_err(|_| TeraAppError::invalid_argument("media_handle_unavailable"))?;
    // SAFETY: fcntl accepts an integer descriptor and reports invalid handles.
    // Only the successful duplicate becomes an owned Rust descriptor.
    let duplicate = unsafe { libc::fcntl(original, libc::F_DUPFD_CLOEXEC, 0) };
    if duplicate < 0 {
        return Err(TeraAppError::invalid_argument("media_handle_unavailable"));
    }
    // SAFETY: a successful F_DUPFD_CLOEXEC returns a new descriptor owned by
    // this call. Every later failure drops File and releases that duplicate.
    let file = File::from(unsafe { OwnedFd::from_raw_fd(duplicate) });
    validate_size(&file, byte_size)?;
    // SAFETY: File owns a live descriptor. These calls inspect access and
    // seekability without changing the shared file position.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags < 0 || flags & libc::O_ACCMODE == libc::O_WRONLY {
        return Err(TeraAppError::invalid_argument("media_handle_unavailable"));
    }
    // SAFETY: the owned descriptor remains live and SEEK_CUR with offset zero
    // leaves the caller's file position unchanged.
    if unsafe { libc::lseek(file.as_raw_fd(), 0, libc::SEEK_CUR) } < 0 {
        return Err(TeraAppError::invalid_argument("media_handle_unavailable"));
    }
    Ok(file)
}

#[cfg(unix)]
fn validate_size(file: &File, byte_size: u64) -> Result<(), TeraAppError> {
    let metadata = file
        .metadata()
        .map_err(|_| TeraAppError::invalid_argument("media_handle_unavailable"))?;
    if !metadata.is_file() || metadata.len() != byte_size {
        return Err(TeraAppError::invalid_argument("media_size_mismatch"));
    }
    Ok(())
}

#[cfg(not(unix))]
fn acquire(_file_descriptor: u64, _byte_size: u64) -> Result<File, TeraAppError> {
    Err(unsupported())
}

#[cfg(not(unix))]
fn unsupported() -> TeraAppError {
    TeraAppError::failure(
        "media_handle_unsupported",
        "capability",
        false,
        &[],
        "Protected media handles are unsupported on this platform.",
    )
}
