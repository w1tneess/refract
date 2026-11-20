use crate::error::RefractError;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

type CancelCallback = Box<dyn FnOnce() + Send>;

struct Inner {
    canceled: AtomicBool,
    next_id: AtomicU64,
    callbacks: Mutex<Vec<(u64, CancelCallback)>>,
}

/// Cooperative, thread-safe cancellation token shared between the Desktop controller
/// and the native Rust engine.
///
/// Intentionally OS-agnostic: contains no subprocess handles or PIDs. Long-running
/// Rust loops poll [`CancelToken::is_canceled`] / [`CancelToken::check`], while
/// subprocess runners can poll or register a lightweight wake callback via
/// [`CancelToken::on_cancel`].
#[derive(Clone)]
pub struct CancelToken {
    inner: Arc<Inner>,
}

/// RAII guard that unregisters a cancellation callback when dropped before cancellation fires.
pub struct CancelGuard {
    inner: Arc<Inner>,
    id: Option<u64>,
}

impl Drop for CancelGuard {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            let mut guard = self
                .inner
                .callbacks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            let removed = guard
                .iter()
                .position(|(entry_id, _)| *entry_id == id)
                .map(|index| guard.remove(index));
            drop(guard);
            drop(removed);
        }
    }
}

impl Default for CancelToken {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for CancelToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CancelToken")
            .field("canceled", &self.is_canceled())
            .finish()
    }
}

impl CancelToken {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Inner {
                canceled: AtomicBool::new(false),
                next_id: AtomicU64::new(1),
                callbacks: Mutex::new(Vec::new()),
            }),
        }
    }

    /// Convenience helper for contexts where cancellation is not wired (e.g. unit tests).
    pub fn none() -> Self {
        Self::new()
    }

    /// Returns `true` once [`CancelToken::cancel`] has been called.
    pub fn is_canceled(&self) -> bool {
        self.inner.canceled.load(Ordering::Acquire)
    }

    /// Cooperative checkpoint for processing loops.
    pub fn check(&self) -> Result<(), RefractError> {
        if self.is_canceled() {
            Err(RefractError::canceled())
        } else {
            Ok(())
        }
    }

    /// Mark this token canceled and invoke any registered callbacks exactly once,
    /// without holding the internal mutex during callback execution.
    pub fn cancel(&self) {
        if self.inner.canceled.swap(true, Ordering::AcqRel) {
            return;
        }
        let mut panic = None;
        loop {
            let callback = {
                let mut guard = self
                    .inner
                    .callbacks
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                if guard.is_empty() {
                    None
                } else {
                    Some(guard.remove(0).1)
                }
            };
            let Some(cb) = callback else { break };
            if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(cb)) {
                if panic.is_none() {
                    panic = Some(payload);
                }
            }
        }
        if let Some(payload) = panic {
            std::panic::resume_unwind(payload);
        }
    }

    /// Register a callback that fires when this token is canceled.
    /// If already canceled, the callback runs immediately.
    /// Dropping the returned [`CancelGuard`] unregisters the callback if it has not fired yet.
    pub fn on_cancel<F>(&self, callback: F) -> CancelGuard
    where
        F: FnOnce() + Send + 'static,
    {
        if self.is_canceled() {
            callback();
            return CancelGuard {
                inner: Arc::clone(&self.inner),
                id: None,
            };
        }

        let id = self.inner.next_id.fetch_add(1, Ordering::Relaxed);
        let mut run_now: Option<CancelCallback> = None;
        {
            let mut guard = self
                .inner
                .callbacks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            if self.is_canceled() {
                run_now = Some(Box::new(callback));
            } else {
                guard.push((id, Box::new(callback)));
            }
        }

        if let Some(cb) = run_now {
            cb();
            CancelGuard {
                inner: Arc::clone(&self.inner),
                id: None,
            }
        } else {
            CancelGuard {
                inner: Arc::clone(&self.inner),
                id: Some(id),
            }
        }
    }

    /// Create a child token that cancels when `self` cancels, without canceling `self`
    /// when the child is canceled independently.
    pub fn child(&self) -> (CancelToken, CancelGuard) {
        let child = CancelToken::new();
        let child_clone = child.clone();
        let guard = self.on_cancel(move || child_clone.cancel());
        (child, guard)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::ErrorCode;
    use std::sync::atomic::AtomicUsize;

    #[test]
    fn cooperative_check_and_idempotent_cancel() {
        let token = CancelToken::new();
        assert!(!token.is_canceled());
        assert!(token.check().is_ok());

        let count = Arc::new(AtomicUsize::new(0));
        let c = Arc::clone(&count);
        let _guard = token.on_cancel(move || {
            c.fetch_add(1, Ordering::SeqCst);
        });

        token.cancel();
        token.cancel(); // idempotent

        assert!(token.is_canceled());
        assert_eq!(token.check().unwrap_err().code, ErrorCode::Canceled);
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn registering_callback_after_cancel_invokes_immediately() {
        let token = CancelToken::new();
        token.cancel();

        let fired = Arc::new(AtomicBool::new(false));
        let f = Arc::clone(&fired);
        let _guard = token.on_cancel(move || f.store(true, Ordering::SeqCst));
        assert!(fired.load(Ordering::SeqCst));
    }

    #[test]
    fn dropping_guard_unregisters_callback() {
        let token = CancelToken::new();
        let fired = Arc::new(AtomicBool::new(false));
        let f = Arc::clone(&fired);
        {
            let _guard = token.on_cancel(move || f.store(true, Ordering::SeqCst));
        }
        token.cancel();
        assert!(!fired.load(Ordering::SeqCst));
    }

    #[test]
    fn child_token_propagates_parent_cancel_only_downward() {
        let parent = CancelToken::new();
        let (child1, _g1) = parent.child();
        let (child2, _g2) = parent.child();

        child1.cancel();
        assert!(child1.is_canceled());
        assert!(!parent.is_canceled());
        assert!(!child2.is_canceled());

        parent.cancel();
        assert!(parent.is_canceled());
        assert!(child2.is_canceled());
    }
    #[test]
    fn nested_guard_drop_does_not_deadlock() {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let token = CancelToken::new();
            let a = token.on_cancel(|| {});
            let b = token.on_cancel(move || drop(a));
            drop(b);
            tx.send(()).unwrap();
        });
        rx.recv_timeout(std::time::Duration::from_secs(2)).unwrap();
    }

    #[test]
    fn dispatch_allows_unregistering_pending_callbacks() {
        let token = CancelToken::new();
        let pending = Arc::new(Mutex::new(None));
        let drop_pending = pending.clone();
        let _a = token.on_cancel(move || {
            drop_pending.lock().unwrap().take();
        });
        let fired = Arc::new(AtomicBool::new(false));
        let f = fired.clone();
        *pending.lock().unwrap() = Some(token.on_cancel(move || f.store(true, Ordering::SeqCst)));
        token.cancel();
        assert!(!fired.load(Ordering::SeqCst));
    }

    #[test]
    fn panic_does_not_skip_remaining_callbacks() {
        let token = CancelToken::new();
        let _a = token.on_cancel(|| panic!("callback failure"));
        let count = Arc::new(AtomicUsize::new(0));
        let c = count.clone();
        let _b = token.on_cancel(move || {
            c.fetch_add(1, Ordering::SeqCst);
        });
        assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| token.cancel())).is_err());
        token.cancel();
        assert_eq!(count.load(Ordering::SeqCst), 1);
    }
}
