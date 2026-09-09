use super::*;
use std::io::{Read, Seek, SeekFrom, Write};
use std::os::unix::fs::MetadataExt;
use std::sync::Arc;

fn fixture(bytes: &[u8]) -> tempfile::NamedTempFile {
    let mut file = tempfile::NamedTempFile::new().expect("fixture");
    file.write_all(bytes).expect("fixture bytes");
    file
}

fn owner(file: &File) -> FfiMediaFile {
    FfiMediaFile::new(file.as_raw_fd() as u64, file.metadata().unwrap().len()).unwrap()
}

// The unique fixture remains linked for the whole test. Comparing its inode
// avoids assuming that a closed descriptor stays unused by parallel tests.
fn still_owns_fixture(fd: RawFd, metadata: &std::fs::Metadata) -> bool {
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: fstat accepts an integer descriptor, writes only to valid storage,
    // and reports a descriptor that was already closed as an error.
    if unsafe { libc::fstat(fd, status.as_mut_ptr()) } != 0 {
        return false;
    }
    // SAFETY: a successful fstat initialized the complete result.
    let status = unsafe { status.assume_init() };
    i128::from(status.st_dev) == i128::from(metadata.dev())
        && i128::from(status.st_ino) == i128::from(metadata.ino())
}

#[test]
fn admission_owns_cloexec_duplicate_and_preserves_caller_offset() {
    let mut original = fixture(b"original");
    original.seek(SeekFrom::Start(3)).unwrap();
    let admitted = owner(original.as_file());
    assert_ne!(admitted.file.as_raw_fd(), original.as_raw_fd());
    // SAFETY: admitted owns this live descriptor for the complete inspection.
    let flags = unsafe { libc::fcntl(admitted.file.as_raw_fd(), libc::F_GETFD) };
    assert_ne!(flags & libc::FD_CLOEXEC, 0);
    assert_eq!(admitted.read(8).unwrap(), b"original");
    assert_eq!(original.stream_position().unwrap(), 3);
    drop(original);
    assert_eq!(admitted.read(8).unwrap(), b"original");
    assert_eq!(format!("{admitted:?}"), "FfiMediaFile { byte_size: 8, .. }");
}

#[test]
fn descriptor_replacement_cannot_change_admitted_file() {
    let mut original = fixture(b"original");
    let replacement = fixture(b"replaced");
    let replacement = File::open(replacement.path()).unwrap();
    let admitted = owner(original.as_file());
    // SAFETY: both slots are owned by this test. dup2 atomically closes and
    // replaces only original's slot, whose sole RAII owner remains original.
    assert_eq!(
        unsafe { libc::dup2(replacement.as_raw_fd(), original.as_raw_fd()) },
        original.as_raw_fd()
    );
    let mut current = String::new();
    original.read_to_string(&mut current).unwrap();
    assert_eq!(current, "replaced");
    assert_eq!(admitted.read(8).unwrap(), b"original");
}

#[test]
fn last_owner_releases_exactly_once_without_closing_caller() {
    let original = fixture(b"original");
    let metadata = original.as_file().metadata().unwrap();
    let first = Arc::new(owner(original.as_file()));
    let fd = first.file.as_raw_fd();
    let weak = Arc::downgrade(&first);
    let second = first.clone();
    drop(first);
    assert!(still_owns_fixture(fd, &metadata));
    assert_eq!(second.read(8).unwrap(), b"original");
    drop(second);
    assert!(weak.upgrade().is_none());
    assert!(!still_owns_fixture(fd, &metadata));
    assert!(still_owns_fixture(original.as_raw_fd(), &metadata));
}

#[test]
fn generated_object_codec_retains_ownership_after_foreign_reference_drop() {
    let original = fixture(b"original");
    let admitted = Arc::new(owner(original.as_file()));
    let weak = Arc::downgrade(&admitted);
    let mut buffer = Vec::new();
    <Arc<FfiMediaFile> as uniffi::FfiConverter<crate::UniFfiTag>>::write(admitted, &mut buffer);
    drop(original);
    assert!(weak.upgrade().is_some());
    let mut input = buffer.as_slice();
    let decoded =
        <Arc<FfiMediaFile> as uniffi::FfiConverter<crate::UniFfiTag>>::try_read(&mut input)
            .unwrap();
    assert!(input.is_empty());
    assert_eq!(decoded.read(8).unwrap(), b"original");
    drop(decoded);
    assert!(weak.upgrade().is_none());
}

#[test]
fn admission_rejects_nonregular_unreadable_and_wrong_size_files() {
    let original = fixture(b"original");
    let write_only = std::fs::OpenOptions::new()
        .write(true)
        .open(original.path())
        .unwrap();
    assert_eq!(
        FfiMediaFile::new(write_only.as_raw_fd() as u64, 8)
            .unwrap_err()
            .report()
            .code,
        "media_handle_unavailable"
    );
    let directory = tempfile::tempdir().unwrap();
    let directory_file = File::open(directory.path()).unwrap();
    assert!(FfiMediaFile::new(directory_file.as_raw_fd() as u64, 8).is_err());
    let (socket, _peer) = std::os::unix::net::UnixStream::pair().unwrap();
    assert!(FfiMediaFile::new(socket.as_raw_fd() as u64, 8).is_err());
    assert_eq!(
        FfiMediaFile::new(original.as_raw_fd() as u64, 7)
            .unwrap_err()
            .report()
            .code,
        "media_size_mismatch"
    );
    assert_eq!(
        FfiMediaFile::new(original.as_raw_fd() as u64, 0)
            .unwrap_err()
            .report()
            .code,
        "invalid_media_reference"
    );
}

#[test]
fn exact_size_limit_is_admitted_and_changes_are_rechecked() {
    let original = fixture(b"x");
    original.as_file().set_len(MEDIA_FILE_MAX_BYTES).unwrap();
    let admitted = owner(original.as_file());
    assert_eq!(admitted.byte_size, MEDIA_FILE_MAX_BYTES);
    assert_eq!(
        admitted.read(MEDIA_FILE_MAX_BYTES).unwrap().len() as u64,
        MEDIA_FILE_MAX_BYTES
    );
    assert_eq!(
        FfiMediaFile::new(original.as_raw_fd() as u64, MEDIA_FILE_MAX_BYTES + 1)
            .unwrap_err()
            .report()
            .code,
        "invalid_media_reference"
    );
    original.as_file().set_len(1).unwrap();
    assert_eq!(
        admitted
            .read(MEDIA_FILE_MAX_BYTES)
            .unwrap_err()
            .report()
            .code,
        "media_size_mismatch"
    );
    assert_eq!(
        admitted.read(1).unwrap_err().report().code,
        "media_size_mismatch"
    );
}
