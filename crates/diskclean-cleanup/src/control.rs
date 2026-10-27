//! Cooperative cancellation and progress for confirmed cleanup batches.
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};

#[derive(Debug, Clone, Default)]
pub struct ApplyControl(Arc<State>);

#[derive(Debug, Default)]
struct State {
    cancelled: AtomicBool,
    completed: AtomicUsize,
    total: AtomicUsize,
}

impl ApplyControl {
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Relaxed);
    }
    #[must_use]
    pub fn is_cancelled(&self) -> bool {
        self.0.cancelled.load(Ordering::Relaxed)
    }
    #[must_use]
    pub fn progress(&self) -> (usize, usize) {
        (
            self.0.completed.load(Ordering::Relaxed),
            self.0.total.load(Ordering::Relaxed),
        )
    }
    pub(crate) fn update(&self, completed: usize, total: usize) {
        self.0.total.store(total, Ordering::Relaxed);
        self.0.completed.store(completed, Ordering::Relaxed);
    }
}
