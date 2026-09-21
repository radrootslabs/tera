use std::{
    io::{BufRead, Read, Write},
    process::{Child, Command, Stdio},
    time::Duration,
};

use futures_util::{FutureExt, SinkExt, StreamExt};
use tokio::net::TcpListener;
use tokio_tungstenite::{accept_async, tungstenite::Message};

use super::{operation_test_support::*, test_support::*};

const CHILD_TEST: &str =
    "runtime::product_surface::submission::signed_artifact_tests::signed_artifact_child";
const DURABLE: &str = "TERA_SIGNED_ARTIFACT_COMMITTED";

struct ChildGuard(Child);
impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
#[ignore = "private child entrypoint exercised by signed_artifact_process_death_preserves_exact_delivery"]
async fn signed_artifact_child() {
    let mut input = String::new();
    std::io::stdin()
        .take(4097)
        .read_to_string(&mut input)
        .unwrap();
    assert!(input.len() <= 4096);
    let (root, relay): (String, String) = serde_json::from_str(&input).unwrap();
    let signer = CountingSigner::new();
    let runtime = runtime(Some(std::path::Path::new(&root)), signer.clone(), &relay).await;
    let request = request();
    prepare(&runtime, &request, false).await;
    runtime.submission_queue(&request, 1).await.unwrap();
    let (loaded, _) = runtime.load_submission_operation(&request).await.unwrap();
    runtime
        .sync()
        .unwrap()
        .sign_prepared(loaded.request)
        .await
        .unwrap();
    let signed = runtime.submission_operation_status(&request).await.unwrap();
    assert!(signed.push().artifact().signed().is_some());
    assert!(!signed.push().artifact().admission_state().is_admitted());
    assert!(signed.push().delivery_plan().attempts().is_empty());
    assert_eq!(signer.count(), 1);
    println!("{DURABLE}");
    std::io::stdout().flush().unwrap();
    // The parent kills this exact child without shutdown, Drop or WAL checkpoint.
    std::future::pending::<()>().await;
}

pub(super) fn kill_after_signed_commit(root: &std::path::Path, relay: &str) {
    kill_at_barrier(root, relay, CHILD_TEST, DURABLE);
}

pub(super) fn kill_at_barrier(
    root: &std::path::Path,
    relay: &str,
    child_test: &str,
    barrier: &str,
) {
    let mut child = ChildGuard(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                child_test,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .unwrap(),
    );
    {
        let mut stdin = child.0.stdin.take().unwrap();
        serde_json::to_writer(&mut stdin, &(root.to_str().unwrap(), relay)).unwrap();
    }
    let stdout = child.0.stdout.take().unwrap();
    let barrier = barrier.to_owned();
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            if line.unwrap().ends_with(&barrier) {
                let _ = send.send(());
                break;
            }
        }
    });
    receive
        .recv_timeout(Duration::from_secs(45))
        .expect("bounded exact durable child barrier");
    child.0.kill().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(child.0.wait().unwrap().signal(), Some(9));
    reader.join().unwrap();
}

async fn receive_event(listener: TcpListener) -> String {
    tokio::time::timeout(Duration::from_secs(15), async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut socket = accept_async(stream).await.unwrap();
        while let Some(message) = socket.next().await {
            if let Message::Text(text) = message.unwrap() {
                let value: serde_json::Value = serde_json::from_str(&text).unwrap();
                if value[0] == "EVENT" {
                    socket
                        .send(Message::Text(
                            serde_json::json!(["OK", value[1]["id"], true, ""])
                                .to_string()
                                .into(),
                        ))
                        .await
                        .unwrap();
                    return text.to_string();
                }
            }
        }
        panic!("relay closed without event");
    })
    .await
    .unwrap()
}

#[tokio::test]
async fn signed_artifact_process_death_preserves_exact_delivery() {
    let root = tempfile::tempdir().unwrap();
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let relay = format!("ws://{}", listener.local_addr().unwrap());
    kill_after_signed_commit(root.path(), &relay);
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err()
    );
    let signer = CountingSigner::new();
    let runtime = runtime(Some(root.path()), signer.clone(), &relay).await;
    let request = request();
    let recovered = runtime.submission_operation_status(&request).await.unwrap();
    assert!(!recovered.push().artifact().admission_state().is_admitted());
    assert!(recovered.push().delivery_plan().attempts().is_empty());
    let raw = recovered
        .push()
        .artifact()
        .signed()
        .unwrap()
        .event()
        .raw_json()
        .to_owned();
    assert_eq!(
        recovered
            .push()
            .delivery_plan()
            .request()
            .unwrap()
            .payload()
            .event()
            .raw_json(),
        raw
    );
    let relay_task = tokio::spawn(receive_event(listener));
    let delivered = runtime
        .submission_advance(&request, recovered.intent().revision().get())
        .await
        .unwrap();
    assert_eq!(
        relay_task.await.unwrap(),
        format!("[\"EVENT\",{raw}]"),
        "the socket must receive the exact persisted event bytes, not a reserialized envelope"
    );
    assert_eq!(
        delivered
            .push()
            .artifact()
            .signed()
            .unwrap()
            .event()
            .raw_json(),
        raw
    );
    assert!(delivered.push().artifact().admission_state().is_admitted());
    assert_eq!(
        signer.count(),
        0,
        "recovery must not ask for another signature"
    );
    assert_eq!(delivered.receipt(), recovered.receipt());
    runtime.shutdown().await.unwrap();
}

#[tokio::test]
async fn delivery_without_persisted_signed_bytes_has_no_effects() {
    for sqlite in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let relay = format!("ws://{}", listener.local_addr().unwrap());
        let signer = CountingSigner::new();
        let runtime = runtime(sqlite.then_some(root.path()), signer.clone(), &relay).await;
        let request = request();
        prepare(&runtime, &request, false).await;
        let before = runtime.submission_operation_status(&request).await.unwrap();
        assert!(before.push().artifact().signed().is_none());
        assert!(before.push().delivery_plan().attempts().is_empty());
        let (loaded, _) = runtime.load_submission_operation(&request).await.unwrap();
        assert_eq!(
            runtime
                .sync()
                .unwrap()
                .deliver_push(loaded.request.operation_id())
                .await
                .unwrap_err(),
            radroots_sync::policy::Error::InvalidSignerOutput
        );
        assert_eq!(
            runtime.submission_operation_status(&request).await.unwrap(),
            before
        );
        assert_eq!(signer.count(), 0);
        assert!(listener.accept().now_or_never().is_none());
        runtime.shutdown().await.unwrap();
    }
}
