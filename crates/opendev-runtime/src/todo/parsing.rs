use super::TodoStatus;

/// Map status alias strings to `TodoStatus`.
///
/// Accepts: `pending`, `todo`, `in_progress`, `doing`, `in-progress`,
/// `completed`, `done`, `complete`.
pub fn parse_status(s: &str) -> Option<TodoStatus> {
    match s.to_lowercase().trim() {
        "pending" | "todo" => Some(TodoStatus::Pending),
        "in_progress" | "doing" | "in-progress" | "in progress" => Some(TodoStatus::InProgress),
        "completed" | "done" | "complete" => Some(TodoStatus::Completed),
        _ => None,
    }
}

/// Strip basic markdown formatting from text (bold, italic, code).
pub fn strip_markdown(text: &str) -> String {
    text.replace("**", "")
        .replace("__", "")
        .replace('*', "")
        .replace('_', " ")
        .replace('`', "")
        .replace("~~", "")
}

/// Parse plan markdown content and extract numbered implementation steps.
///
/// First looks for a section header like `## Implementation Steps` or `## Steps`,
/// then extracts numbered list items from that section. If no such section exists,
/// falls back to extracting all numbered items from the entire document.
pub fn parse_plan_steps(plan_content: &str) -> Vec<String> {
    // First try: section-aware extraction
    let mut steps = Vec::new();
    let mut in_steps_section = false;

    for line in plan_content.lines() {
        let trimmed = line.trim();

        // Detect steps section header
        if trimmed.starts_with("## Implementation Steps")
            || trimmed.starts_with("## Steps")
            || trimmed.starts_with("## implementation steps")
        {
            in_steps_section = true;
            continue;
        }

        // End of section on next header
        if in_steps_section && trimmed.starts_with("## ") {
            break;
        }

        // Extract numbered items
        if in_steps_section
            && let Some(text) = extract_numbered_step(trimmed)
            && !text.is_empty()
        {
            steps.push(text);
        }
    }

    // Fallback: if no section header found, extract all numbered items
    if steps.is_empty() {
        for line in plan_content.lines() {
            let trimmed = line.trim();
            // Skip markdown headers themselves
            if trimmed.starts_with('#') {
                continue;
            }
            if let Some(text) = extract_numbered_step(trimmed)
                && !text.is_empty()
            {
                steps.push(text);
            }
        }
    }

    steps
}

/// Extract the text from a numbered list item.
///
/// Handles formats like:
/// - `1. Step text`
/// - `1) Step text`
/// - `1 - Step text`
fn extract_numbered_step(line: &str) -> Option<String> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Check if line starts with a digit
    let mut chars = line.chars();
    let first = chars.next()?;
    if !first.is_ascii_digit() {
        return None;
    }

    // Skip remaining digits
    let rest: String = chars.collect();
    let rest = rest.trim_start_matches(|c: char| c.is_ascii_digit());

    // Check for separator (. or ) or -)
    let rest = if let Some(s) = rest.strip_prefix(". ") {
        s
    } else if let Some(s) = rest.strip_prefix(") ") {
        s
    } else if let Some(s) = rest.strip_prefix(" - ") {
        s
    } else {
        return None;
    };

    let text = rest.trim();
    if text.is_empty() {
        None
    } else {
        // Strip markdown bold/emphasis markers for cleaner titles
        let text = text.replace("**", "").replace("__", "");
        Some(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plan_steps_basic() {
        let plan = "\
# My Plan

---BEGIN PLAN---

## Summary
Do some stuff.

## Implementation Steps

1. Set up the project structure
2. Add the config parser
3. Implement core logic
4. Write tests
5. Update documentation

## Verification

1. Run tests
2. Check lint

---END PLAN---
";
        let steps = parse_plan_steps(plan);
        assert_eq!(steps.len(), 5);
        assert_eq!(steps[0], "Set up the project structure");
        assert_eq!(steps[4], "Update documentation");
    }

    #[test]
    fn test_parse_plan_steps_with_bold() {
        let plan = "\
## Implementation Steps

1. **Set up** the project
2. **Add** config handling
";
        let steps = parse_plan_steps(plan);
        assert_eq!(steps.len(), 2);
        assert_eq!(steps[0], "Set up the project");
        assert_eq!(steps[1], "Add config handling");
    }

    #[test]
    fn test_parse_plan_steps_stops_at_next_section() {
        let plan = "\
## Steps

1. First step
2. Second step

## Verification

1. Run tests
";
        let steps = parse_plan_steps(plan);
        assert_eq!(steps.len(), 2);
    }

    #[test]
    fn test_parse_plan_steps_empty() {
        let plan = "# Plan\n\nNo steps section here.\n";
        let steps = parse_plan_steps(plan);
        assert!(steps.is_empty());
    }

    #[test]
    fn test_extract_numbered_step_formats() {
        assert_eq!(
            extract_numbered_step("1. Do something"),
            Some("Do something".into())
        );
        assert_eq!(
            extract_numbered_step("12. Multi digit"),
            Some("Multi digit".into())
        );
        assert_eq!(
            extract_numbered_step("1) Paren format"),
            Some("Paren format".into())
        );
        assert_eq!(extract_numbered_step("Not a step"), None);
        assert_eq!(extract_numbered_step(""), None);
        assert_eq!(extract_numbered_step("  "), None);
    }

    #[test]
    fn test_parse_status() {
        assert_eq!(parse_status("pending"), Some(TodoStatus::Pending));
        assert_eq!(parse_status("todo"), Some(TodoStatus::Pending));
        assert_eq!(parse_status("in_progress"), Some(TodoStatus::InProgress));
        assert_eq!(parse_status("doing"), Some(TodoStatus::InProgress));
        assert_eq!(parse_status("in-progress"), Some(TodoStatus::InProgress));
        assert_eq!(parse_status("completed"), Some(TodoStatus::Completed));
        assert_eq!(parse_status("done"), Some(TodoStatus::Completed));
        assert_eq!(parse_status("complete"), Some(TodoStatus::Completed));
        assert_eq!(parse_status("unknown"), None);
    }

    #[test]
    fn test_strip_markdown() {
        assert_eq!(strip_markdown("**bold** text"), "bold text");
        assert_eq!(strip_markdown("`code`"), "code");
        assert_eq!(strip_markdown("~~struck~~"), "struck");
    }
}
