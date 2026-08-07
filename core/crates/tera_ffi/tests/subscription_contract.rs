use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

use radroots_mobile_ffi::{FfiRuntimeChangeKind, FfiRuntimeChangeRecord, RadrootsRuntimeObserver};

mod support;

struct Observer(Sender<FfiRuntimeChangeRecord>);

impl RadrootsRuntimeObserver for Observer {
    fn on_change(&self, change: FfiRuntimeChangeRecord) {
        let _ = self.0.send(change);
    }
}

fn observer() -> (
    Box<dyn RadrootsRuntimeObserver>,
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
    let (first_observer, first_receiver) = observer();
    let (second_observer, second_receiver) = observer();
    let first = runtime
        .subscribe_changes(first_observer)
        .expect("first subscription");
    let second = runtime
        .subscribe_changes(second_observer)
        .expect("second subscription");

    assert_eq!(receive(&first_receiver).kind, FfiRuntimeChangeKind::Initial);
    assert_eq!(
        receive(&second_receiver).kind,
        FfiRuntimeChangeKind::Initial
    );
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
