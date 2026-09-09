use std::{sync::mpsc, time::Duration};

use tera_ffi::{
    FfiInvalidationRevision, FfiLocalNetworkRecord, FfiRuntimeChangeKind, FfiRuntimeChangeRecord,
    FfiTodayProjectionUpdate, TeraRuntimeObserver,
};

mod support;

struct Observer(mpsc::Sender<FfiRuntimeChangeRecord>);

impl TeraRuntimeObserver for Observer {
    fn on_change(&self, change: FfiRuntimeChangeRecord) {
        let _ = self.0.send(change);
    }
}

fn context(generation: u64) -> FfiLocalNetworkRecord {
    FfiLocalNetworkRecord {
        schema_version: 1,
        id: "nearby".into(),
        label: "Nearby".into(),
        relay_urls: vec!["wss://relay.example".into()],
        locality: None,
        followed_authors: vec![],
        generation,
    }
}

#[tokio::test]
async fn actual_context_callbacks_preserve_unsigned_boundaries_and_runtime_scope() {
    let (_root, runtime) = support::runtime().await;
    let (sender, receiver) = mpsc::channel();
    let handle = runtime
        .subscribe_changes(Box::new(Observer(sender)))
        .unwrap();
    let initial = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(initial.scope.public_key, support::PUBLIC_KEY);
    assert_eq!(initial.scope.source_generation, support::GENERATION);
    for (index, generation) in [0, 1, (i64::MAX as u64) + 1, u64::MAX]
        .into_iter()
        .enumerate()
    {
        let context = context(generation);
        runtime
            .phase1_refresh_today(
                context.clone(),
                1_800_000_000,
                FfiTodayProjectionUpdate::Incremental,
            )
            .await
            .unwrap();
        let changed = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(changed.scope.context, Some(context));
        assert_eq!(changed.epoch, initial.epoch);
        assert_eq!(
            changed.revision,
            FfiInvalidationRevision::Current {
                value: index as u64 + 1
            }
        );
    }
    handle.unsubscribe();
    assert_eq!(runtime.shutdown().await.unwrap().state, "closed");
}

#[tokio::test]
async fn invalid_contexts_fail_before_emitting_a_callback() {
    let (_root, runtime) = support::runtime().await;
    let (sender, receiver) = mpsc::channel();
    let handle = runtime
        .subscribe_changes(Box::new(Observer(sender)))
        .unwrap();
    receiver.recv_timeout(Duration::from_secs(5)).unwrap();
    let mut unsupported = context(1);
    unsupported.schema_version = u16::MAX;
    let mut invalid = context(1);
    invalid.id.clear();
    for (context, expected) in [
        (invalid, "invalid_local_network"),
        (unsupported, "unsupported_schema_version"),
    ] {
        let error = runtime
            .phase1_refresh_today(
                context,
                1_800_000_000,
                FfiTodayProjectionUpdate::Incremental,
            )
            .await
            .unwrap_err();
        assert_eq!(error.report().code, expected);
    }
    runtime
        .configure_public_relays(vec!["wss://write.example".into()])
        .unwrap();
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(5)).unwrap().kind,
        FfiRuntimeChangeKind::Relay
    );
    assert!(receiver.try_recv().is_err());
    handle.unsubscribe();
    runtime.shutdown().await.unwrap();
}
