//! Text and path utilities for LSP operations.
//!
//! Provides line/column <-> byte offset conversions, URI handling,
//! and file content helpers needed by symbol tools.

use std::path::{Path, PathBuf};

use crate::protocol::{Position, SourceRange};

/// Text utilities for converting between line/column and byte offsets.
pub struct TextUtils;

impl TextUtils {
    /// Convert a (line, column) position to a byte offset in the text.
    ///
    /// Returns `None` if the position is out of bounds.
    pub fn position_to_offset(text: &str, pos: Position) -> Option<usize> {
        let mut offset = 0usize;

        for (current_line, line_content) in text.split('\n').enumerate() {
            if current_line as u32 == pos.line {
                let col = pos.character as usize;
                if col <= line_content.len() {
                    return Some(offset + col);
                } else {
                    // Column beyond line length — clamp to end
                    return Some(offset + line_content.len());
                }
            }
            // +1 for the newline character
            offset += line_content.len() + 1;
        }

        None
    }

    /// Convert a byte offset to a (line, column) position.
    ///
    /// Returns `None` if offset is beyond the text length.
    pub fn offset_to_position(text: &str, offset: usize) -> Option<Position> {
        if offset > text.len() {
            return None;
        }

        let mut line = 0u32;
        let mut col = 0u32;

        for (i, ch) in text.char_indices() {
            if i == offset {
                return Some(Position::new(line, col));
            }
            if ch == '\n' {
                line += 1;
                col = 0;
            } else {
                col += 1;
            }
        }

        // Offset at end of text
        if offset == text.len() {
            Some(Position::new(line, col))
        } else {
            None
        }
    }

    /// Extract a substring from text using a SourceRange.
    pub fn extract_range(text: &str, range: &SourceRange) -> Option<String> {
        let start = Self::position_to_offset(text, range.start)?;
        let end = Self::position_to_offset(text, range.end)?;
        if end > text.len() || start > end {
            return None;
        }
        Some(text[start..end].to_string())
    }

    /// Get the line content at a given line number (0-indexed).
    pub fn get_line(text: &str, line: u32) -> Option<&str> {
        text.split('\n').nth(line as usize)
    }

    /// Count the total number of lines.
    pub fn line_count(text: &str) -> u32 {
        text.split('\n').count() as u32
    }

    /// Replace a range of text with new content.
    pub fn replace_range(text: &str, range: &SourceRange, replacement: &str) -> Option<String> {
        let start = Self::position_to_offset(text, range.start)?;
        let end = Self::position_to_offset(text, range.end)?;
        if end > text.len() || start > end {
            return None;
        }
        let mut result = String::with_capacity(text.len() - (end - start) + replacement.len());
        result.push_str(&text[..start]);
        result.push_str(replacement);
        result.push_str(&text[end..]);
        Some(result)
    }
}

/// Path utilities for LSP file/URI operations.
pub struct PathUtils;

impl PathUtils {
    /// Convert a file path to a file:// URI string.
    pub fn path_to_uri_string(path: &Path) -> String {
        let absolute = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir().unwrap_or_default().join(path)
        };
        format!("file://{}", absolute.display())
    }

    /// Parse a file:// URI string into a path.
    pub fn uri_string_to_path(uri: &str) -> Option<PathBuf> {
        uri.strip_prefix("file://").map(PathBuf::from)
    }

    /// Normalize a path (resolve `.` and `..` without touching the filesystem).
    pub fn normalize(path: &Path) -> PathBuf {
        let mut components = Vec::new();
        for component in path.components() {
            match component {
                std::path::Component::ParentDir => {
                    components.pop();
                }
                std::path::Component::CurDir => {}
                other => components.push(other),
            }
        }
        components.iter().collect()
    }

    /// Get file extension from a path.
    pub fn extension(path: &Path) -> Option<String> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|s| s.to_lowercase())
    }
}

/// File reading utilities.
pub struct FileUtils;

impl FileUtils {
    /// Read a file to string, returning an error with context.
    pub fn read_to_string(path: &Path) -> Result<String, std::io::Error> {
        std::fs::read_to_string(path)
            .map_err(|e| std::io::Error::new(e.kind(), format!("{}: {}", path.display(), e)))
    }

    /// Write content to a file atomically (via tmp + rename).
    pub fn atomic_write(path: &Path, content: &str) -> Result<(), std::io::Error> {
        let parent = path
            .parent()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::InvalidInput, "no parent"))?;
        std::fs::create_dir_all(parent)?;

        let tmp_path = parent.join(format!(
            ".{}.tmp",
            path.file_name().and_then(|n| n.to_str()).unwrap_or("file")
        ));
        std::fs::write(&tmp_path, content)?;
        std::fs::rename(&tmp_path, path)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "line zero\nline one\nline two\nline three";

    #[test]
    fn test_position_to_offset() {
        // "line zero\nline one\nline two\nline three"
        // line 0: 0..9  (line zero)
        // line 1: 10..17 (line one)
        assert_eq!(
            TextUtils::position_to_offset(SAMPLE, Position::new(0, 0)),
            Some(0)
        );
        assert_eq!(
            TextUtils::position_to_offset(SAMPLE, Position::new(0, 4)),
            Some(4)
        );
        assert_eq!(
            TextUtils::position_to_offset(SAMPLE, Position::new(1, 0)),
            Some(10)
        );
        assert_eq!(
            TextUtils::position_to_offset(SAMPLE, Position::new(1, 5)),
            Some(15)
        );
    }

    #[test]
    fn test_offset_to_position() {
        assert_eq!(
            TextUtils::offset_to_position(SAMPLE, 0),
            Some(Position::new(0, 0))
        );
        assert_eq!(
            TextUtils::offset_to_position(SAMPLE, 4),
            Some(Position::new(0, 4))
        );
        assert_eq!(
            TextUtils::offset_to_position(SAMPLE, 10),
            Some(Position::new(1, 0))
        );
    }

    #[test]
    fn test_offset_to_position_end_of_text() {
        let text = "abc";
        assert_eq!(
            TextUtils::offset_to_position(text, 3),
            Some(Position::new(0, 3))
        );
        assert_eq!(TextUtils::offset_to_position(text, 4), None);
    }

    #[test]
    fn test_extract_range() {
        let range = SourceRange::new(Position::new(0, 5), Position::new(0, 9));
        assert_eq!(
            TextUtils::extract_range(SAMPLE, &range),
            Some("zero".to_string())
        );

        let range = SourceRange::new(Position::new(1, 5), Position::new(1, 8));
        assert_eq!(
            TextUtils::extract_range(SAMPLE, &range),
            Some("one".to_string())
        );
    }

    #[test]
    fn test_get_line() {
        assert_eq!(TextUtils::get_line(SAMPLE, 0), Some("line zero"));
        assert_eq!(TextUtils::get_line(SAMPLE, 3), Some("line three"));
        assert_eq!(TextUtils::get_line(SAMPLE, 4), None);
    }

    #[test]
    fn test_line_count() {
        assert_eq!(TextUtils::line_count(SAMPLE), 4);
        assert_eq!(TextUtils::line_count("single"), 1);
        assert_eq!(TextUtils::line_count("a\nb"), 2);
    }

    #[test]
    fn test_replace_range() {
        let range = SourceRange::new(Position::new(0, 5), Position::new(0, 9));
        let result = TextUtils::replace_range(SAMPLE, &range, "ZERO").unwrap();
        assert!(result.starts_with("line ZERO\n"));
    }

    #[cfg(unix)]
    #[test]
    fn test_path_to_uri_string() {
        let path = Path::new("/tmp/test.rs");
        let uri = PathUtils::path_to_uri_string(path);
        assert_eq!(uri, "file:///tmp/test.rs");
    }

    #[test]
    fn test_uri_string_to_path() {
        let path = PathUtils::uri_string_to_path("file:///tmp/test.rs").unwrap();
        assert_eq!(path, PathBuf::from("/tmp/test.rs"));
        assert!(PathUtils::uri_string_to_path("http://example.com").is_none());
    }

    #[test]
    fn test_normalize_path() {
        let path = Path::new("/a/b/../c/./d");
        let normalized = PathUtils::normalize(path);
        assert_eq!(normalized, PathBuf::from("/a/c/d"));
    }

    #[test]
    fn test_extension() {
        assert_eq!(
            PathUtils::extension(Path::new("foo.RS")),
            Some("rs".to_string())
        );
        assert_eq!(PathUtils::extension(Path::new("no_ext")), None);
    }

    #[test]
    fn test_atomic_write() {
        let tmp = tempfile::TempDir::new().unwrap();
        let path = tmp.path().join("test.txt");
        FileUtils::atomic_write(&path, "hello world").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "hello world");
    }
}
