use std::{sync::Arc, time::Duration};

use tera_ffi::TeraAppError;

mod support;

#[tokio::test]
async fn host_release_ordering_retains_close_and_finishes_within_deadline() {
    let (_root, runtime) = support::runtime().await;
    let host = Arc::new(runtime);
    let closing_owner = Arc::clone(&host);
    let close = tokio::spawn(async move { closing_owner.shutdown().await });
    drop(host);

    let result = tokio::time::timeout(Duration::from_secs(1), close)
        .await
        .expect("mobile shutdown exceeded its host deadline")
        .expect("shutdown task panicked")
        .expect("shutdown failed");
    assert_eq!(result.state, "closed");
    assert!(!result.already_closed);
}

#[tokio::test]
async fn concurrent_host_references_converge_and_repeated_close_is_idempotent() {
    let (_root, runtime) = support::runtime().await;
    let runtime = Arc::new(runtime);
    let first = Arc::clone(&runtime);
    let second = Arc::clone(&runtime);
    let (first, second) = tokio::join!(first.shutdown(), second.shutdown());

    for outcome in [&first, &second] {
        assert!(
            outcome.is_ok()
                || matches!(
                    outcome,
                    Err(TeraAppError::Failure { report })
                        if report.code == "client_close_in_progress"
                )
        );
    }
    let repeated = runtime.shutdown().await.expect("repeated close");
    assert!(repeated.already_closed);
}

struct CallbackGate {
    entered: Arc<tokio::sync::Notify>,
    released: Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>,
}

impl tera_ffi::TeraRuntimeObserver for CallbackGate {
    fn on_change(&self, _: tera_ffi::FfiRuntimeChangeRecord) {
        self.entered.notify_one();
        let (released, wake) = &*self.released;
        drop(
            wake.wait_while(released.lock().unwrap(), |released| !*released)
                .unwrap(),
        );
    }
}

struct ReleaseCallback(Arc<(std::sync::Mutex<bool>, std::sync::Condvar)>);

impl Drop for ReleaseCallback {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

#[tokio::test]
async fn ffi_close_retains_a_slow_native_callback_across_cancelled_and_repeated_waits() {
    use std::{
        future::{Future, poll_fn},
        task::Poll,
    };
    let (_root, runtime) = support::runtime().await;
    let entered = Arc::new(tokio::sync::Notify::new());
    let released = Arc::new((std::sync::Mutex::new(false), std::sync::Condvar::new()));
    let release = ReleaseCallback(Arc::clone(&released));
    let observer = runtime
        .subscribe_changes(Box::new(CallbackGate {
            entered: Arc::clone(&entered),
            released,
        }))
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), entered.notified())
        .await
        .unwrap();
    let mut close = Box::pin(runtime.shutdown());
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            // Poll the actual exported close until storage has settled. Native
            // callback ownership must still keep its result pending.
            let outcome = poll_fn(|context| Poll::Ready(close.as_mut().poll(context))).await;
            assert!(
                outcome.is_pending(),
                "FFI close returned before its native callback"
            );
            if runtime.info().sdk_closed {
                break;
            }
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(!observer.is_active());
    assert_eq!(
        runtime
            .sdk_storage_status()
            .await
            .unwrap_err()
            .report()
            .code,
        "client_closed"
    );
    drop(close);
    let mut repeat = Box::pin(runtime.shutdown());
    let outcome = poll_fn(|context| Poll::Ready(repeat.as_mut().poll(context))).await;
    assert!(
        outcome.is_pending(),
        "Cancelled close must retain its callback drain"
    );
    drop(release);
    let receipt = tokio::time::timeout(Duration::from_secs(5), repeat)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.state, "closed");
    assert!(receipt.already_closed);
    assert!(runtime.shutdown().await.unwrap().already_closed);
}
