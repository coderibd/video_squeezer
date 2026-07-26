//! Concurrent job scheduler and individual video workers.

mod pool;
mod worker;

pub use pool::run_jobs;
