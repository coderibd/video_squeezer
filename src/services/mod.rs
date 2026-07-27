//! External media services: directory scanning, FFprobe, FFmpeg planning, and images.

mod compression;
mod encoder;
mod ffprobe;
mod scanner;
mod thumbnails;

pub use compression::{build_plan, retry_video_bitrate};
pub use encoder::select_encoder;
pub use ffprobe::{probe_video, ProbeInfo};
pub use scanner::scan_videos;
pub use thumbnails::{create_contact_sheet, create_preview_frame};
