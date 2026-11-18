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
            guard.retain(|(entry_id, _)| *entry_id != id);
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
        let callbacks: Vec<CancelCallback> = {
            let mut guard = self
                .inner
                .callbacks
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner());
            guard.drain(..).map(|(_, cb)| cb).collect()
        };
        for cb in callbacks {
            cb();
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
}
