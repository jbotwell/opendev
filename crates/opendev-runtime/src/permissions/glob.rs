//! Glob matching supporting `*` and `**` patterns.
//!
//! Two modes are provided:
//! - [`glob_matches`]: permission mode where `*` matches any character including `/`.
//! - [`glob_matches_path`]: path mode where `*` does not match `/` but `**` does.

/// Glob matching for **tool permission patterns**.
///
/// `*` matches any characters including `/`, since tool arguments commonly contain paths.
/// The pattern is anchored: it must match the entire input string.
pub fn glob_matches(pattern: &str, input: &str) -> bool {
    glob_matches_impl(pattern.as_bytes(), input.as_bytes(), false)
}

/// Path-aware glob matching where `*` does NOT match `/` but `**` does.
///
/// Used for directory scope patterns like `src/**` or `vendor/*`.
pub fn glob_matches_path(pattern: &str, input: &str) -> bool {
    glob_matches_impl(pattern.as_bytes(), input.as_bytes(), true)
}

/// Core glob implementation.
///
/// When `slash_sensitive` is true, `*` does not match `/` (path mode).
/// When false, `*` matches any character (permission mode).
fn glob_matches_impl(pattern: &[u8], input: &[u8], slash_sensitive: bool) -> bool {
    let mut pi = 0;
    let mut ii = 0;
    let mut star_pi = usize::MAX;
    let mut star_ii = 0;
    // Track `**` separately since it always matches `/`
    let mut dstar_pi = usize::MAX;
    let mut dstar_ii = 0;

    while ii < input.len() {
        if pi + 1 < pattern.len() && pattern[pi] == b'*' && pattern[pi + 1] == b'*' {
            // `**` — matches everything including `/`
            dstar_pi = pi;
            dstar_ii = ii;
            pi += 2;
            // Skip trailing `/` after `**`
            if pi < pattern.len() && pattern[pi] == b'/' {
                pi += 1;
            }
            continue;
        } else if pi < pattern.len() && pattern[pi] == b'*' {
            // `*` — matches everything (or everything except `/` in path mode)
            star_pi = pi;
            star_ii = ii;
            pi += 1;
            continue;
        } else if pi < pattern.len() && (pattern[pi] == input[ii] || pattern[pi] == b'?') {
            pi += 1;
            ii += 1;
            continue;
        }

        // Backtrack to single `*`
        if star_pi != usize::MAX && (!slash_sensitive || input[star_ii] != b'/') {
            star_ii += 1;
            ii = star_ii;
            pi = star_pi + 1;
            continue;
        }

        // Backtrack to `**`
        if dstar_pi != usize::MAX {
            dstar_ii += 1;
            ii = dstar_ii;
            pi = dstar_pi + 2;
            if pi < pattern.len() && pattern[pi] == b'/' {
                pi += 1;
            }
            continue;
        }

        return false;
    }

    // Consume trailing `*` or `**`
    while pi < pattern.len() && pattern[pi] == b'*' {
        pi += 1;
    }

    pi == pattern.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_glob_exact_match() {
        assert!(glob_matches("hello", "hello"));
        assert!(!glob_matches("hello", "world"));
    }

    #[test]
    fn test_glob_star() {
        assert!(glob_matches("bash:*", "bash:ls -la"));
        assert!(glob_matches("edit:*", "edit:foo.rs"));
        assert!(!glob_matches("bash:*", "edit:foo.rs"));
    }

    #[test]
    fn test_glob_double_star() {
        assert!(glob_matches("src/**", "src/foo/bar/baz.rs"));
        assert!(glob_matches("**/*.rs", "src/foo/bar/baz.rs"));
        assert!(!glob_matches("src/**", "vendor/foo.rs"));
    }

    #[test]
    fn test_glob_question_mark() {
        assert!(glob_matches("ba?h:*", "bash:cmd"));
        assert!(!glob_matches("ba?h:*", "batch:cmd"));
    }

    #[test]
    fn test_glob_star_matches_slash_in_permission_mode() {
        // In permission mode, `*` matches any char including `/`
        assert!(glob_matches("bash:*", "bash:cat /etc/passwd"));
        assert!(glob_matches("edit:*", "edit:src/foo/bar.rs"));
    }

    #[test]
    fn test_glob_path_star_no_slash() {
        // In path mode, single `*` should not match `/`
        assert!(!glob_matches_path("src/*", "src/foo/bar.rs"));
        assert!(glob_matches_path("src/*", "src/bar.rs"));
    }
}
