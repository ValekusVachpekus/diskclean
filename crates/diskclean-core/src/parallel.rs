//! Bounded, per-operation workers shared by metadata reads and hashing.

/// Maximum configurable workers per operation (and concurrent hashing readers).
pub const MAX_WORKER_THREADS: usize = 32;

/// Resolve 0 (automatic) conservatively for filesystem I/O. Explicit values are
/// clamped so malformed persisted preferences cannot exhaust OS resources.
#[must_use]
pub fn worker_threads(requested: usize) -> usize {
    if requested == 0 {
        std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(8)
    } else {
        requested.clamp(1, MAX_WORKER_THREADS)
    }
}

pub(crate) fn pool(threads: usize) -> Result<rayon::ThreadPool, rayon::ThreadPoolBuildError> {
    rayon::ThreadPoolBuilder::new()
        .num_threads(worker_threads(threads))
        .thread_name(|index| format!("diskclean-worker-{index}"))
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_limits_and_pool_are_local() {
        assert!((1..=8).contains(&worker_threads(0)));
        assert_eq!(worker_threads(usize::MAX), MAX_WORKER_THREADS);
        for count in [1, 4, 2] {
            let pool = pool(count).expect("worker pool");
            assert_eq!(pool.install(rayon::current_num_threads), count);
        }
    }
}
