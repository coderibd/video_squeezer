//! Shared data structures used by the UI, scanner, scheduler, and workers.

mod settings;
mod state;
mod video;

pub use settings::{Codec, EncoderMode, JobConfig, QualityStrategy};
pub use state::SharedState;
pub use video::{FileState, VideoRow};
