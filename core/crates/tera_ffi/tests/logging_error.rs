use tera_ffi::TeraAppError;
use tera_ffi::logging;

#[test]
fn init_logging_stdout_maps_global_subscriber_error() {
    let _ = tracing_subscriber::fmt().try_init();
    let err = logging::init_logging_stdout();
    assert!(matches!(
        err,
        Err(TeraAppError::Failure { report })
            if report.code == "initialization_failed"
    ));
}
