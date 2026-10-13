//! Production append admission must reject a FIFO before waiting for a reader.

use super::{LogRotation, SizeRotatingWriter};
use std::io::Write;
use std::os::unix::fs::FileTypeExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus, Stdio};
use std::time::{Duration, Instant};

const CHILD_ROOT: &str = "TERA_LOG_FILE_KIND_CHILD_ROOT";
const TEST_NAME: &str =
    "logging::writer::file_kind_tests::non_regular_targets_refuse_without_waiting_for_peers";

#[test]
fn non_regular_targets_refuse_without_waiting_for_peers() {
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        let fifo = PathBuf::from(root).join("unread_fifo");
        assert!(
            std::fs::symlink_metadata(&fifo)
                .unwrap()
                .file_type()
                .is_fifo()
        );
        assert!(SizeRotatingWriter::new(fifo, LogRotation::default()).is_err());
        return;
    }

    let root = tempfile::tempdir().unwrap();
    let fifo = root.path().join("unread_fifo");
    let mut setup = Command::new("/usr/bin/mkfifo");
    setup.args(["-m", "600"]).arg(&fifo);
    assert!(
        bounded_child(&mut setup)
            .expect("FIFO fixture setup deadline")
            .success()
    );
    assert!(
        std::fs::symlink_metadata(&fifo)
            .unwrap()
            .file_type()
            .is_fifo()
    );

    let mut opening = Command::new(std::env::current_exe().unwrap());
    opening
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(CHILD_ROOT, root.path());
    let result = bounded_child(&mut opening);
    assert!(
        result.is_some(),
        "production log append waited for a FIFO reader before rejecting the target"
    );
    assert!(result.unwrap().success(), "production FIFO refusal failed");
    assert!(
        std::fs::symlink_metadata(&fifo)
            .unwrap()
            .file_type()
            .is_fifo()
    );

    let ordinary = root.path().join("ordinary.log");
    std::fs::write(&ordinary, b"first").unwrap();
    let mut writer = SizeRotatingWriter::new(ordinary.clone(), LogRotation::default()).unwrap();
    writer.write_all(b"second").unwrap();
    writer.flush().unwrap();
    assert_eq!(std::fs::read(ordinary).unwrap(), b"firstsecond");
}

/// A fixture subprocess is always owned, stopped and reaped before returning.
/// Null pipes keep this liveness oracle independent of output consumption.
fn bounded_child(command: &mut Command) -> Option<ExitStatus> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            return Some(status);
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            return None;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}
