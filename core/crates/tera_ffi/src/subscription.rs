//! Bounded, independent host subscriptions for focused runtime invalidation signals.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Weak};

use tera_core::runtime::invalidation::{InvalidationDomain, RuntimeInvalidations};
use tera_core::runtime::product_surface::LocalNetwork;

use crate::subscription_queue::SubscriptionQueue;
use crate::{FfiRuntimeChangeDelivery, FfiRuntimeChangeKind, FfiRuntimeChangeRecord, TeraAppError};

const MAX_SUBSCRIPTIONS: usize = 32;

#[uniffi::export(callback_interface)]
pub trait TeraRuntimeObserver: Send + Sync {
    fn on_change(&self, change: FfiRuntimeChangeRecord);
}

pub(crate) struct SubscriptionHub {
    next_id: AtomicU64,
    source: RuntimeInvalidations,
    closed: AtomicBool,
    workers: Arc<WorkerState>,
    subscriptions: Mutex<BTreeMap<u64, Arc<SubscriptionQueue>>>,
}

impl SubscriptionHub {
    pub(crate) fn new(source: RuntimeInvalidations) -> Arc<Self> {
        Arc::new(Self {
            next_id: AtomicU64::new(1),
            source,
            closed: AtomicBool::new(false),
            workers: Arc::new(WorkerState::default()),
            subscriptions: Mutex::new(BTreeMap::new()),
        })
    }

    pub(crate) fn subscribe(
        self: &Arc<Self>,
        observer: Box<dyn TeraRuntimeObserver>,
    ) -> Result<Arc<FfiSubscriptionHandle>, TeraAppError> {
        let id = self
            .next_id
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |id| id.checked_add(1))
            .map_err(|_| subscription_error("subscription_limit_reached", false))?;
        {
            let mut subscriptions = self
                .subscriptions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if self.closed.load(Ordering::Acquire) {
                return Err(subscription_error("runtime_closed", false));
            }
            if subscriptions.len() >= MAX_SUBSCRIPTIONS
                || self.workers.active.load(Ordering::Acquire) >= MAX_SUBSCRIPTIONS
            {
                return Err(subscription_error("subscription_limit_reached", true));
            }
            // Enqueue the initial snapshot before exposing this queue to any
            // publisher. An observer always sees the epoch before later hints.
            let queue = SubscriptionQueue::new(
                self.source
                    .snapshot(InvalidationDomain::Initial, None)
                    .into(),
            );
            let receiver = Arc::clone(&queue);
            self.workers.active.fetch_add(1, Ordering::AcqRel);
            let worker = WorkerLease(Arc::clone(&self.workers));
            let hub = Arc::downgrade(self);
            std::thread::Builder::new()
                .name(format!("tera-ffi-observer-{id}"))
                .spawn(move || {
                    let _worker = worker;
                    while let Some(change) = receiver.receive() {
                        let Some(hub) = hub.upgrade() else {
                            break;
                        };
                        let closed = hub.closed.load(Ordering::Acquire);
                        drop(hub);
                        if closed
                            && change.kind != FfiRuntimeChangeKind::Lifecycle
                            && change.delivery != FfiRuntimeChangeDelivery::ResnapshotRequired
                        {
                            continue;
                        }
                        if catch_unwind(AssertUnwindSafe(|| observer.on_change(change))).is_err() {
                            break;
                        }
                    }
                    if let Some(hub) = hub.upgrade() {
                        hub.remove(id);
                    }
                })
                .map_err(|_| subscription_error("subscription_worker_unavailable", true))?;
            subscriptions.insert(id, queue);
        }

        Ok(Arc::new(FfiSubscriptionHandle {
            hub: Arc::downgrade(self),
            id: Mutex::new(Some(id)),
        }))
    }

    pub(crate) fn notify(&self, kind: FfiRuntimeChangeKind, entity_id: Option<String>) {
        self.notify_context(kind, None, entity_id);
    }

    pub(crate) fn notify_context(
        &self,
        kind: FfiRuntimeChangeKind,
        context: Option<&LocalNetwork>,
        entity_id: Option<String>,
    ) {
        // Serialize revision assignment with nonblocking enqueue so concurrent
        // publishers cannot deliver an older domain revision after a newer one.
        let mut subscriptions = self
            .subscriptions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let change: FfiRuntimeChangeRecord =
            self.source.advance(kind.into(), context, entity_id).into();
        subscriptions.retain(|_, queue| queue.send(change.clone()));
    }

    pub(crate) fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            let mut subscriptions = self
                .subscriptions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            let change: FfiRuntimeChangeRecord = self
                .source
                .advance(InvalidationDomain::Lifecycle, None, None)
                .into();
            for queue in subscriptions.values() {
                queue.close(change.clone());
            }
            subscriptions.clear();
        }
    }

    pub(crate) async fn drain(&self) {
        loop {
            let notified = self.workers.drained.notified();
            let mut notified = std::pin::pin!(notified);
            notified.as_mut().enable();
            if self.workers.active.load(Ordering::Acquire) == 0 {
                return;
            }
            notified.await;
        }
    }

    fn remove(&self, id: u64) {
        let removed = self
            .subscriptions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
        if let Some(queue) = removed {
            queue.cancel();
        }
    }
}

impl Drop for SubscriptionHub {
    fn drop(&mut self) {
        self.close();
    }
}

// Workers retain only their settlement counter, avoiding a hub/sender cycle.
// Close rejects new observers and drains callbacks without blocking an executor.
#[derive(Default)]
struct WorkerState {
    active: AtomicUsize,
    drained: tokio::sync::Notify,
}

struct WorkerLease(Arc<WorkerState>);

impl Drop for WorkerLease {
    fn drop(&mut self) {
        if self.0.active.fetch_sub(1, Ordering::AcqRel) == 1 {
            self.0.drained.notify_waiters();
        }
    }
}

#[derive(uniffi::Object)]
pub struct FfiSubscriptionHandle {
    hub: Weak<SubscriptionHub>,
    id: Mutex<Option<u64>>,
}

#[uniffi::export]
impl FfiSubscriptionHandle {
    pub fn unsubscribe(&self) {
        let id = self
            .id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let (Some(hub), Some(id)) = (self.hub.upgrade(), id) {
            hub.remove(id);
        }
    }

    pub fn is_active(&self) -> bool {
        let id = *self
            .id
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (Some(hub), Some(id)) = (self.hub.upgrade(), id) else {
            return false;
        };
        !hub.closed.load(Ordering::Acquire)
            && hub
                .subscriptions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .contains_key(&id)
    }
}

impl Drop for FfiSubscriptionHandle {
    fn drop(&mut self) {
        let id = self
            .id
            .get_mut()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let (Some(hub), Some(id)) = (self.hub.upgrade(), id) {
            hub.remove(id);
        }
    }
}

fn subscription_error(code: &str, retryable: bool) -> TeraAppError {
    TeraAppError::failure(
        code,
        "subscription",
        retryable,
        if retryable { &["retry"] } else { &[] },
        "The runtime change subscription is unavailable.",
    )
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Condvar};
    use std::time::{Duration, Instant};

    use super::*;
    use crate::subscription_queue::CHANGE_BUFFER_CAPACITY;

    fn test_hub() -> Arc<SubscriptionHub> {
        let store = tera_core::runtime::store::MobileUserStoreConfig::from_encoded(
            "/tmp/tera-invalidation-fixture",
            "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798",
            &"04".repeat(32),
            1,
            tera_core::runtime::store::ProtectedDataAvailability::Available,
        )
        .unwrap();
        SubscriptionHub::new(RuntimeInvalidations::new(
            store.public_key(),
            store.source_generation(),
            std::num::NonZeroU128::new(1).unwrap(),
        ))
    }

    struct NoopObserver;

    struct RecordingObserver(std::sync::mpsc::Sender<FfiRuntimeChangeRecord>);

    impl TeraRuntimeObserver for RecordingObserver {
        fn on_change(&self, change: FfiRuntimeChangeRecord) {
            self.0.send(change).unwrap();
        }
    }

    #[test]
    fn concurrent_publication_is_ordered_and_subscription_does_not_advance_domains() {
        let hub = test_hub();
        let (sender, receiver) = std::sync::mpsc::channel();
        let first = hub.subscribe(Box::new(RecordingObserver(sender))).unwrap();
        let initial = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(initial.kind, FfiRuntimeChangeKind::Initial);
        assert_eq!(initial.schema_version, crate::RUNTIME_CHANGE_SCHEMA_VERSION);
        let publishers = (0..8)
            .map(|index| {
                let hub = Arc::clone(&hub);
                std::thread::spawn(move || {
                    hub.notify(FfiRuntimeChangeKind::Drafts, Some(index.to_string()))
                })
            })
            .collect::<Vec<_>>();
        for publisher in publishers {
            publisher.join().unwrap();
        }
        for expected in 1..=8 {
            let change = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(change.epoch, initial.epoch);
            assert_eq!(change.scope, initial.scope);
            assert_eq!(
                change.revision,
                crate::FfiInvalidationRevision::Current { value: expected }
            );
        }
        first.unsubscribe();
        let before = hub.source.snapshot(InvalidationDomain::Drafts, None);
        let (sender, receiver) = std::sync::mpsc::channel();
        let second = hub.subscribe(Box::new(RecordingObserver(sender))).unwrap();
        let resumed = receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(resumed.epoch, initial.epoch);
        assert_eq!(
            before,
            hub.source.snapshot(InvalidationDomain::Drafts, None)
        );
        second.unsubscribe();
    }

    #[test]
    fn subscription_identity_exhaustion_has_no_worker_or_revision_side_effect() {
        let hub = test_hub();
        hub.next_id.store(u64::MAX, Ordering::Release);
        let before = hub.source.snapshot(InvalidationDomain::Initial, None);
        let failure = hub.subscribe(Box::new(NoopObserver)).err().unwrap();
        assert_eq!(failure.report().code, "subscription_limit_reached");
        assert!(!failure.report().retryable);
        assert_eq!(hub.next_id.load(Ordering::Acquire), u64::MAX);
        assert_eq!(hub.workers.active.load(Ordering::Acquire), 0);
        assert_eq!(
            before,
            hub.source.snapshot(InvalidationDomain::Initial, None)
        );
    }

    impl TeraRuntimeObserver for NoopObserver {
        fn on_change(&self, _change: FfiRuntimeChangeRecord) {}
    }

    struct PanicObserver;

    impl TeraRuntimeObserver for PanicObserver {
        fn on_change(&self, _change: FfiRuntimeChangeRecord) {
            panic!("observer panic is isolated");
        }
    }

    struct BlockingObserver(Arc<(Mutex<bool>, Condvar)>);

    impl TeraRuntimeObserver for BlockingObserver {
        fn on_change(&self, change: FfiRuntimeChangeRecord) {
            if change.kind == FfiRuntimeChangeKind::Initial {
                let (released, wake) = &*self.0;
                let guard = released
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                let _guard = wake
                    .wait_while(guard, |released| !*released)
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
            }
        }
    }

    struct PausedObserver {
        entered: Arc<tokio::sync::Notify>,
        gate: Arc<(Mutex<bool>, Condvar)>,
        calls: Arc<AtomicUsize>,
    }

    impl TeraRuntimeObserver for PausedObserver {
        fn on_change(&self, _: FfiRuntimeChangeRecord) {
            self.calls.fetch_add(1, Ordering::AcqRel);
            self.entered.notify_one();
            let (released, wake) = &*self.gate;
            drop(
                wake.wait_while(released.lock().unwrap(), |released| !*released)
                    .unwrap(),
            );
        }
    }

    #[tokio::test]
    async fn close_drains_native_callbacks_even_after_the_wait_is_cancelled() {
        use std::{
            future::Future,
            task::{Context, Poll, Waker},
        };
        let hub = test_hub();
        let entered = Arc::new(tokio::sync::Notify::new());
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let handle = hub
            .subscribe(Box::new(PausedObserver {
                entered: Arc::clone(&entered),
                gate: Arc::clone(&gate),
                calls: Arc::clone(&calls),
            }))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .unwrap();
        hub.notify(FfiRuntimeChangeKind::Today, None);
        hub.close();
        let mut drain = Box::pin(hub.drain());
        assert!(matches!(
            drain.as_mut().poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        assert!(!handle.is_active());
        assert_eq!(
            hub.subscribe(Box::new(NoopObserver))
                .err()
                .unwrap()
                .report()
                .code,
            "runtime_closed"
        );
        drop(drain);
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        tokio::time::timeout(Duration::from_secs(5), hub.drain())
            .await
            .unwrap();
        hub.close();
        hub.drain().await;
        assert_eq!(calls.load(Ordering::Acquire), 2);
        assert_eq!(hub.workers.active.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn unsubscribe_discards_queued_callbacks_and_drains_the_admitted_callback() {
        let hub = test_hub();
        let entered = Arc::new(tokio::sync::Notify::new());
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let calls = Arc::new(AtomicUsize::new(0));
        let handle = hub
            .subscribe(Box::new(PausedObserver {
                entered: Arc::clone(&entered),
                gate: Arc::clone(&gate),
                calls: Arc::clone(&calls),
            }))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .unwrap();
        for _ in 0..=CHANGE_BUFFER_CAPACITY {
            hub.notify(FfiRuntimeChangeKind::Today, None);
        }
        handle.unsubscribe();
        assert!(!handle.is_active());
        assert_eq!(hub.workers.active.load(Ordering::Acquire), 1);
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        tokio::time::timeout(Duration::from_secs(5), hub.drain())
            .await
            .unwrap();
        assert_eq!(calls.load(Ordering::Acquire), 1);
        assert_eq!(hub.workers.active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn dropping_the_hub_releases_an_idle_observer_worker() {
        let hub = test_hub();
        let workers = Arc::clone(&hub.workers);
        let (sender, receiver) = std::sync::mpsc::channel();
        let handle = hub.subscribe(Box::new(RecordingObserver(sender))).unwrap();
        receiver.recv_timeout(Duration::from_secs(5)).unwrap();
        drop(hub);
        assert!(!handle.is_active());
        let deadline = Instant::now() + Duration::from_secs(5);
        while workers.active.load(Ordering::Acquire) != 0 && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(workers.active.load(Ordering::Acquire), 0);
    }

    #[tokio::test]
    async fn cancelled_callback_keeps_worker_admission_until_it_actually_returns() {
        let hub = test_hub();
        let entered = Arc::new(tokio::sync::Notify::new());
        let gate = Arc::new((Mutex::new(false), Condvar::new()));
        let paused = hub
            .subscribe(Box::new(PausedObserver {
                entered: Arc::clone(&entered),
                gate: Arc::clone(&gate),
                calls: Arc::new(AtomicUsize::new(0)),
            }))
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), entered.notified())
            .await
            .unwrap();
        let others = (1..MAX_SUBSCRIPTIONS)
            .map(|_| hub.subscribe(Box::new(NoopObserver)).unwrap())
            .collect::<Vec<_>>();
        paused.unsubscribe();
        let attempted = hub.subscribe(Box::new(NoopObserver));
        let active = hub.workers.active.load(Ordering::Acquire);
        // Release even if the assertion rejects an over-admitted mutant.
        *gate.0.lock().unwrap() = true;
        gate.1.notify_all();
        assert_eq!(active, MAX_SUBSCRIPTIONS);
        let error = attempted
            .err()
            .expect("cancelled callback still owns its worker slot");
        assert_eq!(error.report().code, "subscription_limit_reached");
        assert!(error.report().retryable);
        tokio::time::timeout(Duration::from_secs(5), async {
            while hub.workers.active.load(Ordering::Acquire) == MAX_SUBSCRIPTIONS {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let resumed = hub.subscribe(Box::new(NoopObserver)).unwrap();
        drop(resumed);
        drop(others);
        hub.close();
        tokio::time::timeout(Duration::from_secs(5), hub.drain())
            .await
            .unwrap();
        assert_eq!(hub.workers.active.load(Ordering::Acquire), 0);
    }

    #[test]
    fn closed_limit_and_detached_handle_paths_are_typed_and_idempotent() {
        let closed = test_hub();
        closed.close();
        closed.close();
        closed.notify(FfiRuntimeChangeKind::Today, None);
        let error = closed
            .subscribe(Box::new(NoopObserver))
            .err()
            .expect("closed hub");
        assert_eq!(error.report().code, "runtime_closed");
        assert!(!error.report().retryable);

        let hub = test_hub();
        let handles = (0..MAX_SUBSCRIPTIONS)
            .map(|_| hub.subscribe(Box::new(NoopObserver)).expect("subscription"))
            .collect::<Vec<_>>();
        let error = hub
            .subscribe(Box::new(NoopObserver))
            .err()
            .expect("bounded subscription limit");
        assert_eq!(error.report().code, "subscription_limit_reached");
        assert!(error.report().retryable);
        drop(handles);

        let detached_hub = test_hub();
        let detached = detached_hub
            .subscribe(Box::new(NoopObserver))
            .expect("detached subscription");
        drop(detached_hub);
        assert!(!detached.is_active());
        detached.unsubscribe();
        detached.unsubscribe();
    }

    #[test]
    fn callback_panics_and_full_buffers_never_escape_or_block_publishers() {
        let hub = test_hub();
        let panicking = hub
            .subscribe(Box::new(PanicObserver))
            .expect("panicking subscription");
        let deadline = Instant::now() + Duration::from_secs(1);
        while panicking.is_active() && Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert!(!panicking.is_active());

        let release = Arc::new((Mutex::new(false), Condvar::new()));
        let blocked = hub
            .subscribe(Box::new(BlockingObserver(Arc::clone(&release))))
            .expect("blocked subscription");
        for generation in 0..=CHANGE_BUFFER_CAPACITY {
            hub.notify(
                FfiRuntimeChangeKind::Drafts,
                Some(format!("draft-{generation}")),
            );
        }
        assert!(blocked.is_active());
        let (released, wake) = &*release;
        *released
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = true;
        wake.notify_all();
        hub.close();
        assert!(!blocked.is_active());
    }
}
