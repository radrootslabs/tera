use std::path::PathBuf;

#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn init_logging(
    dir: Option<String>,
    file_name: Option<String>,
    is_stdout: Option<bool>,
) -> Result<(), crate::RadrootsAppError> {
    let opts = logging_options(dir, file_name, is_stdout);
    match radroots_log::init_logging(opts) {
        Ok(()) => Ok(()),
        Err(err) => Err(crate::RadrootsAppError::initialization(format!("{err}"))),
    }
}

fn logging_options(
    dir: Option<String>,
    file_name: Option<String>,
    is_stdout: Option<bool>,
) -> radroots_log::LoggingOptions {
    radroots_log::LoggingOptions {
        dir: dir.map(PathBuf::from),
        file_name: file_name.unwrap_or_else(|| "radroots.log".to_string()),
        file_layout: radroots_log::LogFileLayout::StableFileName,
        stdout: is_stdout.unwrap_or(true),
        ..radroots_log::LoggingOptions::default()
    }
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn init_logging_stdout() -> Result<(), crate::RadrootsAppError> {
    match radroots_log::init_stdout() {
        Ok(()) => Ok(()),
        Err(err) => Err(crate::RadrootsAppError::initialization(format!("{err}"))),
    }
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn log_info(msg: String) -> Result<(), crate::RadrootsAppError> {
    radroots_log::log_info(msg);
    Ok(())
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn log_error(msg: String) -> Result<(), crate::RadrootsAppError> {
    radroots_log::log_error(msg);
    Ok(())
}

#[cfg_attr(not(coverage_nightly), uniffi::export)]
pub fn log_debug(msg: String) -> Result<(), crate::RadrootsAppError> {
    radroots_log::log_debug(msg);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::logging_options;
    use radroots_log::{LogFileLayout, LogFormat, LogRotation, LoggingOptions};
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
        assert_eq!(options.file_layout, LogFileLayout::StableFileName);
        assert!(!options.stdout);
        assert!(!options.stderr);
        assert_eq!(options.format, LogFormat::Compact);
        assert_eq!(options.identity, None);
        assert_eq!(options.rotation, LogRotation::default());
        assert_eq!(options.default_level, None);
    }

    #[test]
    fn logging_options_preserve_public_api_defaults() {
        let options = logging_options(None, None, None);

        assert_eq!(options.file_name, "radroots.log");
        assert!(options.stdout);
        assert_eq!(
            options,
            LoggingOptions {
                file_layout: LogFileLayout::StableFileName,
                ..LoggingOptions::default()
            }
        );
    }
}
