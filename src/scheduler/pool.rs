//! Fixed-size worker pool.

use crate::{
    app::view::refresh_ui,
    models::{FileState, JobConfig, SharedState},
    scheduler::worker::{mark_cancelled, process_video, update_row},
    AppWindow,
};
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    thread,
};

/// Runs the queue with up to `config.jobs` FFmpeg workers at the same time.
///
/// Workers share one atomic counter. Fetching the next number assigns a queue
/// row to exactly one worker without requiring a second job queue structure.
pub fn run_jobs(config: JobConfig, state: Arc<SharedState>, weak: slint::Weak<AppWindow>) {
    let next = Arc::new(AtomicUsize::new(0));
    let row_count = state.rows.lock().expect("rows mutex").len();
    let mut workers = Vec::new();

    for _ in 0..config.jobs.min(row_count.max(1)) {
        let config = config.clone();
        let state = state.clone();
        let weak = weak.clone();
        let next = next.clone();

        workers.push(thread::spawn(move || loop {
            let index = next.fetch_add(1, Ordering::SeqCst);
            if index >= state.rows.lock().expect("rows mutex").len() {
                break;
            }

            if state.cancel.load(Ordering::SeqCst) {
                mark_cancelled(index, &state, &weak);
                continue;
            }

            if let Err(error) = process_video(index, &config, &state, &weak) {
                update_row(index, &state, |row| {
                    row.state = FileState::Failed;
                    row.message = format!("{error:#}");
                });
                refresh_ui(&weak, &state);
            }
        }));
    }

    // Joining keeps the scheduler thread alive until every worker has stopped.
    for worker in workers {
        let _ = worker.join();
    }
}
