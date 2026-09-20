//! Serializes Today state and retains cache bytes after uncertain backend writes.
//!
//! Dropping a storage future need not cancel work already dispatched to SQLite.
//! Only a fresh runtime after storage shutdown can discard this uncertainty.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use tokio::sync::{Mutex, MutexGuard};

#[derive(Default)]
pub(super) struct TodayProjectionFence {
    mutex: Mutex<()>,
    pending: AtomicUsize,
    uncertain: AtomicBool,
}

impl TodayProjectionFence {
    pub(super) async fn lock(&self) -> MutexGuard<'_, ()> {
        self.mutex.lock().await
    }

    /// Call while holding the Today mutex, before dispatching an ownership write.
    pub(super) fn begin_write(&self) -> WritePermit<'_> {
        self.pending.fetch_add(1, Ordering::AcqRel);
        WritePermit {
            fence: self,
            completed: false,
        }
    }

    #[cfg(any(feature = "mobile-social", test))]
    pub(super) fn can_collect(&self) -> bool {
        self.pending.load(Ordering::Acquire) == 0 && !self.uncertain.load(Ordering::Acquire)
    }
}

pub(super) struct WritePermit<'a> {
    fence: &'a TodayProjectionFence,
    completed: bool,
}

impl WritePermit<'_> {
    pub(super) fn complete(mut self) {
        self.completed = true;
    }
}

impl Drop for WritePermit<'_> {
    fn drop(&mut self) {
        if !self.completed {
            self.fence.uncertain.store(true, Ordering::Release);
        }
        self.fence.pending.fetch_sub(1, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        future::{Future, pending},
        task::{Context, Poll, Waker},
    };

    #[test]
    fn cancelled_dispatched_write_retains_uncertainty_after_later_success() {
        let fence = TodayProjectionFence::default();
        let write = || async {
            let permit = fence.begin_write();
            pending::<()>().await;
            permit.complete();
        };
        drop(write()); // An unpolled future never dispatched a write.
        assert!(fence.can_collect());
        let mut future = Box::pin(write());
        assert!(matches!(
            future
                .as_mut()
                .poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        assert!(!fence.can_collect());
        drop(future);
        fence.begin_write().complete();
        assert!(!fence.can_collect());
    }

    #[test]
    fn all_acknowledged_writes_must_finish_before_collection() {
        let fence = TodayProjectionFence::default();
        let first = fence.begin_write();
        let second = fence.begin_write();
        first.complete();
        assert!(!fence.can_collect());
        second.complete();
        assert!(fence.can_collect());
    }
}
