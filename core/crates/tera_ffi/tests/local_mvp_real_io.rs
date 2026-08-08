use std::io::Write;
use std::os::fd::AsRawFd;
use std::time::Duration;

use nostr::{EventBuilder, Keys, Kind, Metadata, Tag, Timestamp};
use nostr_relay_builder::MockRelay;
use nostr_sdk::Client;
use radroots_blossom::Sha256;
use radroots_mobile_ffi::{
    FfiAddCommandType, FfiAddDraftInput, FfiBlossomEndpointAuthority, FfiBlossomHostKind,
    FfiBlossomUploadInput, FfiCancellationPolicy, FfiEventTimingKind, FfiLocalNetworkRecord,
    FfiMediaStage, FfiOutboxState, FfiPreparedMediaInput, FfiQueuePolicyRecord,
    FfiRelaySatisfaction, FfiRetractionDraftInput, FfiTodayCardType, FfiTodayProjectionUpdate,
    FfiTodayRelaySyncState, HostSigningOutcome, HostSigningRequest, HostSigningResult,
    MOBILE_FFI_SCHEMA_VERSION, ProtectedDataAvailability, RadrootsHostSigner, RadrootsRuntime,
    SignerAvailabilityRecord, SignerStatusRecord,
};
use secp256k1::{Keypair, Message, Secp256k1, SecretKey};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[allow(dead_code)]
mod support;

const AUTHORED_AT: u64 = 1_786_000_000;
const AS_OF: u64 = 1_786_200_000;
const FIXTURE_SECRET: &str = "0000000000000000000000000000000000000000000000000000000000000001";
const REPLY_SECRET: &str = "0000000000000000000000000000000000000000000000000000000000000002";

struct FixtureHostSigner;

#[async_trait::async_trait]
impl RadrootsHostSigner for FixtureHostSigner {
    async fn signer_status(&self) -> SignerStatusRecord {
        SignerStatusRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            availability: SignerAvailabilityRecord::Ready,
        }
    }

    async fn sign(&self, request: HostSigningRequest) -> HostSigningResult {
        let secret = SecretKey::from_slice(&hex::decode(FIXTURE_SECRET).expect("fixture secret"))
            .expect("valid fixture secret");
        let keypair = Keypair::from_secret_key(&Secp256k1::new(), &secret);
        let digest: [u8; 32] = request
            .event_id_digest
            .clone()
            .try_into()
            .expect("32-byte event digest");
        assert_eq!(hex::encode(digest), request.expected_event_id);
        assert_eq!(
            keypair.x_only_public_key().0.to_string(),
            request.public_key
        );
        let signature = Secp256k1::new()
            .sign_schnorr_no_aux_rand(&Message::from_digest(digest), &keypair)
            .to_string();
        HostSigningResult {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            outcome: HostSigningOutcome::Signed,
            operation_id: request.operation_id,
            signer_request_id: request.signer_request_id,
            public_key: request.public_key,
            purpose: request.purpose,
            signature_hex: Some(signature),
            completed_at_unix_ms: unix_time_ms(),
        }
    }
}

struct BlossomServer {
    origin: String,
    task: tokio::task::JoinHandle<()>,
}

impl BlossomServer {
    async fn spawn(bytes: Vec<u8>, corrupt_retrieval: bool) -> Self {
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind Blossom server");
        let origin = format!("http://{}", listener.local_addr().expect("Blossom address"));
        let hash = Sha256::digest(bytes.as_slice()).to_string();
        let blob_url = format!("{origin}/{hash}.png");
        let descriptor = serde_json::to_vec(&serde_json::json!({
            "url": blob_url,
            "sha256": hash,
            "size": bytes.len(),
            "type": "image/png",
            "uploaded": AUTHORED_AT,
        }))
        .expect("descriptor JSON");
        let task = tokio::spawn(async move {
            let (mut upload, _) = listener.accept().await.expect("accept Blossom upload");
            let request = read_http_request(&mut upload).await;
            let request_text = String::from_utf8_lossy(&request);
            assert!(request_text.starts_with("PUT /upload HTTP/1.1\r\n"));
            assert!(
                request_text
                    .to_ascii_lowercase()
                    .contains("authorization: nostr ")
            );
            assert!(
                request_text
                    .to_ascii_lowercase()
                    .contains(format!("x-sha-256: {hash}").as_str())
            );
            write_http_response(&mut upload, "application/json", &descriptor).await;

            let (mut retrieval, _) = listener.accept().await.expect("accept Blossom retrieval");
            let request = read_http_request(&mut retrieval).await;
            assert!(
                String::from_utf8_lossy(&request)
                    .starts_with(format!("GET /{hash}.png HTTP/1.1\r\n").as_str())
            );
            let mut response_bytes = bytes;
            if corrupt_retrieval {
                response_bytes[0] ^= 1;
            }
            write_http_response(&mut retrieval, "image/png", &response_bytes).await;
        });
        Self { origin, task }
    }

    async fn finish(self) {
        self.task.await.expect("Blossom server task");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn public_runtime_completes_the_local_mvp_against_real_protocol_services() {
    let relay = MockRelay::run().await.expect("local NIP-01 relay");
    let relay_url = relay.url().await.to_string();
    let image_bytes = png(2, 3);
    let blossom = BlossomServer::spawn(image_bytes.clone(), false).await;
    let mut image_file = tempfile::tempfile().expect("media file");
    image_file.write_all(&image_bytes).expect("write media");
    let media = prepared_media(&blossom.origin, &image_bytes, &image_file, "Harvest photo");

    let publisher_root = tempfile::tempdir().expect("publisher root");
    support::prepare(publisher_root.path());
    let publisher = runtime_with_signer(publisher_root.path()).await;
    configure_simulator(&publisher, &relay_url, &blossom.origin);
    let context = local_network(&relay_url);

    let update_id = draft_id(1);
    let update = publisher
        .phase1_save_draft(
            update_id.clone(),
            add_input(
                FfiAddCommandType::CreateUpdate,
                "Equal-time harvest update",
                None,
            ),
            AUTHORED_AT,
            None,
            1_800_000_000_000,
        )
        .await
        .expect("save offline update");
    let queued_update = queue(
        &publisher,
        &update_id,
        update.revision,
        &relay_url,
        false,
        1_800_000_000_500,
    )
    .await;
    assert_eq!(queued_update.state, FfiOutboxState::Queued);
    publisher
        .shutdown()
        .await
        .expect("shutdown before delivery");

    let publisher = runtime_with_signer(publisher_root.path()).await;
    configure_simulator(&publisher, &relay_url, &blossom.origin);
    let recovered = publisher
        .phase1_recover_draft_queue(update_id.clone(), 1_800_000_001_000)
        .await
        .expect("recover queued update after restart");
    assert_eq!(recovered.state, FfiOutboxState::Queued);
    advance_complete(&publisher, &update_id, recovered.revision).await;

    let flows = [
        (
            2,
            add_input(
                FfiAddCommandType::CreatePhotoUpdate,
                "Equal-time photo harvest",
                Some(media.clone()),
            ),
        ),
        (
            3,
            add_input(
                FfiAddCommandType::CreateAsk,
                "Equal-time ask: who has basil?",
                None,
            ),
        ),
        (4, event_input("Equal-time Saturday market")),
        (5, food_input("Equal-time carrots", "today-carrots")),
    ];
    for (index, input) in flows {
        let id = draft_id(index);
        let saved = publisher
            .phase1_save_draft(
                id.clone(),
                input,
                AUTHORED_AT,
                None,
                1_800_000_000_000 + u64::from(index),
            )
            .await
            .expect("save authored flow");
        let ready = if index == 2 {
            let uploaded = publisher
                .phase1_upload_draft_media(FfiBlossomUploadInput {
                    schema_version: MOBILE_FFI_SCHEMA_VERSION,
                    draft_id: id.clone(),
                    expected_revision: saved.revision,
                    media: media.clone(),
                    authorization_content: "Upload the exact local harvest image".to_owned(),
                    authorization_created_at_unix_s: AUTHORED_AT,
                    authorization_lifetime_seconds: 300,
                    operation_id: "21".repeat(16),
                    artifact_id: "22".repeat(16),
                    signing_deadline_unix_ms: u64::MAX,
                    signing_cancellation: FfiCancellationPolicy::LocalCooperative,
                    verified_at_unix_ms: 1_800_000_000_100,
                    updated_at_unix_ms: 1_800_000_000_200,
                })
                .await
                .expect("upload and re-fetch exact media");
            assert_eq!(uploaded.media[0].stage, FfiMediaStage::Verified);
            uploaded
        } else {
            saved
        };
        let queued = queue(
            &publisher,
            &id,
            ready.revision,
            &relay_url,
            false,
            1_800_000_000_500 + u64::from(index),
        )
        .await;
        advance_complete(&publisher, &id, queued.revision).await;
    }
    blossom.finish().await;

    let reader_root = tempfile::tempdir().expect("fresh reader root");
    support::prepare(reader_root.path());
    let reader = RadrootsRuntime::new(
        reader_root.path().to_string_lossy().into_owned(),
        support::PUBLIC_KEY.to_owned(),
        support::GENERATION.to_owned(),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
    )
    .await
    .expect("fresh reader runtime");
    reader
        .configure_simulator_relays(vec![relay_url.clone()])
        .expect("reader relay profile");
    let first_sync = reader
        .phase1_sync_today(context.clone(), AS_OF, FfiTodayProjectionUpdate::Rebuild)
        .await
        .expect("fresh relay re-read");
    assert_eq!(first_sync.relay_state, FfiTodayRelaySyncState::Complete);
    assert_eq!(first_sync.events_admitted, 5);

    let cards = collect_pages(&reader, &context, 2, AS_OF).await;
    assert_eq!(cards.len(), 5, "all equal-time cards survive frozen paging");
    let mut card_types = cards.iter().map(|card| card.card_type).collect::<Vec<_>>();
    card_types.sort_by_key(|card_type| match card_type {
        FfiTodayCardType::Update => 0,
        FfiTodayCardType::PhotoUpdate => 1,
        FfiTodayCardType::Ask => 2,
        FfiTodayCardType::Event => 3,
        FfiTodayCardType::FoodAvailability => 4,
    });
    assert_eq!(
        card_types,
        vec![
            FfiTodayCardType::Update,
            FfiTodayCardType::PhotoUpdate,
            FfiTodayCardType::Ask,
            FfiTodayCardType::Event,
            FfiTodayCardType::FoodAvailability,
        ]
    );
    let photo = cards
        .iter()
        .find(|card| card.card_type == FfiTodayCardType::PhotoUpdate)
        .expect("photo card");
    assert_eq!(photo.media.len(), 1);
    assert_eq!(
        photo.media[0].sha256.as_deref(),
        Some(media.sha256.as_str())
    );
    let update = cards
        .iter()
        .find(|card| card.card_type == FfiTodayCardType::Update)
        .expect("update card");
    let food = cards
        .iter()
        .find(|card| card.card_type == FfiTodayCardType::FoodAvailability)
        .expect("food card");
    assert_eq!(food.food_summary.as_deref(), Some("Freshly harvested"));
    assert_eq!(food.food_published_at_unix_s, Some(AUTHORED_AT));
    assert_eq!(food.food_status.as_deref(), Some("active"));
    let update_event_id = update.source_event_id.clone();
    let update_card_id = update.card_id.clone();
    let food_event_id = food.source_event_id.clone();
    let food_card_id = food.card_id.clone();

    publish_supporting_events(&relay_url, &update_event_id, &food_event_id).await;
    reader
        .phase1_sync_today(
            context.clone(),
            AS_OF,
            FfiTodayProjectionUpdate::Incremental,
        )
        .await
        .expect("sync profile and thread events");
    let enriched = reader
        .phase1_today_page(context.clone(), 20, Some(AS_OF), None)
        .await
        .expect("enriched Today");
    assert_eq!(
        enriched
            .items
            .iter()
            .find(|card| card.card_id == update_card_id)
            .expect("reply root")
            .thread
            .len(),
        1
    );
    assert_eq!(
        enriched
            .items
            .iter()
            .find(|card| card.card_id == food_card_id)
            .expect("comment root")
            .thread
            .len(),
        1
    );

    let offline_port = unused_loopback_port().await;
    let replacement_id = draft_id(6);
    let mut replacement_input = food_input("Corrected carrots from Moss Farm", "today-carrots");
    replacement_input.food_published_at_unix_s = Some(AUTHORED_AT);
    let replacement = publisher
        .phase1_save_draft(
            replacement_id.clone(),
            replacement_input,
            AUTHORED_AT + 10,
            None,
            1_800_000_010_000,
        )
        .await
        .expect("save replacement");
    let replacement_queued = publisher
        .phase1_queue_draft(
            replacement_id.clone(),
            replacement.revision,
            FfiQueuePolicyRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                relay_urls: vec![relay_url.clone(), format!("ws://127.0.0.1:{offline_port}")],
                satisfaction: FfiRelaySatisfaction::AllAccepted,
                delivery_deadline_unix_ms: u64::MAX,
                cancellation: FfiCancellationPolicy::LocalCooperative,
            },
            1_800_000_010_100,
        )
        .await
        .expect("queue replacement");
    let partial = publisher
        .phase1_advance_draft(replacement_id, replacement_queued.revision)
        .await
        .expect("attempt partial replacement delivery");
    assert_eq!(
        partial.state,
        FfiOutboxState::PartiallyDelivered,
        "partial delivery status: {partial:?}"
    );
    let settlement = partial.settlement.expect("partial settlement");
    assert_eq!(settlement.delivery_satisfied, 0);
    assert_eq!(settlement.delivery_exhausted, 1);

    reader
        .phase1_sync_today(
            context.clone(),
            AS_OF,
            FfiTodayProjectionUpdate::Incremental,
        )
        .await
        .expect("sync replacement");
    let replaced = reader
        .phase1_today_page(context.clone(), 20, Some(AS_OF), None)
        .await
        .expect("replacement projection");
    let current_food = replaced
        .items
        .iter()
        .find(|card| card.card_id == food_card_id)
        .expect("current food head");
    assert_eq!(current_food.content, "Corrected carrots from Moss Farm");
    assert_ne!(current_food.source_event_id, food_event_id);

    let retraction_id = draft_id(7);
    let retraction = publisher
        .phase1_save_retraction_draft(
            retraction_id.clone(),
            FfiRetractionDraftInput {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                command_type: FfiAddCommandType::CreateUpdate,
                target_card_id: update_card_id.clone(),
                target_event_id: update_event_id,
                target_kind: 1,
                target_address: None,
                reason: "Superseded local update".to_owned(),
            },
            AUTHORED_AT + 20,
            1_800_000_020_000,
        )
        .await
        .expect("save deletion request");
    let retraction_queued = queue(
        &publisher,
        &retraction_id,
        retraction.revision,
        &relay_url,
        false,
        1_800_000_020_100,
    )
    .await;
    advance_complete(&publisher, &retraction_id, retraction_queued.revision).await;
    reader
        .phase1_sync_today(
            context.clone(),
            AS_OF,
            FfiTodayProjectionUpdate::Incremental,
        )
        .await
        .expect("sync deletion");
    let after_deletion = reader
        .phase1_today_page(context.clone(), 20, Some(AS_OF), None)
        .await
        .expect("projection after deletion");
    assert_eq!(after_deletion.items.len(), 4);
    assert!(
        after_deletion
            .items
            .iter()
            .all(|card| card.card_id != update_card_id)
    );

    let search = reader
        .phase1_search(context.clone(), "moss farm".to_owned(), 20, AS_OF)
        .await
        .expect("search current projection");
    assert!(search.iter().any(|result| result.profile.is_some()));
    assert!(search.iter().any(|result| {
        result
            .card
            .as_ref()
            .is_some_and(|card| card.content == "Corrected carrots from Moss Farm")
    }));
    let me = reader
        .phase1_me(context.clone(), AS_OF)
        .await
        .expect("Me projection");
    assert_eq!(
        me.profile
            .as_ref()
            .and_then(|profile| profile.display_name.as_deref()),
        Some("Moss Farm")
    );
    assert_eq!(me.cards.len(), 4);

    prove_corrupted_media_fails(&publisher, &image_bytes, &image_file).await;

    reader.shutdown().await.expect("reader shutdown");
    publisher.shutdown().await.expect("publisher shutdown");
    relay.shutdown();
}

async fn runtime_with_signer(root: &std::path::Path) -> RadrootsRuntime {
    RadrootsRuntime::with_host_signer(
        root.to_string_lossy().into_owned(),
        support::PUBLIC_KEY.to_owned(),
        support::GENERATION.to_owned(),
        1_800_000_000_000,
        ProtectedDataAvailability::Available,
        Box::new(FixtureHostSigner),
    )
    .await
    .expect("runtime with fixture host signer")
}

fn configure_simulator(runtime: &RadrootsRuntime, relay_url: &str, blossom_origin: &str) {
    runtime
        .configure_simulator_relays(vec![relay_url.to_owned()])
        .expect("simulator relay profile");
    runtime
        .configure_blossom(
            FfiBlossomHostKind::Simulator,
            FfiBlossomEndpointAuthority::LoopbackDevelopment,
            blossom_origin.to_owned(),
            vec![],
        )
        .expect("simulator Blossom profile");
}

fn local_network(relay_url: &str) -> FfiLocalNetworkRecord {
    FfiLocalNetworkRecord {
        schema_version: MOBILE_FFI_SCHEMA_VERSION,
        id: "local-mvp".to_owned(),
        label: "Local MVP".to_owned(),
        relay_urls: vec![relay_url.to_owned()],
        locality: None,
        followed_authors: Vec::new(),
        generation: 1,
    }
}

fn add_input(
    command_type: FfiAddCommandType,
    content: &str,
    media: Option<FfiPreparedMediaInput>,
) -> FfiAddDraftInput {
    FfiAddDraftInput {
        schema_version: MOBILE_FFI_SCHEMA_VERSION,
        command_type,
        content: content.to_owned(),
        identifier: None,
        title: None,
        summary: None,
        location: None,
        event_timing: None,
        event_start_date: None,
        event_end_date: None,
        event_start_unix_s: None,
        event_end_unix_s: None,
        event_timezone: None,
        price_amount: None,
        currency: None,
        unit: None,
        quantity: None,
        food_published_at_unix_s: None,
        food_status: None,
        media: media.into_iter().collect(),
    }
}

fn event_input(content: &str) -> FfiAddDraftInput {
    FfiAddDraftInput {
        identifier: Some("saturday-market".to_owned()),
        title: Some("Saturday Market".to_owned()),
        location: Some("Victoria".to_owned()),
        event_timing: Some(FfiEventTimingKind::AllDay),
        event_start_date: Some("2026-08-09".to_owned()),
        ..add_input(FfiAddCommandType::CreateEvent, content, None)
    }
}

fn food_input(content: &str, identifier: &str) -> FfiAddDraftInput {
    FfiAddDraftInput {
        identifier: Some(identifier.to_owned()),
        title: Some("Carrots".to_owned()),
        summary: Some("Freshly harvested".to_owned()),
        location: Some("Victoria".to_owned()),
        price_amount: Some("4.5".to_owned()),
        currency: Some("CAD".to_owned()),
        unit: Some("kg".to_owned()),
        quantity: Some("12".to_owned()),
        food_status: Some("active".to_owned()),
        ..add_input(FfiAddCommandType::CreateFoodAvailability, content, None)
    }
}

fn prepared_media(
    _origin: &str,
    bytes: &[u8],
    file: &std::fs::File,
    alt: &str,
) -> FfiPreparedMediaInput {
    let hash = Sha256::digest(bytes).to_string();
    FfiPreparedMediaInput {
        schema_version: MOBILE_FFI_SCHEMA_VERSION,
        opaque_reference: format!("media:{hash}"),
        file_descriptor: u64::try_from(file.as_raw_fd()).expect("nonnegative media descriptor"),
        sha256: hash,
        media_type: "image/png".to_owned(),
        byte_size: u64::try_from(bytes.len()).expect("media size"),
        width: 2,
        height: 3,
        alt: alt.to_owned(),
        prepared_at_unix_s: AUTHORED_AT,
    }
}

async fn queue(
    runtime: &RadrootsRuntime,
    draft_id: &str,
    revision: u64,
    relay_url: &str,
    all: bool,
    queued_at_unix_ms: u64,
) -> radroots_mobile_ffi::FfiDraftStatusRecord {
    runtime
        .phase1_queue_draft(
            draft_id.to_owned(),
            revision,
            FfiQueuePolicyRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                relay_urls: vec![relay_url.to_owned()],
                satisfaction: if all {
                    FfiRelaySatisfaction::AllAccepted
                } else {
                    FfiRelaySatisfaction::AnyAccepted
                },
                delivery_deadline_unix_ms: u64::MAX,
                cancellation: FfiCancellationPolicy::LocalCooperative,
            },
            queued_at_unix_ms,
        )
        .await
        .expect("queue draft")
}

async fn advance_complete(runtime: &RadrootsRuntime, draft_id: &str, revision: u64) {
    let status = match runtime
        .phase1_advance_draft(draft_id.to_owned(), revision)
        .await
    {
        Ok(status) => status,
        Err(error) => {
            let durable = runtime
                .phase1_draft_status(draft_id.to_owned())
                .await
                .expect("durable failure status");
            panic!("advance failed: {error:?}; durable status: {durable:?}");
        }
    };
    assert_eq!(status.state, FfiOutboxState::Complete);
    let settlement = status.settlement.expect("complete settlement");
    assert_eq!(settlement.signed, 1);
    assert_eq!(settlement.admitted, 1);
    assert_eq!(settlement.delivery_satisfied, 1);
}

async fn collect_pages(
    runtime: &RadrootsRuntime,
    context: &FfiLocalNetworkRecord,
    limit: u16,
    as_of: u64,
) -> Vec<radroots_mobile_ffi::FfiTodayCardRecord> {
    let first = runtime
        .phase1_today_page(context.clone(), limit, Some(as_of), None)
        .await
        .expect("first Today page");
    let mut items = first.items;
    let mut cursor = first.next_cursor;
    while let Some(next) = cursor {
        let page = runtime
            .phase1_today_page(context.clone(), limit, None, Some(next))
            .await
            .expect("continued Today page");
        items.extend(page.items);
        cursor = page.next_cursor;
    }
    let mut unique = items.iter().map(|card| &card.card_id).collect::<Vec<_>>();
    unique.sort_unstable();
    unique.dedup();
    assert_eq!(unique.len(), items.len(), "frozen pages have no duplicates");
    items
}

async fn publish_supporting_events(relay_url: &str, update_id: &str, food_id: &str) {
    let profile_keys = Keys::parse(FIXTURE_SECRET).expect("profile keys");
    let profile_client = Client::new(profile_keys);
    profile_client
        .add_relay(relay_url)
        .await
        .expect("profile relay");
    profile_client.connect().await;
    profile_client
        .wait_for_connection(Duration::from_secs(2))
        .await;
    profile_client
        .send_event_builder(
            EventBuilder::metadata(
                &Metadata::new()
                    .name("moss")
                    .display_name("Moss Farm")
                    .about("Local harvests"),
            )
            .custom_created_at(Timestamp::from_secs(AUTHORED_AT + 1)),
        )
        .await
        .expect("publish profile");
    profile_client.shutdown().await;

    let reply_keys = Keys::parse(REPLY_SECRET).expect("reply keys");
    let reply_author = reply_keys.public_key().to_string();
    let reply_client = Client::new(reply_keys);
    reply_client
        .add_relay(relay_url)
        .await
        .expect("reply relay");
    reply_client.connect().await;
    reply_client
        .wait_for_connection(Duration::from_secs(2))
        .await;
    reply_client
        .send_event_builder(
            EventBuilder::text_note("The farm stand is open")
                .tags([
                    Tag::parse(["e", update_id, relay_url, "root"]).expect("reply root tag"),
                    Tag::parse(["p", support::PUBLIC_KEY]).expect("reply author tag"),
                ])
                .custom_created_at(Timestamp::from_secs(AUTHORED_AT + 2)),
        )
        .await
        .expect("publish reply");
    reply_client
        .send_event_builder(
            EventBuilder::new(Kind::Custom(1_111), "Are these available Saturday?")
                .tags([
                    Tag::parse(["E", food_id, relay_url, support::PUBLIC_KEY])
                        .expect("comment root event tag"),
                    Tag::parse(["K", "30402"]).expect("comment root kind tag"),
                    Tag::parse(["P", support::PUBLIC_KEY, relay_url])
                        .expect("comment root author tag"),
                    Tag::parse(["e", food_id, relay_url, support::PUBLIC_KEY])
                        .expect("comment parent event tag"),
                    Tag::parse(["k", "30402"]).expect("comment parent kind tag"),
                    Tag::parse(["p", support::PUBLIC_KEY, relay_url])
                        .expect("comment parent author tag"),
                    Tag::parse(["p", reply_author.as_str()]).expect("self author context tag"),
                ])
                .custom_created_at(Timestamp::from_secs(AUTHORED_AT + 3)),
        )
        .await
        .expect("publish comment");
    reply_client.shutdown().await;
}

async fn prove_corrupted_media_fails(
    runtime: &RadrootsRuntime,
    bytes: &[u8],
    image_file: &std::fs::File,
) {
    let corrupt = BlossomServer::spawn(bytes.to_vec(), true).await;
    runtime
        .configure_blossom(
            FfiBlossomHostKind::Simulator,
            FfiBlossomEndpointAuthority::LoopbackDevelopment,
            corrupt.origin.clone(),
            vec![],
        )
        .expect("corrupt test Blossom profile");
    let media = prepared_media(
        &corrupt.origin,
        bytes,
        image_file,
        "Corrupt retrieval proof",
    );
    let id = draft_id(8);
    let saved = runtime
        .phase1_save_draft(
            id.clone(),
            add_input(
                FfiAddCommandType::CreatePhotoUpdate,
                "This image must fail closed",
                Some(media.clone()),
            ),
            AUTHORED_AT + 30,
            None,
            1_800_000_030_000,
        )
        .await
        .expect("save corrupt-media draft");
    let error = runtime
        .phase1_upload_draft_media(FfiBlossomUploadInput {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            draft_id: id.clone(),
            expected_revision: saved.revision,
            media,
            authorization_content: "Verify corrupt retrieval rejection".to_owned(),
            authorization_created_at_unix_s: AUTHORED_AT,
            authorization_lifetime_seconds: 300,
            operation_id: "31".repeat(16),
            artifact_id: "32".repeat(16),
            signing_deadline_unix_ms: u64::MAX,
            signing_cancellation: FfiCancellationPolicy::LocalCooperative,
            verified_at_unix_ms: 1_800_000_030_100,
            updated_at_unix_ms: 1_800_000_030_200,
        })
        .await
        .expect_err("corrupt media retrieval must fail");
    assert_eq!(error.report().code, "authoring_failed");
    let failed = runtime
        .phase1_draft_status(id)
        .await
        .expect("durable corrupt-media status");
    assert_eq!(failed.media[0].stage, FfiMediaStage::Failed);
    assert!(failed.media[0].possible_orphan);
    corrupt.finish().await;
}

async fn unused_loopback_port() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("reserve unused port");
    listener.local_addr().expect("unused address").port()
}

async fn read_http_request(stream: &mut tokio::net::TcpStream) -> Vec<u8> {
    let mut request = Vec::new();
    let header_end = loop {
        let mut chunk = [0_u8; 1_024];
        let read = stream.read(&mut chunk).await.expect("read HTTP request");
        assert_ne!(read, 0, "HTTP request ended before headers");
        request.extend_from_slice(&chunk[..read]);
        if let Some(index) = request.windows(4).position(|value| value == b"\r\n\r\n") {
            break index + 4;
        }
        assert!(request.len() < 64 * 1_024, "HTTP headers are bounded");
    };
    let content_length = String::from_utf8_lossy(&request[..header_end])
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .and_then(|value| value.trim().parse::<usize>().ok())
        })
        .unwrap_or_default();
    while request.len() - header_end < content_length {
        let mut chunk = [0_u8; 1_024];
        let read = stream.read(&mut chunk).await.expect("read HTTP body");
        assert_ne!(read, 0, "HTTP request ended before body");
        request.extend_from_slice(&chunk[..read]);
    }
    request
}

async fn write_http_response(stream: &mut tokio::net::TcpStream, content_type: &str, body: &[u8]) {
    let head = format!(
        "HTTP/1.1 200 OK\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream
        .write_all(head.as_bytes())
        .await
        .expect("write response head");
    stream.write_all(body).await.expect("write response body");
    stream.shutdown().await.expect("close HTTP response");
}

fn draft_id(index: u8) -> String {
    format!("{index:02x}").repeat(16)
}

fn png(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes
}

fn unix_time_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock after epoch")
        .as_millis()
        .try_into()
        .expect("current time fits u64")
}
