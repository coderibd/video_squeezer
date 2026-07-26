//! Thread-safe application state.

use super::VideoRow;
use std::sync::{atomic::AtomicBool, Mutex};

/// State shared by the GUI thread, scanner thread, and encoding workers.
///
/// `Mutex` protects collections that require multi-step access. Atomic flags
/// are used for simple yes/no values that workers check frequently.
#[derive(Default)]
pub struct SharedState {
    pub rows: Mutex<Vec<VideoRow>>,
    pub selected: Mutex<Option<usize>>,
    pub cancel: AtomicBool,
    pub paused: AtomicBool,
    pub running: AtomicBool,
    pub scanning: AtomicBool,
}
