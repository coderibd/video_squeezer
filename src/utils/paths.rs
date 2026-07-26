//! File-extension and filename helpers.

use std::path::Path;

/// Returns true when the path has one of the supported video extensions.
pub fn is_video(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|extension| extension.to_str())
            .map(|extension| extension.to_ascii_lowercase())
            .as_deref(),
        Some(
            "mp4"
                | "mkv"
                | "mov"
                | "avi"
                | "m4v"
                | "webm"
                | "wmv"
                | "mpg"
                | "mpeg"
                | "ts"
                | "mts"
                | "m2ts"
        )
    )
}

/// Replaces characters that are awkward in generated temporary filenames.
pub fn sanitize_filename(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn video_extension_check_is_case_insensitive() {
        assert!(is_video(Path::new("movie.MKV")));
        assert!(is_video(Path::new("clip.Mp4")));
        assert!(!is_video(Path::new("notes.txt")));
    }

    #[test]
    fn sanitization_preserves_safe_characters() {
        assert_eq!(sanitize_filename("movie-2026_final"), "movie-2026_final");
    }

    #[test]
    fn sanitization_replaces_unsafe_characters() {
        assert_eq!(
            sanitize_filename("a file (final).mkv"),
            "a_file__final__mkv"
        );
    }
}
