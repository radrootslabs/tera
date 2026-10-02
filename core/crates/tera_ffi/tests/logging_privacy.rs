use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const CHILD_ROOT: &str = "TERA_LOG_PRIVACY_CHILD_ROOT";
const TEST_NAME: &str = "actual_file_and_stdout_sinks_exclude_private_material";
const CANARIES: [&str; 7] = [
    "plain private post text without a redaction keyword",
    "Bearer CANARY_AUTHORIZATION_VALUE",
    "49.123456,-123.987654 precise location",
    "/Users/private/canary.json",
    "nsec1CANARY_SECRET",
    "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
    "PRIVATE_TARGET_CANARY",
];

#[test]
fn actual_file_and_stdout_sinks_exclude_private_material() {
    if let Some(root) = std::env::var_os(CHILD_ROOT) {
        emit_and_drain(std::path::Path::new(&root));
        return;
    }
    let root = tempfile::tempdir().unwrap();
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", TEST_NAME, "--nocapture"])
        .env(CHILD_ROOT, root.path())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if child.try_wait().unwrap().is_some() {
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            panic!("bounded diagnostics sink process did not finish");
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    let stderr = String::from_utf8(output.stderr).unwrap();
    let file = std::fs::read_to_string(root.path().join("privacy.log")).unwrap();
    assert_safe(&stdout);
    assert_safe(&file);
    for canary in CANARIES {
        assert!(!stderr.contains(canary));
    }
    let lines = stdout
        .lines()
        .filter(|line| line.starts_with("level="))
        .collect::<Vec<_>>();
    assert_eq!(lines.len(), 6);
    assert_eq!(lines, file.lines().collect::<Vec<_>>());
}

fn emit_and_drain(root: &std::path::Path) {
    tera_ffi::logging::init_logging(
        Some(root.to_str().unwrap().to_owned()),
        Some("privacy.log".to_owned()),
        Some(true),
    )
    .unwrap();
    tera_ffi::logging::log_info(CANARIES[0].to_owned()).unwrap();
    tera_ffi::logging::log_error(CANARIES[1].to_owned()).unwrap();
    tera_ffi::logging::log_debug(CANARIES[2].to_owned()).unwrap();
    let span = tracing::info_span!("private_span", path = CANARIES[3], secret = CANARIES[4]);
    let _entered = span.enter();
    tracing::error!(target: "PRIVATE_TARGET_CANARY", authorization=CANARIES[1], content=CANARIES[0], location=CANARIES[2], id=CANARIES[5], message=CANARIES[3]);
    tracing::info!(
        code = "runtime_started",
        count = 7_u64,
        attempts = 2_i64,
        schema = 1_u64,
        latency_ms = 9_u64
    );
    tracing::warn!(code = CANARIES[0], count = u64::MAX, unknown = CANARIES[4]);
    let path = root.join("privacy.log");
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        if text.lines().count() >= 6 {
            assert_safe(&text);
            return;
        }
        assert!(
            Instant::now() < deadline,
            "bounded owned writer did not drain"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn assert_safe(text: &str) {
    for canary in CANARIES {
        assert!(!text.contains(canary));
    }
    assert!(!text.contains("private_span"));
    assert!(!text.contains(&u64::MAX.to_string()));
    assert!(text.contains("code=runtime_started count=7 attempts=2 schema=1 latency_ms=9"));
    assert!(text.contains("level=ERROR code=message_redacted"));
    assert!(text.len() < 4096);
}
