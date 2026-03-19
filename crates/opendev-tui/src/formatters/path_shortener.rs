//! Centralized path shortening for all TUI display.
//!
//! `PathShortener` caches the home directory and working directory at construction
//! time, avoiding repeated `dirs::home_dir()` syscalls. All path display in the TUI
//! should flow through this struct.

/// Caches home_dir and working_dir at construction time.
/// All methods are cheap string operations — no syscalls after construction.
#[derive(Debug, Clone)]
pub struct PathShortener {
    working_dir: Option<String>,
    home_dir: Option<String>,
}

impl PathShortener {
    /// Construct with cached dirs. Calls `dirs::home_dir()` exactly once.
    pub fn new(working_dir: Option<&str>) -> Self {
        Self {
            working_dir: working_dir.filter(|s| !s.is_empty()).map(|s| s.to_string()),
            home_dir: dirs::home_dir().map(|h| h.to_string_lossy().into_owned()),
        }
    }

    /// Single path: wd-prefix → relative, home-prefix → ~/…, else as-is.
    pub fn shorten(&self, path: &str) -> String {
        // Try working dir first
        if let Some(ref wd) = self.working_dir
            && path.starts_with(wd.as_str())
        {
            let rel = path.strip_prefix(wd.as_str()).unwrap_or(path);
            let rel = rel.strip_prefix('/').unwrap_or(rel);
            if rel.is_empty() {
                return ".".to_string();
            }
            return rel.to_string();
        }
        // Strip leading "./"
        let cleaned = path.strip_prefix("./").unwrap_or(path);
        // Try home dir
        self.replace_home_prefix(cleaned)
    }

    /// Free-form text: replace all occurrences of wd and home with short forms.
    pub fn shorten_text(&self, text: &str) -> String {
        let result = if let Some(ref wd) = self.working_dir {
            // Pass 1: replace "wd/" → "" (slash is a natural boundary)
            let wd_slash = format!("{wd}/");
            let result = text.replace(&wd_slash, "");
            // Pass 2: replace standalone "wd" → "." at path boundaries
            self.replace_at_boundary(&result, wd, ".")
        } else {
            text.to_string()
        };
        // Pass 3: replace home dir paths with ~/...
        self.replace_home_in_text(&result)
    }

    /// Shorten a path for status bar display: home → ~, then keep it compact.
    ///
    /// - Paths under home: `~/codes/opendev` stays as-is (≤3 components after ~),
    ///   longer paths like `~/a/b/c/d` become `~/…/c/d`.
    /// - Paths outside home with >3 components: `.../last/two`.
    pub fn shorten_display(&self, path: &str) -> String {
        let display = self.replace_home_prefix(path);

        if let Some(after_tilde) = display.strip_prefix("~/") {
            let parts: Vec<&str> = after_tilde.split('/').filter(|p| !p.is_empty()).collect();
            if parts.len() <= 3 {
                return display;
            }
            // ~/a/b/c/d → ~/…/c/d
            return format!("~/…/{}", parts[parts.len() - 2..].join("/"));
        }

        // Non-home paths (e.g. /usr/local/share/app)
        let parts: Vec<&str> = display.split('/').filter(|p| !p.is_empty()).collect();
        if parts.len() <= 3 {
            return display;
        }
        format!("…/{}", parts[parts.len() - 2..].join("/"))
    }

    /// Replace the home directory prefix with `~` in a single path.
    fn replace_home_prefix(&self, path: &str) -> String {
        if let Some(ref home) = self.home_dir
            && let Some(rest) = path.strip_prefix(home.as_str())
        {
            let rest = rest.strip_prefix('/').unwrap_or(rest);
            if rest.is_empty() {
                return "~".to_string();
            }
            return format!("~/{rest}");
        }
        path.to_string()
    }

    /// Replace home directory paths in free-form text with `~/...`.
    fn replace_home_in_text(&self, text: &str) -> String {
        let home = match self.home_dir {
            Some(ref h) => h,
            None => return text.to_string(),
        };
        // Replace "home/" → "~/" (slash is a natural boundary)
        let home_slash = format!("{home}/");
        let result = text.replace(&home_slash, "~/");
        // Replace standalone home dir at boundaries
        self.replace_at_boundary(&result, home, "~")
    }

    /// Replace `needle` with `replacement` only at path boundaries.
    /// A boundary means the character after `needle` is NOT alphanumeric, '-', '_', or '.'.
    fn replace_at_boundary(&self, text: &str, needle: &str, replacement: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut remaining = text;
        while let Some(pos) = remaining.find(needle) {
            out.push_str(&remaining[..pos]);
            let after = &remaining[pos + needle.len()..];
            let extends_path = after
                .as_bytes()
                .first()
                .is_some_and(|&b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.');
            if extends_path {
                out.push_str(needle);
            } else {
                out.push_str(replacement);
            }
            remaining = after;
        }
        out.push_str(remaining);
        out
    }
}

impl Default for PathShortener {
    fn default() -> Self {
        Self::new(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> String {
        dirs::home_dir().unwrap().to_string_lossy().into_owned()
    }

    #[test]
    fn test_shorten_relative_to_working_dir() {
        let home = home();
        let ps = PathShortener::new(Some(&format!("{home}/project")));
        assert_eq!(
            ps.shorten(&format!("{home}/project/src/main.rs")),
            "src/main.rs"
        );
    }

    #[test]
    fn test_shorten_working_dir_itself() {
        let home = home();
        let ps = PathShortener::new(Some(&format!("{home}/project")));
        assert_eq!(ps.shorten(&format!("{home}/project")), ".");
    }

    #[test]
    fn test_shorten_outside_working_dir_uses_tilde() {
        let home = home();
        let ps = PathShortener::new(Some(&format!("{home}/project")));
        assert_eq!(
            ps.shorten(&format!("{home}/other/src/main.rs")),
            "~/other/src/main.rs"
        );
    }

    #[test]
    fn test_shorten_home_dir_itself() {
        let home = home();
        let ps = PathShortener::new(Some("/some/other/dir"));
        assert_eq!(ps.shorten(&home), "~");
    }

    #[test]
    fn test_shorten_strips_dot_slash() {
        let ps = PathShortener::new(Some("/project"));
        assert_eq!(ps.shorten("./src/main.rs"), "src/main.rs");
    }

    #[test]
    fn test_shorten_text_replaces_wd() {
        let home = home();
        let ps = PathShortener::new(Some(&format!("{home}/project")));
        let text = format!("Explore repo at {home}/project/src with focus on tests");
        assert_eq!(
            ps.shorten_text(&text),
            "Explore repo at src with focus on tests"
        );
    }

    #[test]
    fn test_shorten_text_standalone_wd() {
        let home = home();
        let ps = PathShortener::new(Some(&format!("{home}/project")));
        let text = format!("List({home}/project)");
        assert_eq!(ps.shorten_text(&text), "List(.)");
    }

    #[test]
    fn test_shorten_text_boundary_safety() {
        let ps = PathShortener::new(Some("/project"));
        let text = "Explore /project-v2/src";
        assert_eq!(ps.shorten_text(text), "Explore /project-v2/src");
    }

    #[test]
    fn test_shorten_text_home_fallback() {
        let home = home();
        let ps = PathShortener::new(Some(&format!("{home}/my-project")));
        let text = format!("List({home}/other-project)");
        assert_eq!(ps.shorten_text(&text), "List(~/other-project)");
    }

    #[test]
    fn test_shorten_text_no_working_dir() {
        let home = home();
        let ps = PathShortener::new(None);
        let text = format!("some text {home}/project/file.rs");
        assert_eq!(ps.shorten_text(&text), "some text ~/project/file.rs");
    }

    #[test]
    fn test_shorten_display_short_path() {
        let ps = PathShortener::default();
        assert_eq!(ps.shorten_display("/home/user"), "/home/user");
    }

    #[test]
    fn test_shorten_display_long_non_home_path() {
        let ps = PathShortener::default();
        assert_eq!(
            ps.shorten_display("/a/b/c/d/myapp"),
            "…/d/myapp"
        );
    }

    #[test]
    fn test_shorten_display_home_short() {
        let home = home();
        let ps = PathShortener::default();
        // ~/codes/opendev → stays as-is (≤3 parts after ~)
        let result = ps.shorten_display(&format!("{home}/codes/opendev"));
        assert_eq!(result, "~/codes/opendev");
    }

    #[test]
    fn test_shorten_display_home_long() {
        let home = home();
        let ps = PathShortener::default();
        // ~/a/b/c/d → ~/…/c/d
        let result = ps.shorten_display(&format!("{home}/a/b/c/d"));
        assert_eq!(result, "~/…/c/d");
    }

    #[test]
    fn test_default_no_working_dir() {
        let ps = PathShortener::default();
        assert!(ps.working_dir.is_none());
        assert!(ps.home_dir.is_some());
    }
}
