//! Controlled inbound SPI retains original public targets; it performs no I/O.
use super::fixture::*;
use crate::TeraRuntime;
use crate::runtime::{builder::RuntimeBuilder, restore::ApplicationRestoreGuard};
use radroots_event::SignedEvent;
use radroots_transport::SinkStatus;
use radroots_transport::outcome::{FetchTargetOutcome, FetchTargetState};
use radroots_transport::sink::{DeliveryReceipt, DeliveryRequest, SinkFailure};
use radroots_transport::source::{EventProvenance, NextPage, ObservedEvent};
use radroots_transport::{
    BoxFuture, Error, EventSink, EventSource, FetchPage, FetchRequest, SourceStatus, TransportId,
};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

pub(super) struct OriginalTargets {
    pub incomplete: AtomicBool,
    pub requests: Mutex<Vec<String>>,
    pub deliveries: AtomicUsize,
    pub original: SignedEvent,
    pub author: radroots_identity::PublicKey,
    pub authored_at: u64,
}

impl OriginalTargets {
    pub fn new(fixture: &Fixture) -> Self {
        let raw = field(&fixture.expected["signed_event"], "raw_json");
        let wire = radroots_event::wire::Nip01EventWire::parse_json(raw).unwrap();
        Self {
            incomplete: AtomicBool::new(true),
            requests: Mutex::new(vec![]),
            deliveries: AtomicUsize::new(0),
            author: radroots_identity::PublicKey::from_hex(field(&fixture.rust, "public_key"))
                .unwrap(),
            authored_at: serde_json::from_str::<serde_json::Value>(raw).unwrap()["created_at"]
                .as_u64()
                .unwrap(),
            original: SignedEvent::from_wire_verified_id(wire, raw).unwrap(),
        }
    }
}

impl EventSource for OriginalTargets {
    fn status(&self) -> BoxFuture<'_, Result<SourceStatus, Error>> {
        Box::pin(async { unreachable!("Explicit reconciliation only") })
    }
    fn fetch(&self, request: FetchRequest) -> BoxFuture<'_, Result<FetchPage, Error>> {
        Box::pin(async move {
            assert_eq!(request.target_set().len(), 1);
            assert_eq!(request.bounds().limit(), 64);
            let target = &request.target_set().targets()[0];
            assert!(
                ["wss://relay-one.example", "wss://relay-two.example"]
                    .into_iter()
                    .map(|url| radroots_transport::Target::nostr_relay(url).unwrap())
                    .any(|original| &original == target)
            );
            assert_eq!(request.selector().authors(), &[self.author]);
            assert_eq!(request.selector().kinds(), &[1]);
            assert_eq!(
                request.selector().since_unix_seconds(),
                Some(self.authored_at)
            );
            assert_eq!(
                request.selector().until_unix_seconds(),
                Some(self.authored_at)
            );
            self.requests
                .lock()
                .unwrap()
                .push(target.fingerprint().as_str().into());
            let incomplete = self.incomplete.load(Ordering::SeqCst);
            let state = if incomplete {
                FetchTargetState::Partial
            } else {
                FetchTargetState::Complete
            };
            let events = if !incomplete && target.uri().as_str().contains("relay-one") {
                let now = crate::runtime::product_surface::phase1_operation_now_unix_ms().unwrap();
                let provenance =
                    EventProvenance::new(TransportId::NOSTR, target.fingerprint().clone(), now)
                        .unwrap();
                vec![ObservedEvent::new(self.original.clone(), provenance)]
            } else {
                vec![]
            };
            FetchPage::for_request(
                &request,
                events,
                vec![FetchTargetOutcome::new(target.fingerprint().clone(), state)],
                NextPage::Complete,
            )
        })
    }
}

impl EventSink for OriginalTargets {
    fn status(&self) -> BoxFuture<'_, Result<SinkStatus, Error>> {
        Box::pin(async { unreachable!("Reconciliation cannot publish") })
    }
    fn deliver(&self, _: DeliveryRequest) -> BoxFuture<'_, Result<DeliveryReceipt, SinkFailure>> {
        Box::pin(async {
            self.deliveries.fetch_add(1, Ordering::SeqCst);
            panic!("Restore/review/consent must not publish a replacement event")
        })
    }
}

pub(super) async fn controlled_runtime(
    fixture: &Fixture,
    guard: ApplicationRestoreGuard,
    source: Arc<OriginalTargets>,
) -> TeraRuntime {
    let config = fixture.config(true);
    // First execute the actual production guarded constructor and startup
    // checks. Only the inbound/outbound SPI is subsequently controlled; retain
    // the same SQLite owner, guard and lifecycle, with no signer installed.
    let mut runtime = RuntimeBuilder::new(config.clone())
        .restore_guard(guard)
        .build()
        .await
        .unwrap();
    runtime.client.close().await.unwrap();
    runtime.client = radroots_sdk::ClientBuilder::sqlite(config.sqlite_options().unwrap())
        .await
        .unwrap()
        .source(source.clone())
        .sink(source)
        .host_sync(radroots_sdk::sync::HostPolicy::standard())
        .build()
        .unwrap();
    runtime.validate_restore_startup().await.unwrap();
    runtime
}
