//! Bounded, independent host subscriptions for focused runtime invalidation signals.

use std::collections::BTreeMap;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{SyncSender, TrySendError, sync_channel};
use std::sync::{Arc, Mutex, Weak};

use crate::{MOBILE_FFI_SCHEMA_VERSION, RadrootsAppError};

const MAX_SUBSCRIPTIONS: usize = 32;
const CHANGE_BUFFER_CAPACITY: usize = 16;

#[derive(Clone, Copy, Debug, Eq, PartialEq, uniffi::Enum)]
pub enum FfiRuntimeChangeKind {
    Initial,
    Identity,
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
pub trait RadrootsRuntimeObserver: Send + Sync {
    fn on_change(&self, change: FfiRuntimeChangeRecord);
}

pub(crate) struct SubscriptionHub {
    next_id: AtomicU64,
    generation: AtomicU64,
    closed: AtomicBool,
    subscriptions: Mutex<BTreeMap<u64, SyncSender<FfiRuntimeChangeRecord>>>,
}

impl SubscriptionHub {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            next_id: AtomicU64::new(1),
            generation: AtomicU64::new(1),
            closed: AtomicBool::new(false),
            subscriptions: Mutex::new(BTreeMap::new()),
        })
    }

    pub(crate) fn subscribe(
        self: &Arc<Self>,
        observer: Box<dyn RadrootsRuntimeObserver>,
    ) -> Result<Arc<FfiSubscriptionHandle>, RadrootsAppError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(subscription_error("runtime_closed", false));
        }
        let id = self.next_id.fetch_add(1, Ordering::AcqRel);
        let (sender, receiver) = sync_channel(CHANGE_BUFFER_CAPACITY);
        let observer: Arc<dyn RadrootsRuntimeObserver> = Arc::from(observer);
        let hub = Arc::downgrade(self);
        std::thread::Builder::new()
            .name(format!("radroots-ffi-observer-{id}"))
            .spawn(move || {
                while let Ok(change) = receiver.recv() {
                    if catch_unwind(AssertUnwindSafe(|| observer.on_change(change))).is_err() {
                        break;
                    }
                }
                if let Some(hub) = hub.upgrade() {
                    hub.remove(id);
                }
            })
            .map_err(|_| subscription_error("subscription_worker_unavailable", true))?;

        {
            let mut subscriptions = self
                .subscriptions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if self.closed.load(Ordering::Acquire) || subscriptions.len() >= MAX_SUBSCRIPTIONS {
                return Err(subscription_error("subscription_limit_reached", true));
            }
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
            self.generation.fetch_add(1, Ordering::AcqRel);
            self.subscriptions
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .clear();
        }
    }

    fn remove(&self, id: u64) {
        self.subscriptions
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&id);
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

fn subscription_error(code: &str, retryable: bool) -> RadrootsAppError {
    RadrootsAppError::failure(
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

    impl RadrootsRuntimeObserver for NoopObserver {
        fn on_change(&self, _change: FfiRuntimeChangeRecord) {}
    }

    struct PanicObserver;

    impl RadrootsRuntimeObserver for PanicObserver {
        fn on_change(&self, _change: FfiRuntimeChangeRecord) {
            panic!("observer panic is isolated");
        }
    }

    struct BlockingObserver(Arc<(Mutex<bool>, Condvar)>);

    impl RadrootsRuntimeObserver for BlockingObserver {
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
