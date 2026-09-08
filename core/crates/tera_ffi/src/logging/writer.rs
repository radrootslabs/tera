use std::ffi::{OsStr, OsString};
#[cfg(any(test, not(unix)))]
use std::fs;
use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

const DEFAULT_MAX_FILE_BYTES: u64 = 10 * 1024 * 1024;
const DEFAULT_RETAINED_FILES: usize = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct LogRotation {
    max_file_bytes: u64,
    retained_files: usize,
}

impl Default for LogRotation {
    fn default() -> Self {
        Self {
            max_file_bytes: DEFAULT_MAX_FILE_BYTES,
            retained_files: DEFAULT_RETAINED_FILES,
        }
    }
}

pub(super) struct SizeRotatingWriter {
    #[cfg(not(unix))]
    path: PathBuf,
    #[cfg(unix)]
    directory: File,
    file_name: OsString,
    file: Option<File>,
    bytes_written: u64,
    policy: LogRotation,
}

impl SizeRotatingWriter {
    pub(super) fn new(path: PathBuf, policy: LogRotation) -> io::Result<Self> {
        let file_name = path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "log target has no name"))?
            .to_owned();
        #[cfg(unix)]
        let directory = open_secure_directory(path.parent().ok_or_else(|| {
            io::Error::new(io::ErrorKind::InvalidInput, "log target has no parent")
        })?)?;
        #[cfg(unix)]
        let file = open_append_at(&directory, &file_name)?;
        #[cfg(not(unix))]
        let file = open_append(&path)?;
        let bytes_written = file.metadata()?.len();
        let mut writer = Self {
            #[cfg(not(unix))]
            path,
            #[cfg(unix)]
            directory,
            file_name,
            file: Some(file),
            bytes_written,
            policy,
        };
        if writer.bytes_written >= writer.policy.max_file_bytes {
            writer.rotate()?;
        }
        Ok(writer)
    }

    fn rotate(&mut self) -> io::Result<()> {
        if let Some(mut file) = self.file.take() {
            file.flush()?;
            file.sync_data()?;
        }
        if self.policy.retained_files == 1 {
            self.remove_if_present(&self.file_name)?;
        } else {
            self.remove_if_present(&rotated_name(
                &self.file_name,
                self.policy.retained_files - 1,
            ))?;
            for index in (2..self.policy.retained_files).rev() {
                self.rename_if_present(
                    &rotated_name(&self.file_name, index - 1),
                    &rotated_name(&self.file_name, index),
                )?;
            }
            self.rename_if_present(&self.file_name, &rotated_name(&self.file_name, 1))?;
        }
        #[cfg(unix)]
        let file = open_append_at(&self.directory, &self.file_name)?;
        #[cfg(not(unix))]
        let file = open_append(&self.path)?;
        self.file = Some(file);
        self.bytes_written = 0;
        Ok(())
    }

    fn remove_if_present(&self, name: &OsStr) -> io::Result<()> {
        #[cfg(unix)]
        {
            use rustix::fs::{AtFlags, unlinkat};
            match unlinkat(&self.directory, name, AtFlags::empty()) {
                Ok(()) => Ok(()),
                Err(error) if error == rustix::io::Errno::NOENT => Ok(()),
                Err(error) => Err(io::Error::from_raw_os_error(error.raw_os_error())),
            }
        }
        #[cfg(not(unix))]
        remove_if_present(&self.path.with_file_name(name))
    }

    fn rename_if_present(&self, source: &OsStr, target: &OsStr) -> io::Result<()> {
        #[cfg(unix)]
        {
            use rustix::fs::renameat;
            match renameat(&self.directory, source, &self.directory, target) {
                Ok(()) => Ok(()),
                Err(error) if error == rustix::io::Errno::NOENT => Ok(()),
                Err(error) => Err(io::Error::from_raw_os_error(error.raw_os_error())),
            }
        }
        #[cfg(not(unix))]
        rename_if_present(
            &self.path.with_file_name(source),
            &self.path.with_file_name(target),
        )
    }

    fn file_mut(&mut self) -> io::Result<&mut File> {
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("log file is unavailable"))
    }
}

impl Write for SizeRotatingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let incoming = u64::try_from(buffer.len())
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "log event is too large"))?;
        if incoming > self.policy.max_file_bytes {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "log event exceeds the configured file limit",
            ));
        }
        if self.bytes_written > 0
            && self.bytes_written.saturating_add(incoming) > self.policy.max_file_bytes
        {
            self.rotate()?;
        }
        let written = self.file_mut()?.write(buffer)?;
        self.bytes_written = self
            .bytes_written
            .saturating_add(u64::try_from(written).unwrap_or(u64::MAX));
        Ok(written)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.file_mut()?.flush()
    }
}

#[cfg(test)]
fn reject_unsafe_target(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_file() => Err(
            io::Error::new(io::ErrorKind::InvalidInput, "log target is not a safe file"),
        ),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(unix)]
fn open_secure_directory(path: &Path) -> io::Result<File> {
    use rustix::fs::{FileType, Mode, OFlags, fstat, open};
    use rustix::process::geteuid;

    let directory = open(
        path,
        OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )
    .map_err(errno_to_io)?;
    let status = fstat(&directory).map_err(errno_to_io)?;
    if FileType::from_raw_mode(status.st_mode) != FileType::Directory
        || status.st_uid != geteuid().as_raw()
        || status.st_mode & 0o022 != 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "log parent must be an owner-controlled non-writable directory",
        ));
    }
    Ok(File::from(directory))
}

#[cfg(unix)]
fn open_append_at(directory: &File, name: &OsStr) -> io::Result<File> {
    use rustix::fs::{FileType, Mode, OFlags, fchmod, fstat, openat};

    let descriptor = openat(
        directory,
        name,
        OFlags::WRONLY | OFlags::APPEND | OFlags::CREATE | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .map_err(errno_to_io)?;
    let status = fstat(&descriptor).map_err(errno_to_io)?;
    if FileType::from_raw_mode(status.st_mode) != FileType::RegularFile || status.st_nlink != 1 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "log target must be one regular file link",
        ));
    }
    fchmod(&descriptor, Mode::RUSR | Mode::WUSR).map_err(errno_to_io)?;
    Ok(File::from(descriptor))
}

#[cfg(unix)]
fn errno_to_io(error: rustix::io::Errno) -> io::Error {
    io::Error::from_raw_os_error(error.raw_os_error())
}

#[cfg(not(unix))]
fn open_append(_path: &Path) -> io::Result<File> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "secure file logging is unavailable without a no-follow ACL implementation",
    ))
}

#[cfg(test)]
fn rotated_path(path: &Path, index: usize) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(format!(".{index}"));
    PathBuf::from(value)
}

fn rotated_name(name: &OsStr, index: usize) -> OsString {
    let mut value = name.to_owned();
    value.push(format!(".{index}"));
    value
}

#[cfg(any(test, not(unix)))]
fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(any(test, not(unix)))]
fn rename_if_present(source: &Path, target: &Path) -> io::Result<()> {
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
#[cfg_attr(coverage_nightly, coverage(off))]
mod tests {
    use super::{
        LogRotation, SizeRotatingWriter, reject_unsafe_target, remove_if_present,
        rename_if_present, rotated_path,
    };
    use std::ffi::OsStr;
    use std::io::Write;

    #[test]
    fn rotation_is_size_bounded_and_retention_is_finite() {
        let directory = tempfile::tempdir().expect("temporary log directory");
        let path = directory.path().join("radroots.log");
        let mut writer = SizeRotatingWriter::new(
            path.clone(),
            LogRotation {
                max_file_bytes: 5,
                retained_files: 3,
            },
        )
        .expect("writer");
        for value in [b"1111", b"2222", b"3333", b"4444"] {
            writer.write_all(value).expect("log entry");
        }
        writer.flush().expect("flush");
        assert_eq!(std::fs::read(&path).expect("current"), b"4444");
        assert_eq!(
            std::fs::read(rotated_path(&path, 1)).expect("first retained"),
            b"3333"
        );
        assert_eq!(
            std::fs::read(rotated_path(&path, 2)).expect("second retained"),
            b"2222"
        );
        assert!(!rotated_path(&path, 3).exists());
    }

    #[test]
    fn single_file_rotation_removes_the_previous_file() {
        let directory = tempfile::tempdir().expect("temporary log directory");
        let path = directory.path().join("radroots.log");
        std::fs::write(&path, b"full!").expect("fixture");
        let mut writer = SizeRotatingWriter::new(
            path.clone(),
            LogRotation {
                max_file_bytes: 5,
                retained_files: 1,
            },
        )
        .expect("writer");
        writer.write_all(b"next").expect("write");
        writer.flush().expect("flush");
        assert_eq!(std::fs::read(path).expect("current"), b"next");
    }

    #[test]
    fn oversized_events_and_unavailable_files_fail_closed() {
        let directory = tempfile::tempdir().expect("temporary log directory");
        let path = directory.path().join("radroots.log");
        let mut writer = SizeRotatingWriter::new(
            path,
            LogRotation {
                max_file_bytes: 3,
                retained_files: 2,
            },
        )
        .expect("writer");
        assert_eq!(
            writer.write(b"four").expect_err("oversized").kind(),
            std::io::ErrorKind::InvalidData
        );
        writer.write_all(b"a").expect("first short write");
        writer.write_all(b"b").expect("second short write");
        writer.file = None;
        assert_eq!(
            writer.flush().expect_err("missing file").kind(),
            std::io::ErrorKind::Other
        );
    }

    #[test]
    fn unsafe_targets_and_absent_rotation_files_are_handled() {
        let directory = tempfile::tempdir().expect("temporary directory");
        assert!(reject_unsafe_target(directory.path()).is_err());
        let regular = directory.path().join("regular");
        std::fs::write(&regular, b"regular").expect("regular file");
        assert!(reject_unsafe_target(&regular).is_ok());
        let missing = directory.path().join("missing");
        assert!(reject_unsafe_target(&missing).is_ok());
        assert!(remove_if_present(&missing).is_ok());
        assert!(rename_if_present(&missing, &directory.path().join("target")).is_ok());
        assert!(remove_if_present(directory.path()).is_err());

        let source = directory.path().join("source");
        std::fs::write(&source, b"source").expect("source");
        assert!(rename_if_present(&source, directory.path()).is_err());

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(directory.path(), &missing).expect("symlink");
            assert!(reject_unsafe_target(&missing).is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn descriptor_relative_open_rejects_symlinks_and_multiple_links() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let outside = tempfile::NamedTempFile::new().expect("outside file");
        let symlink = directory.path().join("radroots.log");
        std::os::unix::fs::symlink(outside.path(), &symlink).expect("symlink");
        assert!(SizeRotatingWriter::new(symlink.clone(), LogRotation::default()).is_err());

        std::fs::remove_file(&symlink).expect("remove symlink");
        std::fs::hard_link(outside.path(), &symlink).expect("hard link");
        assert!(SizeRotatingWriter::new(symlink, LogRotation::default()).is_err());

        let link_holder = tempfile::tempdir().expect("link holder");
        let parent_link = link_holder.path().join("parent-link");
        std::os::unix::fs::symlink(directory.path(), &parent_link).expect("parent symlink");
        assert!(
            SizeRotatingWriter::new(parent_link.join("other.log"), LogRotation::default()).is_err()
        );

        use std::os::unix::fs::PermissionsExt;
        let insecure = tempfile::tempdir().expect("insecure parent");
        std::fs::set_permissions(insecure.path(), std::fs::Permissions::from_mode(0o777))
            .expect("insecure permissions");
        assert!(
            SizeRotatingWriter::new(insecure.path().join("radroots.log"), LogRotation::default())
                .is_err()
        );
    }

    #[cfg(unix)]
    #[test]
    fn descriptor_relative_rotation_propagates_non_missing_errors() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut writer = SizeRotatingWriter::new(
            directory.path().join("radroots.log"),
            LogRotation::default(),
        )
        .expect("writer");
        writer.flush().expect("flush");

        std::fs::create_dir(directory.path().join("blocked")).expect("blocked directory");
        assert!(writer.remove_if_present(OsStr::new("blocked")).is_err());
        std::fs::write(directory.path().join("source"), b"source").expect("source");
        assert!(
            writer
                .rename_if_present(OsStr::new("source"), OsStr::new("blocked"))
                .is_err()
        );
    }

    #[test]
    fn rotation_without_an_open_file_recreates_the_target() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("radroots.log");
        let mut writer = SizeRotatingWriter::new(
            path,
            LogRotation {
                max_file_bytes: 3,
                retained_files: 2,
            },
        )
        .expect("writer");
        writer.file = None;
        writer.rotate().expect("rotation");
        writer.write_all(b"ok").expect("write after rotation");
    }
}
