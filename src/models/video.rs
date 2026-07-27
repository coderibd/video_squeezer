//! Queue item and lifecycle definitions.

use slint::Color;
use std::{path::PathBuf, time::Instant};

/// Lifecycle state for one video in the processing queue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileState {
    Queued,
    Processing,
    Completed,
    Skipped,
    Failed,
    Cancelled,
}

impl FileState {
    /// Human-readable label displayed in the queue.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Queued => "Queued",
            Self::Processing => "Processing",
            Self::Completed => "Completed",
            Self::Skipped => "Skipped",
            Self::Failed => "Failed",
            Self::Cancelled => "Cancelled",
        }
    }

    /// Compact symbol displayed beside the status label.
    pub fn icon(&self) -> &'static str {
        match self {
            Self::Queued => "◷",
            Self::Processing => "▶",
            Self::Completed => "✓",
            Self::Skipped => "↷",
            Self::Failed => "!",
            Self::Cancelled => "■",
        }
    }

    /// Status color used by the Slint queue row.
    pub fn color(&self) -> Color {
        match self {
            Self::Queued => Color::from_rgb_u8(132, 139, 150),
            Self::Processing => Color::from_rgb_u8(38, 112, 238),
            Self::Completed => Color::from_rgb_u8(49, 169, 82),
            Self::Skipped => Color::from_rgb_u8(128, 76, 204),
            Self::Failed => Color::from_rgb_u8(211, 66, 66),
            Self::Cancelled => Color::from_rgb_u8(235, 144, 24),
        }
    }
}

/// Runtime information for one source video.
///
/// This object is updated as the file moves from queued to processing and then
/// to a terminal state. A clone is sent to the UI refresh layer so the mutex is
/// never held while drawing the interface.
#[derive(Debug, Clone)]
pub struct VideoRow {
    pub path: PathBuf,
    pub state: FileState,
    pub progress: f32,
    pub original_bytes: u64,
    pub output_bytes: Option<u64>,
    pub width: u32,
    pub height: u32,
    pub duration_secs: f64,
    pub fps: f64,
    pub preview_path: Option<PathBuf>,
    pub encoder: Option<String>,
    pub started_at: Option<Instant>,
    pub speed: f64,
    pub message: String,

    // Compression Advisor results. These are calculated before encoding so the
    // user can understand the trade-off between target size and resolution.
    pub predicted_output_bytes: Option<u64>,
    pub planned_width: Option<u32>,
    pub planned_height: Option<u32>,
    pub recommended_video_bps: Option<u64>,
    pub quality_label: String,
    pub advisor_message: String,
    pub encode_attempt: usize,
}

impl VideoRow {
    /// Returns only the final filename, suitable for display in the queue.
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<unknown>")
            .to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_state_has_a_display_label_and_icon() {
        let states = [
            FileState::Queued,
            FileState::Processing,
            FileState::Completed,
            FileState::Skipped,
            FileState::Failed,
            FileState::Cancelled,
        ];

        for state in states {
            assert!(!state.label().is_empty());
            assert!(!state.icon().is_empty());
        }
    }
}
