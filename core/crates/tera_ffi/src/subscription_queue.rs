//! One bounded observer queue with an atomic out-of-band resnapshot signal.
//!
//! Queue contents, loss and closure share the same lock and wait predicate.
//! A producer cannot strand a final gap between the consumer's empty check and
//! sleep. No callback or asynchronous work runs while the lock is held.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex};

use crate::{FfiRuntimeChangeKind, FfiRuntimeChangeRecord};

pub(crate) const CHANGE_BUFFER_CAPACITY: usize = 16;

pub(crate) struct SubscriptionQueue {
    state: Mutex<State>,
    ready: Condvar,
    gap: FfiRuntimeChangeRecord,
}

struct State {
    changes: VecDeque<FfiRuntimeChangeRecord>,
    dirty: bool,
    closed: bool,
    cancelled: bool,
    #[cfg(test)]
    waiting: bool,
}

impl SubscriptionQueue {
    pub(crate) fn new(initial: FfiRuntimeChangeRecord) -> Arc<Self> {
        let gap = initial.clone().requiring_resnapshot();
        let mut changes = VecDeque::with_capacity(CHANGE_BUFFER_CAPACITY);
        changes.push_back(initial);
        Arc::new(Self {
            state: Mutex::new(State {
                changes,
                dirty: false,
                closed: false,
                cancelled: false,
                #[cfg(test)]
                waiting: false,
            }),
            ready: Condvar::new(),
            gap,
        })
    }

    /// Returns false only after this queue stops admitting producer work.
    pub(crate) fn send(&self, change: FfiRuntimeChangeRecord) -> bool {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if state.closed {
            return false;
        }
        if state.changes.len() < CHANGE_BUFFER_CAPACITY {
            state.changes.push_back(change);
        } else {
            state.dirty = true;
        }
        self.ready.notify_one();
        true
    }

    pub(crate) fn receive(&self) -> Option<FfiRuntimeChangeRecord> {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            if state.cancelled {
                return None;
            }
            // Preserve first epoch admission even if publication beats the worker.
            if state
                .changes
                .front()
                .is_some_and(|value| value.kind == FfiRuntimeChangeKind::Initial)
            {
                return state.changes.pop_front();
            }
            if state.dirty {
                state.dirty = false;
                return Some(self.gap.clone());
            }
            if let Some(change) = state.changes.pop_front() {
                return Some(change);
            }
            if state.closed {
                return None;
            }
            #[cfg(test)]
            {
                state.waiting = true;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            #[cfg(test)]
            {
                state.waiting = false;
            }
        }
    }

    /// Drop stale ordinary callbacks but retain pending loss and final lifecycle.
    pub(crate) fn close(&self, lifecycle: FfiRuntimeChangeRecord) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !state.closed {
            state.changes.clear();
            state.changes.push_back(lifecycle);
            state.closed = true;
            self.ready.notify_one();
        }
    }

    /// An admitted callback may finish; no queued callback remains owned afterward.
    pub(crate) fn cancel(&self) {
        let mut state = self
            .state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.changes.clear();
        state.dirty = false;
        state.closed = true;
        state.cancelled = true;
        self.ready.notify_one();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        FfiInvalidationRevision, FfiRuntimeChangeDelivery, FfiRuntimeChangeScope,
        RUNTIME_CHANGE_SCHEMA_VERSION,
    };
    use std::time::Duration;

    fn change(kind: FfiRuntimeChangeKind, revision: u64) -> FfiRuntimeChangeRecord {
        FfiRuntimeChangeRecord {
            schema_version: RUNTIME_CHANGE_SCHEMA_VERSION,
            scope: FfiRuntimeChangeScope {
                public_key: "a".repeat(64),
                source_generation: "b".repeat(64),
                context: None,
            },
            epoch: "c".repeat(32),
            revision: FfiInvalidationRevision::Current { value: revision },
            delivery: FfiRuntimeChangeDelivery::Change,
            kind,
            entity_id: Some(revision.to_string()),
        }
    }

    #[test]
    fn exact_capacity_and_one_more_retain_a_gap_without_a_later_event() {
        let initial = change(FfiRuntimeChangeKind::Initial, 0);
        let queue = SubscriptionQueue::new(initial.clone());
        assert_eq!(queue.receive(), Some(initial.clone()));
        for revision in 1..=CHANGE_BUFFER_CAPACITY {
            assert!(queue.send(change(FfiRuntimeChangeKind::Drafts, revision as u64)));
        }
        {
            let state = queue.state.lock().unwrap();
            assert_eq!(state.changes.len(), CHANGE_BUFFER_CAPACITY);
            assert!(!state.dirty);
        }
        assert!(queue.send(change(FfiRuntimeChangeKind::Media, 99)));
        assert_eq!(
            queue.state.lock().unwrap().changes.len(),
            CHANGE_BUFFER_CAPACITY
        );
        let gap = queue.receive().unwrap();
        assert_eq!(gap, initial.requiring_resnapshot());
        for revision in 1..=CHANGE_BUFFER_CAPACITY {
            assert_eq!(
                queue.receive(),
                Some(change(FfiRuntimeChangeKind::Drafts, revision as u64))
            );
        }
        queue.cancel();
        assert_eq!(queue.receive(), None);
        assert!(!queue.send(change(FfiRuntimeChangeKind::Media, 100)));
    }

    #[test]
    fn close_preserves_final_gap_and_lifecycle_but_cancel_discards_queued_work() {
        for cancel in [false, true] {
            let initial = change(FfiRuntimeChangeKind::Initial, 0);
            let queue = SubscriptionQueue::new(initial.clone());
            for revision in 1..=CHANGE_BUFFER_CAPACITY {
                queue.send(change(FfiRuntimeChangeKind::Today, revision as u64));
            }
            // A raced publisher cannot replace the first epoch notification.
            assert_eq!(queue.receive(), Some(initial.clone()));
            let lifecycle = change(FfiRuntimeChangeKind::Lifecycle, 1);
            queue.close(lifecycle.clone());
            if cancel {
                queue.cancel();
            } else {
                assert_eq!(queue.receive(), Some(initial.requiring_resnapshot()));
                assert_eq!(queue.receive(), Some(lifecycle));
            }
            assert_eq!(queue.receive(), None);
            assert!(queue.state.lock().unwrap().changes.is_empty());
        }
    }

    #[test]
    fn idle_receiver_wakes_on_close_or_cancellation() {
        for cancel in [false, true] {
            let queue = SubscriptionQueue::new(change(FfiRuntimeChangeKind::Initial, 0));
            queue.receive().unwrap();
            let receiver = Arc::clone(&queue);
            let (sender, result) = std::sync::mpsc::channel();
            let worker = std::thread::spawn(move || sender.send(receiver.receive()).unwrap());
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !queue.state.lock().unwrap().waiting && std::time::Instant::now() < deadline {
                std::thread::yield_now();
            }
            assert!(queue.state.lock().unwrap().waiting);
            let lifecycle = change(FfiRuntimeChangeKind::Lifecycle, 1);
            if cancel {
                queue.cancel();
            } else {
                queue.close(lifecycle.clone());
            }
            assert_eq!(
                result.recv_timeout(Duration::from_secs(5)).unwrap(),
                if cancel { None } else { Some(lifecycle) }
            );
            worker.join().unwrap();
            assert_eq!(queue.receive(), None);
        }
    }
}
