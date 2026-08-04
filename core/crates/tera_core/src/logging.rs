mod writer;

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::sync::Mutex;

use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::prelude::*;

use self::writer::{LogRotation, SizeRotatingWriter};

#[derive(Debug, Clone, PartialEq, Eq)]
struct LoggingOptions {
    dir: Option<PathBuf>,
    file_name: String,
    stdout: bool,
    rotation: LogRotation,
}

impl Default for LoggingOptions {
    fn default() -> Self {
        Self {
            dir: None,
            file_name: "radroots.log".to_owned(),
            stdout: true,
            rotation: LogRotation::default(),
        }
    }
}

struct ActiveLogging {
    options: LoggingOptions,
    _file_guard: Option<WorkerGuard>,
}

static LOGGING: Mutex<Option<ActiveLogging>> = Mutex::new(None);

#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn init_logging(
    dir: Option<String>,
    file_name: Option<String>,
    is_stdout: Option<bool>,
) -> Result<(), crate::RadrootsAppError> {
    let opts = logging_options(dir, file_name, is_stdout);
    initialize(opts).map_err(crate::RadrootsAppError::initialization)
}

fn logging_options(
    dir: Option<String>,
    file_name: Option<String>,
    is_stdout: Option<bool>,
) -> LoggingOptions {
    LoggingOptions {
        dir: dir.map(PathBuf::from),
        file_name: file_name.unwrap_or_else(|| "radroots.log".to_string()),
        stdout: is_stdout.unwrap_or(true),
        ..LoggingOptions::default()
    }
}

fn initialize(options: LoggingOptions) -> Result<(), String> {
    validate_options(&options)?;
    let mut active = LOGGING
        .lock()
        .map_err(|_| "logging initialization state is poisoned".to_owned())?;
    if let Some(existing) = active.as_ref() {
        return if existing.options == options {
            Ok(())
        } else {
            Err("logging is already initialized with a different configuration".to_owned())
        };
    }

    let (file_writer, file_guard) = build_file_writer(&options)?;
    let file_layer = file_writer.as_ref().map(|writer| {
        tracing_subscriber::fmt::layer()
            .with_writer(writer.clone())
            .with_ansi(false)
            .with_target(false)
    });
    let stdout_layer = options.stdout.then(|| {
        tracing_subscriber::fmt::layer()
            .with_writer(std::io::stdout)
            .with_target(false)
    });
    tracing_subscriber::registry()
        .with(file_layer)
        .with(stdout_layer)
        .try_init()
        .map_err(|error| error.to_string())?;
    *active = Some(ActiveLogging {
        options,
        _file_guard: file_guard,
    });
    Ok(())
}

fn validate_options(options: &LoggingOptions) -> Result<(), String> {
    if options.dir.is_none() && !options.stdout {
        return Err("logging requires at least one configured output".to_owned());
    }
    if options.file_name.is_empty()
        || Path::new(&options.file_name).components().count() != 1
        || !matches!(
            Path::new(&options.file_name).components().next(),
            Some(Component::Normal(_))
        )
    {
        return Err("log file_name must be a safe basename".to_owned());
    }
    Ok(())
}

type FileWriter = tracing_appender::non_blocking::NonBlocking;

fn build_file_writer(
    options: &LoggingOptions,
) -> Result<(Option<FileWriter>, Option<WorkerGuard>), String> {
    let Some(dir) = options.dir.as_ref() else {
        return Ok((None, None));
    };
    fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    let metadata = fs::symlink_metadata(dir).map_err(|error| error.to_string())?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        return Err("log directory is not a safe directory".to_owned());
    }
    let writer = SizeRotatingWriter::new(dir.join(&options.file_name), options.rotation)
        .map_err(|error| error.to_string())?;
    let (writer, guard) = tracing_appender::non_blocking::NonBlockingBuilder::default()
        .buffered_lines_limit(8192)
        .lossy(false)
        .thread_name("radroots-app-log-writer")
        .finish(writer);
    Ok((Some(writer), Some(guard)))
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn init_logging_stdout() -> Result<(), crate::RadrootsAppError> {
    initialize(LoggingOptions::default()).map_err(crate::RadrootsAppError::initialization)
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn log_info(msg: String) -> Result<(), crate::RadrootsAppError> {
    tracing::info!("{msg}");
    Ok(())
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn log_error(msg: String) -> Result<(), crate::RadrootsAppError> {
    tracing::error!("{msg}");
    Ok(())
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn log_debug(msg: String) -> Result<(), crate::RadrootsAppError> {
    tracing::debug!("{msg}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{LogRotation, LoggingOptions, logging_options};
    use std::path::PathBuf;

    #[test]
    fn logging_options_adopt_bounded_library_defaults() {
        let options = logging_options(
            Some("logs".to_owned()),
            Some("mobile.log".to_owned()),
            Some(false),
        );

        assert_eq!(options.dir, Some(PathBuf::from("logs")));
        assert_eq!(options.file_name, "mobile.log");
        assert!(!options.stdout);
        assert_eq!(options.rotation, LogRotation::default());
    }

    #[test]
    fn logging_options_preserve_public_api_defaults() {
        let options = logging_options(None, None, None);

        assert_eq!(options.file_name, "radroots.log");
        assert!(options.stdout);
        assert_eq!(options, LoggingOptions::default());
    }
}
