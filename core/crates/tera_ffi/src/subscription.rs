//! Bounded, independent host subscriptions for focused runtime invalidation signals.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, Weak};

use crate::{MOBILE_FFI_SCHEMA_VERSION, TeraAppError};

const MAX_SUBSCRIPTIONS: usize = 32;
const CHANGE_BUFFER_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRuntimeChangeKind {
    Initial,
    Identity,
    Settings,
    Profile,
    Today,
    Drafts,
    Relay,
    Media,
    Lifecycle,
}

#[derive(Clone, Debug, Eq, PartialEq, uniffi::Record)]
pub struct FfiRuntimeChangeRecord {
    pub schema_version: u16,
    pub generation: u64,
    pub kind: FfiRuntimeChangeKind,
    pub entity_id: Option<String>,
}

#[uniffi::export(callback_interface)]
pub trait TeraRuntimeObserver: Send + Sync {
    fn on_change(&self, change: FfiRuntimeChangeRecord);
}

pub(crate) struct SubscriptionHub {
    next_id: AtomicU64,
    generation: AtomicU64,
    closed: AtomicBool,
    workers: Arc<WorkerState>,
    subscriptions: Mutex<BTreeMap<u64, SyncSender<FfiRuntimeChangeRecord>>>,
}

impl SubscriptionHub {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            next_id: AtomicU64::new(1),
            generation: AtomicU64::new(1),
            closed: AtomicBool::new(false),
            workers: Arc::new(WorkerState::default()),
            subscriptions: Mutex::new(BTreeMap::new()),
        })
    }

    pub(crate) fn subscribe(
        self: &Arc<Self>,
        observer: Box<dyn TeraRuntimeObserver>,
    ) -> Result<Arc<FfiSubscriptionHandle>, TeraAppError> {
        let id = self.next_id.fetch_add(1, Ordering::AcqRel);
        let (sender, receiver) = sync_channel::<FfiRuntimeChangeRecord>(CHANGE_BUFFER_CAPACITY);
        {
            let mut subscriptions = self
                .subscriptions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if self.closed.load(Ordering::Acquire) {
                return Err(subscription_error("runtime_closed", false));
            }
            if subscriptions.len() >= MAX_SUBSCRIPTIONS {
                return Err(subscription_error("subscription_limit_reached", true));
            }
            self.workers.active.fetch_add(1, Ordering::AcqRel);
            let worker = WorkerLease(Arc::clone(&self.workers));
            let hub = Arc::downgrade(self);
            std::thread::Builder::new()
                .name(format!("tera-ffi-observer-{id}"))
                .spawn(move || {
                    let _worker = worker;
                    while let Ok(change) = receiver.recv() {
                        let Some(hub) = hub.upgrade() else {
                            break;
                        };
                        let closed = hub.closed.load(Ordering::Acquire);
                        drop(hub);
                        if closed && change.kind != FfiRuntimeChangeKind::Lifecycle {
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
            subscriptions.insert(id, sender.clone());
        }

        let initial = FfiRuntimeChangeRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            generation: self.generation.load(Ordering::Acquire),
            kind: FfiRuntimeChangeKind::Initial,
            entity_id: None,
        };
        let _ = sender.try_send(initial);
        Ok(Arc::new(FfiSubscriptionHandle {
            hub: Arc::downgrade(self),
            id: Mutex::new(Some(id)),
        }))
    }

    pub(crate) fn notify(&self, kind: FfiRuntimeChangeKind, entity_id: Option<String>) {
        if self.closed.load(Ordering::Acquire) {
            return;
        }
        let change = FfiRuntimeChangeRecord {
            schema_version: MOBILE_FFI_SCHEMA_VERSION,
            generation: self.generation.fetch_add(1, Ordering::AcqRel) + 1,
            kind,
            entity_id,
        };
        let senders = self
            .subscriptions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .iter()
            .map(|(id, sender)| (*id, sender.clone()))
            .collect::<Vec<_>>();
        let mut disconnected = Vec::new();
        for (id, sender) in senders {
            match sender.try_send(change.clone()) {
                Ok(()) | Err(TrySendError::Full(_)) => {}
                Err(TrySendError::Disconnected(_)) => disconnected.push(id),
            }
        }
        if !disconnected.is_empty() {
            let mut subscriptions = self
                .subscriptions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for id in disconnected {
                subscriptions.remove(&id);
            }
        }
    }

    pub(crate) fn close(&self) {
        if !self.closed.swap(true, Ordering::AcqRel) {
            let change = FfiRuntimeChangeRecord {
                schema_version: MOBILE_FFI_SCHEMA_VERSION,
                generation: self.generation.fetch_add(1, Ordering::AcqRel) + 1,
                kind: FfiRuntimeChangeKind::Lifecycle,
                entity_id: None,
            };
            let mut subscriptions = self
                .subscriptions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            for sender in subscriptions.values() {
                let _ = sender.try_send(change.clone());
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
        self.subscriptions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
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

    struct NoopObserver;

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
        let hub = SubscriptionHub::new();
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

    #[test]
    fn closed_limit_and_detached_handle_paths_are_typed_and_idempotent() {
        let closed = SubscriptionHub::new();
        closed.close();
        closed.close();
        closed.notify(FfiRuntimeChangeKind::Today, None);
        let error = closed
            .subscribe(Box::new(NoopObserver))
            .err()
            .expect("closed hub");
        assert_eq!(error.report().code, "runtime_closed");
        assert!(!error.report().retryable);

        let hub = SubscriptionHub::new();
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

        let detached_hub = SubscriptionHub::new();
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
        let hub = SubscriptionHub::new();
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
