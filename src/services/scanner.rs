//! Directory scanning and initial metadata collection.

use crate::{
    models::{FileState, VideoRow},
    services::{probe_video, ProbeInfo},
    utils::is_video,
};
use std::{fs, path::Path};
use walkdir::WalkDir;

/// Finds supported video files and creates the initial queue rows.
///
/// FFprobe is called during scanning so dimensions and duration are available
/// before encoding starts. Failed probes do not stop the whole scan; the row is
/// retained with zero-valued metadata and can later report a worker error.
pub fn scan_videos(root: &Path, recursive: bool) -> Vec<VideoRow> {
    let walker = if recursive {
        WalkDir::new(root)
    } else {
        WalkDir::new(root).max_depth(1)
    };

    walker
        .follow_links(false)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| is_video(entry.path()))
        .map(|entry| {
            let path = entry.into_path();
            let original_bytes = fs::metadata(&path)
                .map(|metadata| metadata.len())
                .unwrap_or(0);
            let probe = probe_video(&path).unwrap_or(ProbeInfo {
                width: 0,
                height: 0,
                duration_secs: 0.0,
            });

            VideoRow {
                path,
                state: FileState::Queued,
                progress: 0.0,
                original_bytes,
                output_bytes: None,
                width: probe.width,
                height: probe.height,
                duration_secs: probe.duration_secs,
                preview_path: None,
                encoder: None,
                started_at: None,
                speed: 0.0,
                message: String::new(),
            }
        })
        .collect()
}
