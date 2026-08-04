use std::fs::{self, File, OpenOptions};
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
    path: PathBuf,
    file: Option<File>,
    bytes_written: u64,
    policy: LogRotation,
}

impl SizeRotatingWriter {
    pub(super) fn new(path: PathBuf, policy: LogRotation) -> io::Result<Self> {
        reject_unsafe_target(&path)?;
        let file = open_append(&path)?;
        let bytes_written = file.metadata()?.len();
        let mut writer = Self {
            path,
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
            remove_if_present(&self.path)?;
        } else {
            remove_if_present(&rotated_path(&self.path, self.policy.retained_files - 1))?;
            for index in (2..self.policy.retained_files).rev() {
                rename_if_present(
                    &rotated_path(&self.path, index - 1),
                    &rotated_path(&self.path, index),
                )?;
            }
            rename_if_present(&self.path, &rotated_path(&self.path, 1))?;
        }
        self.file = Some(open_append(&self.path)?);
        self.bytes_written = 0;
        Ok(())
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

fn open_append(path: &Path) -> io::Result<File> {
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
        options.mode(0o600);
        let file = options.open(path)?;
        file.set_permissions(fs::Permissions::from_mode(0o600))?;
        Ok(file)
    }
    #[cfg(not(unix))]
    options.open(path)
}

fn rotated_path(path: &Path, index: usize) -> PathBuf {
    let mut value = path.as_os_str().to_owned();
    value.push(format!(".{index}"));
    PathBuf::from(value)
}

fn remove_if_present(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn rename_if_present(source: &Path, target: &Path) -> io::Result<()> {
    match fs::rename(source, target) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::{LogRotation, SizeRotatingWriter, rotated_path};
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
}
