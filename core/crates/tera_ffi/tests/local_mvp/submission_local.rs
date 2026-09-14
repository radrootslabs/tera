//! Scoped local recovery through the real public boundary and protocol services.
use super::*;
use tera_ffi::{
    FfiComposerFormRecord, FfiComposerMediaRecord, FfiComposerSaveRequest, FfiComposerScopeRecord,
    FfiSubmissionMediaInput, FfiSubmissionOperationRecord, FfiSubmissionReservationRequest,
    FfiSubmissionUploadResponse, composer_reserve_id, submission_reserve_id,
};

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn all_five_scoped_families_repair_local_visibility_without_republication() {
    let relay = MockRelay::run().await.unwrap();
    let relay_url = relay.url().await.to_string();
    let bytes = png(2, 3);
    let blossom = BlossomServer::spawn(bytes.clone(), false).await;
    let mut file = tempfile::tempfile().unwrap();
    file.write_all(&bytes).unwrap();
    let media = prepared_media(&blossom.origin, &bytes, &file, "Harvest photo");
    let root = tempfile::tempdir().unwrap();
    support::prepare(root.path());
    let runtime = runtime_with_signer(root.path()).await;
    configure_simulator(&runtime, &relay_url, &blossom.origin).await;
    let context = local_network(&relay_url);
    let mut operations = Vec::new();
    for input in [
        add_input(FfiAddCommandType::CreateUpdate, "Local update", None),
        add_input(FfiAddCommandType::CreateAsk, "Local ask", None),
        event_input("Local event"),
        food_input("Local food", "local-repair-food"),
        add_input(
            FfiAddCommandType::CreatePhotoUpdate,
            "Local photo",
            Some(media),
        ),
    ] {
        operations.push(publish(&runtime, input, &bytes).await);
    }
    blossom.finish().await;
    runtime.shutdown().await.unwrap();
    drop(runtime);

    // Reconstruct with no host signer. Local recovery cannot obtain another
    // signature, and the original operation facts remain the sole authority.
    let runtime = TeraRuntime::new(
        root.path().to_string_lossy().into_owned(),
        support::PUBLIC_KEY.into(),
        support::GENERATION.into(),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .await
    .unwrap();
    runtime
        .configure_simulator_relays(vec![relay_url.clone()])
        .await
        .unwrap();
    for operation in &operations {
        assert_eq!(operation.settlement.signed, 1);
        assert_eq!(operation.settlement.admitted, 1);
        assert_eq!(operation.settlement.delivery_satisfied, 1);
        for _ in 0..3 {
            let repaired = runtime
                .submission_reconcile_local(operation.request.clone(), context.clone())
                .await
                .unwrap();
            assert_eq!(&repaired, operation);
        }
    }
    let cards = collect_pages(&runtime, &context, 2, unix_time_ms() / 1000).await;
    assert_eq!(cards.len(), 5);
    for operation in &operations {
        let card = cards
            .iter()
            .find(|card| card.local_operation_id.as_ref() == Some(&operation.operation_id))
            .expect("exact author operation overlay");
        assert_eq!(card.local_operation_state.as_deref(), Some("complete"));
    }
    let (sender, receiver) = std::sync::mpsc::channel();
    let subscription = runtime
        .subscribe_changes(Box::new(Observer(sender)))
        .unwrap();
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(1)).unwrap().kind,
        tera_ffi::FfiRuntimeChangeKind::Initial
    );
    for operation in &operations {
        assert_eq!(
            runtime
                .submission_reconcile_local(operation.request.clone(), context.clone())
                .await
                .unwrap(),
            *operation
        );
    }
    assert!(
        receiver.recv_timeout(Duration::from_millis(100)).is_err(),
        "idempotent repair must not restart an observer recovery loop"
    );
    subscription.unsubscribe();
    let reader = Client::new(Keys::parse(REPLY_SECRET).unwrap());
    reader.add_relay(&relay_url).await.unwrap();
    reader.connect().await;
    reader.wait_for_connection(Duration::from_secs(2)).await;
    let events = reader
        .fetch_events(
            nostr::Filter::new().author(Keys::parse(FIXTURE_SECRET).unwrap().public_key()),
            Duration::from_secs(5),
        )
        .await
        .unwrap();
    assert_eq!(events.len(), 5, "local repair creates no additional event");
    for card in &cards {
        assert!(
            events
                .iter()
                .any(|event| event.id.to_hex() == card.source_event_id)
        );
    }
    reader.disconnect().await;
    runtime.shutdown().await.unwrap();
    relay.shutdown();
}

struct Observer(std::sync::mpsc::Sender<tera_ffi::FfiRuntimeChangeRecord>);

impl tera_ffi::TeraRuntimeObserver for Observer {
    fn on_change(&self, change: tera_ffi::FfiRuntimeChangeRecord) {
        self.0.send(change).unwrap();
    }
}

async fn publish(
    runtime: &TeraRuntime,
    input: FfiAddDraftInput,
    bytes: &[u8],
) -> FfiSubmissionOperationRecord {
    let media = input.media.clone();
    let source = FfiComposerSaveRequest {
        schema_version: 1,
        scope: FfiComposerScopeRecord {
            schema_version: 1,
            author_public_key: support::PUBLIC_KEY.into(),
            local_network_id: "local-mvp".into(),
        },
        id: composer_reserve_id().unwrap().id,
        expected_revision: None,
        edit_sequence: 1,
        form: form(input),
    };
    runtime.composer_save(source.clone()).await.unwrap();
    let request = FfiSubmissionReservationRequest {
        schema_version: 1,
        command_id: submission_reserve_id().unwrap().id,
        scope: source.scope,
        composer_id: source.id,
        expected_revision: 1,
    };
    let mut status = runtime
        .submission_prepare(request.clone(), media.clone())
        .await
        .unwrap();
    if let Some(media) = media.into_iter().next() {
        let mut input = FfiSubmissionMediaInput {
            schema_version: 1,
            request: request.clone(),
            expected_revision: status.revision,
            media,
        };
        let job = runtime
            .submission_prepare_upload(input.clone())
            .await
            .unwrap();
        let url = job.upload_url.strip_prefix("http://").unwrap();
        let (authority, path) = url.split_once('/').unwrap();
        let mut stream = tokio::net::TcpStream::connect(authority).await.unwrap();
        let header = format!(
            "PUT /{path} HTTP/1.1\r\nHost: {authority}\r\nAuthorization: {}\r\nX-Sha-256: {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            job.authorization_header,
            job.expected_sha256,
            job.media_type,
            bytes.len(),
        );
        stream.write_all(header.as_bytes()).await.unwrap();
        stream.write_all(bytes).await.unwrap();
        let mut response = Vec::new();
        tokio::time::timeout(Duration::from_secs(5), stream.read_to_end(&mut response))
            .await
            .unwrap()
            .unwrap();
        assert!(response.starts_with(b"HTTP/1.1 200 OK\r\n"));
        let body = response
            .windows(4)
            .position(|part| part == b"\r\n\r\n")
            .unwrap()
            + 4;
        input.expected_revision = job.submission.revision;
        status = runtime
            .submission_complete_upload(
                input,
                FfiSubmissionUploadResponse {
                    schema_version: 1,
                    status_code: 200,
                    media_type: Some("application/json".into()),
                    content_encoding: None,
                    body: response[body..].to_vec(),
                },
            )
            .await
            .unwrap();
    }
    runtime
        .submission_advance(request, status.revision)
        .await
        .unwrap()
}

fn form(input: FfiAddDraftInput) -> FfiComposerFormRecord {
    FfiComposerFormRecord {
        schema_version: 1,
        command_type: input.command_type,
        content: input.content,
        identifier: input.identifier,
        title: input.title,
        summary: input.summary,
        location: input.location,
        event_timing: input.event_timing,
        event_start_date: input.event_start_date,
        event_end_date: input.event_end_date,
        event_start_unix_s: input.event_start_unix_s,
        event_end_unix_s: input.event_end_unix_s,
        event_timezone: input.event_timezone,
        price_amount: input.price_amount,
        currency: input.currency,
        unit: input.unit,
        quantity: input.quantity,
        food_published_at_unix_s: input.food_published_at_unix_s,
        food_status: input.food_status,
        media: input
            .media
            .into_iter()
            .map(|media| FfiComposerMediaRecord {
                schema_version: 1,
                opaque_reference: media.opaque_reference,
                sha256: media.sha256,
                media_type: media.media_type,
                byte_size: media.byte_size,
                width: media.width,
                height: media.height,
                alt: media.alt,
                prepared_at_unix_s: media.prepared_at_unix_s,
            })
            .collect(),
    }
}
