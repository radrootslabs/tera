use super::*;
use std::io::{BufRead, Read, Write};

fn intent(content: &str) -> Phase1ReviseIntent {
    let event = "ab".repeat(32);
    let target = Phase1RevisionTarget::from_source(
        AddCommandType::CreateUpdate,
        CardId::derive(
            TodayCardType::Update,
            &CardSourceIdentity::Event(radroots_event::EventId::parse(&event).unwrap()),
        ),
        event,
        None,
        AUTHOR.to_owned(),
    )
    .unwrap();
    Phase1ReviseIntent::new(
        target,
        Phase1AddCommand::CreateUpdate(CreateUpdate::new(content).unwrap()),
        vec![],
        Phase1DraftFormSnapshot {
            content: content.into(),
            ..update_form()
        },
    )
    .unwrap()
}

#[tokio::test]
async fn repeat_preparation_is_one_graph_and_changed_capture_conflicts() {
    let runtime = runtime();
    let captured = intent("Original replacement");
    let first = runtime
        .prepare_revision_intent([101; 16], captured.clone())
        .await
        .unwrap();
    let id = *first.replacement().draft().draft_id().as_bytes();
    let child = revision_child_id(first.replacement().draft())
        .unwrap()
        .unwrap();
    assert_ne!(id, child);
    assert!(first.retraction().is_none());
    assert!(first.replacement().push().is_none());
    for _ in 0..3 {
        let replay = runtime
            .prepare_revision_intent([101; 16], captured.clone())
            .await
            .unwrap();
        assert_eq!(replay.replacement().draft(), first.replacement().draft());
        assert_eq!(
            revision_child_id(replay.replacement().draft()).unwrap(),
            Some(child)
        );
    }
    assert_eq!(
        runtime
            .prepare_revision_intent([101; 16], intent("Changed input"))
            .await
            .unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
    // Generic editing cannot erase the frozen revision graph, even before queueing.
    assert_eq!(
        runtime
            .phase1_save_draft_with_form(
                id,
                captured.command.clone(),
                1_900_000_000,
                vec![],
                captured.form.clone(),
                Some(1),
                first.replacement().draft().updated_at_unix_ms() + 1
            )
            .await
            .unwrap_err(),
        Phase1DraftError::RevisionConflict
    );
    let stopped = runtime.phase1_cancel_revision(id).await.unwrap();
    let replay = runtime
        .prepare_revision_intent([101; 16], captured.clone())
        .await
        .unwrap();
    assert_eq!(replay.replacement().draft(), stopped.replacement().draft());
    let independent = runtime
        .prepare_revision_intent([102; 16], captured)
        .await
        .unwrap();
    assert_ne!(
        independent.replacement().draft().draft_id(),
        first.replacement().draft().draft_id()
    );
    assert_eq!(runtime.phase1_draft_heads(100).await.unwrap().len(), 2);
}

#[tokio::test]
async fn preparation_rejects_unauthenticated_foreign_author_and_zero_request() {
    let anonymous = TeraRuntime::test_memory().unwrap();
    assert_eq!(
        anonymous
            .prepare_revision_intent([101; 16], intent("replacement"))
            .await
            .unwrap_err(),
        Phase1DraftError::IdentityUnavailable
    );
    let runtime = runtime();
    let mut foreign = intent("replacement");
    foreign.target.author_public_key = "ab".repeat(32);
    assert_eq!(
        runtime
            .prepare_revision_intent([101; 16], foreign)
            .await
            .unwrap_err(),
        Phase1DraftError::InvalidRevision
    );
    assert_eq!(
        runtime
            .prepare_revision_intent([0; 16], intent("replacement"))
            .await
            .unwrap_err(),
        Phase1DraftError::InvalidRevision
    );
    assert!(runtime.phase1_draft_heads(100).await.unwrap().is_empty());
}

#[tokio::test]
async fn original_form_requires_the_exact_signed_source_and_current_author() {
    let runtime = signing_runtime();
    let captured = intent("Original form");
    let source = runtime
        .phase1_save_draft_with_form(
            [42; 16],
            captured.command.clone(),
            1_900_000_000,
            vec![],
            captured.form.clone(),
            None,
            1_900_000_000_000,
        )
        .await
        .unwrap();
    assert_eq!(
        runtime
            .revision_source_form([42; 16], &captured.target)
            .await
            .unwrap_err(),
        Phase1DraftError::InvalidRevision
    );
    let queued = runtime
        .phase1_queue_draft(
            [42; 16],
            source.draft().revision().get(),
            policy(),
            1_900_000_000_001,
        )
        .await
        .unwrap();
    let signed = runtime
        .phase1_sign_queued_draft([42; 16], queued.draft().revision().get())
        .await
        .unwrap();
    let event = signed
        .push()
        .unwrap()
        .artifact()
        .signed()
        .unwrap()
        .event()
        .id()
        .to_hex();
    let target = Phase1RevisionTarget::from_source(
        AddCommandType::CreateUpdate,
        signed.card_id(),
        event,
        None,
        AUTHOR,
    )
    .unwrap();
    assert_eq!(
        runtime
            .revision_source_form([42; 16], &target)
            .await
            .unwrap(),
        captured.form
    );
    assert_eq!(
        runtime
            .revision_source_form([43; 16], &target)
            .await
            .unwrap_err(),
        Phase1DraftError::NotFound
    );
    let mut wrong = target.clone();
    wrong.author_public_key = "ab".repeat(32);
    assert_eq!(
        runtime
            .revision_source_form([42; 16], &wrong)
            .await
            .unwrap_err(),
        Phase1DraftError::InvalidRevision
    );
    assert_eq!(
        runtime
            .revision_source_form([42; 16], &captured.target)
            .await
            .unwrap_err(),
        Phase1DraftError::InvalidRevision
    );
    // A retained event without a lossless form remains explicitly unavailable.
    let no_form = runtime
        .phase1_save_draft(
            [44; 16],
            captured.command,
            1_900_000_000,
            vec![],
            None,
            1_900_000_000_000,
        )
        .await
        .unwrap();
    assert_eq!(no_form.card_id(), target.card_id());
    assert_eq!(
        runtime
            .revision_source_form([44; 16], &target)
            .await
            .unwrap_err(),
        Phase1DraftError::NotFound
    );
}

fn config(root: &std::path::Path) -> MobileUserStoreConfig {
    MobileUserStoreConfig::from_encoded(
        root,
        AUTHOR,
        &"04".repeat(32),
        1_900_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .unwrap()
}

const BARRIER: &str = "TERA_REVISION_CHILD_DURABLE";

#[tokio::test]
#[ignore = "private crash child exercised by revision_child_process_death_replays_one_graph"]
async fn revision_child() {
    let mut input = String::new();
    std::io::stdin()
        .take(4097)
        .read_to_string(&mut input)
        .unwrap();
    assert!(input.len() <= 4096);
    let root: String = serde_json::from_str(&input).unwrap();
    let store = config(std::path::Path::new(&root));
    std::fs::create_dir_all(store.owner_directory()).unwrap();
    let runtime = RuntimeBuilder::new(store).build().await.unwrap();
    let captured = intent("Crash-safe replacement");
    let saved = runtime
        .prepare_revision_intent([101; 16], captured.clone())
        .await
        .unwrap();
    let child = revision_child_id(saved.replacement().draft())
        .unwrap()
        .unwrap();
    // Exercise exactly the durable child creation boundary. Delivery eligibility
    // is independently tested by the coordinator and belongs to C102.
    runtime
        .phase1_create_revision_retraction(
            &captured.target,
            child,
            *saved.replacement().draft().draft_id().as_bytes(),
        )
        .await
        .unwrap();
    println!("{BARRIER}");
    std::io::stdout().flush().unwrap();
    std::future::pending::<()>().await;
}

struct Child(std::process::Child);
impl Drop for Child {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test]
async fn revision_child_process_death_replays_one_graph() {
    use std::process::{Command, Stdio};
    let root = tempfile::tempdir().unwrap();
    let mut child = Child(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                concat!(module_path!(), "::revision_child").trim_start_matches("tera_core::"),
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
    serde_json::to_writer(child.0.stdin.take().unwrap(), root.path().to_str().unwrap()).unwrap();
    let stdout = child.0.stdout.take().unwrap();
    let (send, receive) = std::sync::mpsc::sync_channel(1);
    let reader = std::thread::spawn(move || {
        for line in std::io::BufReader::new(stdout).lines() {
            if line.unwrap().ends_with(BARRIER) {
                send.send(()).unwrap();
                break;
            }
        }
    });
    receive
        .recv_timeout(std::time::Duration::from_secs(45))
        .expect("durable child boundary");
    child.0.kill().unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(child.0.wait().unwrap().signal(), Some(9));
    reader.join().unwrap();
    let runtime = RuntimeBuilder::new(config(root.path()))
        .build()
        .await
        .unwrap();
    let recovered = runtime
        .prepare_revision_intent([101; 16], intent("Crash-safe replacement"))
        .await
        .unwrap();
    let retained = recovered.retraction().unwrap().draft().clone();
    let child_id = revision_child_id(recovered.replacement().draft())
        .unwrap()
        .unwrap();
    assert_eq!(retained.draft_id().as_bytes(), &child_id);
    for _ in 0..3 {
        let replay = runtime
            .phase1_create_revision_retraction(
                recovered.target(),
                child_id,
                *recovered.replacement().draft().draft_id().as_bytes(),
            )
            .await
            .unwrap();
        assert_eq!(replay.draft(), &retained);
    }
    assert_eq!(runtime.phase1_draft_heads(100).await.unwrap().len(), 2);
    assert!(recovered.replacement().push().is_none());
    assert!(recovered.retraction().unwrap().push().is_none());
    runtime.shutdown().await.unwrap();
}
