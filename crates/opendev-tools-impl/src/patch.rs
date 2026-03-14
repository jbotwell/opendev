//! Patch tool — apply unified diff patches to files.

use std::collections::HashMap;
use std::path::Path;

use opendev_tools_core::{BaseTool, ToolContext, ToolResult};

/// Tool for applying unified diff patches.
#[derive(Debug)]
pub struct PatchTool;

#[async_trait::async_trait]
impl BaseTool for PatchTool {
    fn name(&self) -> &str {
        "patch"
    }

    fn description(&self) -> &str {
        "Apply a unified diff patch to files in the working directory."
    }

    fn parameter_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "patch": {
                    "type": "string",
                    "description": "Unified diff patch content"
                },
                "strip": {
                    "type": "integer",
                    "description": "Number of leading path components to strip (default: 1)"
                }
            },
            "required": ["patch"]
        })
    }

    async fn execute(
        &self,
        args: HashMap<String, serde_json::Value>,
        ctx: &ToolContext,
    ) -> ToolResult {
        let patch_content = match args.get("patch").and_then(|v| v.as_str()) {
            Some(p) => p,
            None => return ToolResult::fail("patch is required"),
        };

        let strip = args.get("strip").and_then(|v| v.as_u64()).unwrap_or(1) as usize;

        let cwd = &ctx.working_dir;

        // Try git apply first
        let result = try_git_apply(patch_content, cwd, strip).await;
        if result.success {
            return result;
        }

        // Fall back to manual patch application
        apply_patch_manually(patch_content, cwd, strip)
    }
}

async fn try_git_apply(patch: &str, cwd: &Path, strip: usize) -> ToolResult {
    let strip_arg = format!("-p{strip}");

    let mut child = match tokio::process::Command::new("git")
        .args(["apply", &strip_arg, "--stat", "-"])
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return ToolResult::fail("git not available"),
    };

    // Write patch to stdin
    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(patch.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }

    let output = match child.wait_with_output().await {
        Ok(o) => o,
        Err(e) => return ToolResult::fail(format!("git apply failed: {e}")),
    };

    // Now actually apply (first was just --stat for preview)
    let mut child = match tokio::process::Command::new("git")
        .args(["apply", &strip_arg, "-"])
        .current_dir(cwd)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(_) => return ToolResult::fail("git not available"),
    };

    if let Some(mut stdin) = child.stdin.take() {
        use tokio::io::AsyncWriteExt;
        let _ = stdin.write_all(patch.as_bytes()).await;
        let _ = stdin.shutdown().await;
    }

    let apply_output = match child.wait_with_output().await {
        Ok(o) => o,
        Err(e) => return ToolResult::fail(format!("git apply failed: {e}")),
    };

    if apply_output.status.success() {
        let stat = String::from_utf8_lossy(&output.stdout).to_string();
        ToolResult::ok(format!("Patch applied successfully via git apply.\n{stat}"))
    } else {
        let stderr = String::from_utf8_lossy(&apply_output.stderr).to_string();
        ToolResult::fail(format!("git apply failed: {stderr}"))
    }
}

/// Simple manual patch application for when git is not available.
fn apply_patch_manually(patch: &str, cwd: &Path, strip: usize) -> ToolResult {
    let mut files_modified = Vec::new();
    let mut current_file: Option<String> = None;
    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current_hunk: Option<HunkBuilder> = None;

    for line in patch.lines() {
        if line.starts_with("+++ ") {
            // Save previous file's hunks
            if let Some(file) = current_file.take() {
                if let Err(e) = apply_hunks(cwd, &file, &hunks) {
                    return ToolResult::fail(format!("Failed to patch {file}: {e}"));
                }
                files_modified.push(file);
                hunks.clear();
            }

            // Parse target file path
            let path = line.strip_prefix("+++ ").unwrap_or("");
            let path = strip_path(path, strip);
            if path == "/dev/null" {
                continue;
            }
            current_file = Some(path);

            // Flush any pending hunk
            if let Some(hb) = current_hunk.take() {
                hunks.push(hb.build());
            }
        } else if line.starts_with("@@ ") {
            // Flush previous hunk
            if let Some(hb) = current_hunk.take() {
                hunks.push(hb.build());
            }
            // Parse hunk header: @@ -old_start,old_count +new_start,new_count @@
            if let Some(hb) = parse_hunk_header(line) {
                current_hunk = Some(hb);
            }
        } else if let Some(ref mut hb) = current_hunk {
            hb.lines.push(line.to_string());
        }
    }

    // Flush last hunk and file
    if let Some(hb) = current_hunk.take() {
        hunks.push(hb.build());
    }
    if let Some(file) = current_file.take() {
        if let Err(e) = apply_hunks(cwd, &file, &hunks) {
            return ToolResult::fail(format!("Failed to patch {file}: {e}"));
        }
        files_modified.push(file);
    }

    if files_modified.is_empty() {
        return ToolResult::fail("No files were modified by the patch");
    }

    ToolResult::ok(format!(
        "Patch applied manually to {} file(s): {}",
        files_modified.len(),
        files_modified.join(", ")
    ))
}

fn strip_path(path: &str, strip: usize) -> String {
    let parts: Vec<&str> = path.splitn(strip + 1, '/').collect();
    if parts.len() > strip {
        parts[strip..].join("/")
    } else {
        path.to_string()
    }
}

struct HunkBuilder {
    old_start: usize,
    lines: Vec<String>,
}

struct Hunk {
    old_start: usize,
    lines: Vec<String>,
}

impl HunkBuilder {
    fn build(self) -> Hunk {
        Hunk {
            old_start: self.old_start,
            lines: self.lines,
        }
    }
}

fn parse_hunk_header(line: &str) -> Option<HunkBuilder> {
    // @@ -old_start,old_count +new_start,new_count @@
    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 3 {
        return None;
    }
    let old_range = parts[1].strip_prefix('-')?;
    let old_start: usize = old_range.split(',').next()?.parse().ok()?;

    Some(HunkBuilder {
        old_start,
        lines: Vec::new(),
    })
}

fn apply_hunks(cwd: &Path, file: &str, hunks: &[Hunk]) -> Result<(), String> {
    let path = cwd.join(file);

    let original = if path.exists() {
        std::fs::read_to_string(&path).map_err(|e| format!("Cannot read {file}: {e}"))?
    } else {
        String::new()
    };

    let mut file_lines: Vec<String> = original.lines().map(String::from).collect();
    let mut offset: i64 = 0;

    for hunk in hunks {
        let start = ((hunk.old_start as i64 - 1) + offset).max(0) as usize;
        let mut pos = start;
        let mut added = 0i64;
        let mut removed = 0i64;

        for line in &hunk.lines {
            if let Some(content) = line.strip_prefix('+') {
                file_lines.insert(pos, content.to_string());
                pos += 1;
                added += 1;
            } else if let Some(_content) = line.strip_prefix('-') {
                if pos < file_lines.len() {
                    file_lines.remove(pos);
                    removed += 1;
                }
            } else if line.starts_with(' ') || line.is_empty() {
                // Context line — just advance
                pos += 1;
            }
        }

        offset += added - removed;
    }

    // Write result
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("Cannot create directory: {e}"))?;
    }

    let content = file_lines.join("\n");
    // Preserve trailing newline if original had one
    let content = if original.ends_with('\n') && !content.ends_with('\n') {
        content + "\n"
    } else {
        content
    };

    std::fs::write(&path, content).map_err(|e| format!("Cannot write {file}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_strip_path() {
        assert_eq!(strip_path("a/b/c.rs", 1), "b/c.rs");
        assert_eq!(strip_path("a/b/c.rs", 2), "c.rs");
        assert_eq!(strip_path("c.rs", 0), "c.rs");
    }

    #[test]
    fn test_parse_hunk_header() {
        let hb = parse_hunk_header("@@ -10,5 +10,7 @@ fn main()").unwrap();
        assert_eq!(hb.old_start, 10);
    }

    #[tokio::test]
    async fn test_patch_missing() {
        let tool = PatchTool;
        let ctx = ToolContext::new("/tmp");
        let result = tool.execute(HashMap::new(), &ctx).await;
        assert!(!result.success);
    }

    #[test]
    fn test_apply_hunks_simple() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("test.txt"), "line1\nline2\nline3\n").unwrap();

        let hunk = Hunk {
            old_start: 2,
            lines: vec!["-line2".to_string(), "+line2_modified".to_string()],
        };

        apply_hunks(tmp.path(), "test.txt", &[hunk]).unwrap();
        let result = std::fs::read_to_string(tmp.path().join("test.txt")).unwrap();
        assert!(result.contains("line2_modified"));
        assert!(!result.contains("\nline2\n"));
    }

    // -----------------------------------------------------------------------
    // Property-based tests for patch hunk parsing (fuzzing #71)
    // -----------------------------------------------------------------------

    mod proptest_patch {
        use super::*;
        use proptest::prelude::*;

        proptest! {
            /// parse_hunk_header must never panic on arbitrary input.
            #[test]
            fn fuzz_hunk_header_no_panic(line in "\\PC*") {
                let _ = parse_hunk_header(&line);
            }

            /// strip_path must never panic on arbitrary input.
            #[test]
            fn fuzz_strip_path_no_panic(
                path in "\\PC{0,200}",
                strip in 0usize..10
            ) {
                let _ = strip_path(&path, strip);
            }

            /// Valid hunk headers must be parsed correctly.
            #[test]
            fn valid_hunk_header_parsed(
                old_start in 1usize..10000,
                old_count in 0usize..1000,
                new_start in 1usize..10000,
                new_count in 0usize..1000,
            ) {
                let line = format!("@@ -{old_start},{old_count} +{new_start},{new_count} @@");
                let result = parse_hunk_header(&line);
                prop_assert!(result.is_some(), "Failed to parse: {}", line);
                let hb = result.unwrap();
                prop_assert_eq!(hb.old_start, old_start);
            }

            /// apply_patch_manually must not panic on arbitrary patch content.
            #[test]
            fn fuzz_apply_patch_manually_no_panic(
                patch in "\\PC{0,1000}",
                strip in 0usize..5,
            ) {
                let tmp = TempDir::new().unwrap();
                // Create a dummy file so patch application has something to work with
                std::fs::write(tmp.path().join("test.txt"), "line1\nline2\nline3\n").unwrap();
                // Should not panic — errors are returned as ToolResult
                let _ = apply_patch_manually(&patch, tmp.path(), strip);
            }
        }
    }
}
