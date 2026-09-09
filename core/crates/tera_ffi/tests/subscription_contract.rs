use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use tera_ffi::{FfiRuntimeChangeKind, FfiRuntimeChangeRecord, TeraRuntimeObserver};

mod support;

struct Observer(Sender<FfiRuntimeChangeRecord>);

impl TeraRuntimeObserver for Observer {
    fn on_change(&self, change: FfiRuntimeChangeRecord) {
        let _ = self.0.send(change);
    }
}

fn observer() -> (
    Box<dyn TeraRuntimeObserver>,
    Receiver<FfiRuntimeChangeRecord>,
) {
    let (sender, receiver) = channel();
    (Box::new(Observer(sender)), receiver)
}

fn receive(receiver: &Receiver<FfiRuntimeChangeRecord>) -> FfiRuntimeChangeRecord {
    receiver
        .recv_timeout(Duration::from_secs(1))
        .expect("bounded observer delivery")
}

#[tokio::test]
async fn subscriptions_are_independent_bounded_handles_and_stop_individually() {
    let (_root, runtime) = support::runtime().await;
    let settings = runtime.phase1_settings().await.unwrap();
    let (first_observer, first_receiver) = observer();
    let (second_observer, second_receiver) = observer();
    let first = runtime
        .subscribe_changes(first_observer)
        .expect("first subscription");
    let second = runtime
        .subscribe_changes(second_observer)
        .expect("second subscription");

    let first_initial = receive(&first_receiver);
    let second_initial = receive(&second_receiver);
    assert_eq!(first_initial.kind, FfiRuntimeChangeKind::Initial);
    assert_eq!(second_initial, first_initial);
    assert_eq!(first_initial.scope.public_key, support::PUBLIC_KEY);
    assert_eq!(first_initial.scope.source_generation, support::GENERATION);
    assert_eq!(first_initial.scope.context, None);
    assert_eq!(first_initial.epoch.len(), 32);
    assert_eq!(runtime.phase1_settings().await.unwrap(), settings);
    first.unsubscribe();
    assert!(!first.is_active());
    assert!(second.is_active());

    runtime
        .configure_public_relays(vec!["wss://write.example".to_owned()])
        .expect("relay configuration");
    assert_eq!(receive(&second_receiver).kind, FfiRuntimeChangeKind::Relay);
    assert!(
        first_receiver
            .recv_timeout(Duration::from_millis(50))
            .is_err()
    );

    runtime.shutdown().await.expect("shutdown");
    assert_eq!(
        receive(&second_receiver).kind,
        FfiRuntimeChangeKind::Lifecycle
    );
    assert!(!second.is_active());
}

#[tokio::test]
async fn today_hints_retain_the_exact_query_context_and_domain_revision() {
    use tera_ffi::{FfiInvalidationRevision, FfiLocalNetworkRecord, FfiTodayProjectionUpdate};
    let (root, runtime) = support::runtime().await;
    let (first_observer, receiver) = observer();
    let handle = runtime.subscribe_changes(first_observer).unwrap();
    let initial = receive(&receiver);
    for (index, relay) in ["wss://first.example", "wss://second.example"]
        .into_iter()
        .enumerate()
    {
        let context = FfiLocalNetworkRecord {
            schema_version: 1,
            id: "default".into(),
            label: "Local network".into(),
            relay_urls: vec![relay.into()],
            locality: None,
            followed_authors: vec![],
            generation: 1,
        };
        runtime
            .phase1_refresh_today(
                context.clone(),
                1_800_000_000,
                FfiTodayProjectionUpdate::Incremental,
            )
            .await
            .unwrap();
        let hint = receive(&receiver);
        assert_eq!(hint.kind, FfiRuntimeChangeKind::Today);
        assert_eq!(hint.epoch, initial.epoch);
        assert_eq!(hint.scope.public_key, support::PUBLIC_KEY);
        assert_eq!(hint.scope.source_generation, support::GENERATION);
        assert_eq!(hint.scope.context, Some(context));
        assert_eq!(
            hint.revision,
            FfiInvalidationRevision::Current {
                value: index as u64 + 1
            }
        );
    }
    handle.unsubscribe();
    let settings = runtime.phase1_settings().await.unwrap();
    runtime.shutdown().await.unwrap();
    let reopened = tera_ffi::TeraRuntime::new(
        root.path().to_string_lossy().into_owned(),
        support::PUBLIC_KEY.into(),
        support::GENERATION.into(),
        1_800_000_000_000,
        tera_ffi::ProtectedDataAvailability::Available,
    )
    .await
    .unwrap();
    assert_eq!(reopened.phase1_settings().await.unwrap(), settings);
    let (next_observer, next_receiver) = observer();
    let next = reopened.subscribe_changes(next_observer).unwrap();
    let next_initial = receive(&next_receiver);
    assert_eq!(next_initial.scope, initial.scope);
    assert_ne!(next_initial.epoch, initial.epoch);
    next.unsubscribe();
    reopened.shutdown().await.unwrap();
}

struct PausedObserver {
    sender: Sender<FfiRuntimeChangeRecord>,
    release: std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}

impl TeraRuntimeObserver for PausedObserver {
    fn on_change(&self, change: FfiRuntimeChangeRecord) {
        let initial = change.kind == FfiRuntimeChangeKind::Initial
            && change.delivery == tera_ffi::FfiRuntimeChangeDelivery::Change;
        let _ = self.sender.send(change);
        if initial {
            let (released, wake) = &*self.release;
            drop(
                wake.wait_while(released.lock().unwrap(), |value| !*value)
                    .unwrap(),
            );
        }
    }
}

struct ReleaseOnDrop(std::sync::Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>);

impl Drop for ReleaseOnDrop {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

#[tokio::test]
async fn slow_observer_receives_final_gap_and_can_query_all_committed_settings_without_more_events()
{
    use tera_ffi::{FfiReplaceSettingsRecord, FfiRuntimeChangeDelivery};
    let (_root, runtime) = support::runtime().await;
    let (sender, receiver) = channel();
    let release = std::sync::Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let release_on_drop = ReleaseOnDrop(std::sync::Arc::clone(&release));
    let handle = runtime
        .subscribe_changes(Box::new(PausedObserver { sender, release }))
        .unwrap();
    let initial = receive(&receiver);
    let mut settings = runtime.phase1_settings().await.unwrap();
    let before = settings.revision;
    // The production FFI queue admits sixteen pending records; the next commit
    // must leave a gap even when this caller performs no subsequent mutation.
    for _ in 0..17 {
        let mut media_network = settings.media_network;
        media_network.allow_cellular_downloads = !media_network.allow_cellular_downloads;
        settings = runtime
            .phase1_replace_settings(FfiReplaceSettingsRecord {
                schema_version: settings.schema_version,
                expected_revision: settings.revision,
                relays: settings.relays,
                blossom: settings.blossom,
                media_network,
                local_storage: settings.local_storage,
            })
            .await
            .unwrap()
            .settings;
    }
    assert_eq!(settings.revision, before + 17);
    drop(release_on_drop);
    let gap = receive(&receiver);
    assert_eq!(gap.delivery, FfiRuntimeChangeDelivery::ResnapshotRequired);
    assert_eq!(gap.scope, initial.scope);
    assert_eq!(gap.epoch, initial.epoch);
    assert_eq!(gap.kind, FfiRuntimeChangeKind::Initial);
    assert_eq!(gap.entity_id, None);
    assert_eq!(runtime.phase1_settings().await.unwrap(), settings);
    handle.unsubscribe();
    runtime.shutdown().await.unwrap();
}
