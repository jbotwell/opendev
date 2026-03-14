//! Formatter factory — dispatches tool names to the appropriate formatter.

use super::base::{FormattedOutput, ToolFormatter};
use super::bash_formatter::BashFormatter;
use super::directory_formatter::DirectoryFormatter;
use super::file_formatter::FileFormatter;
use super::generic_formatter::GenericFormatter;
use super::todo_formatter::TodoFormatter;
use super::tool_registry::{ResultFormat, lookup_tool};

/// Factory that selects the right formatter for a given tool name.
pub struct FormatterFactory;

impl FormatterFactory {
    /// Get the appropriate formatter for a tool name and format its output.
    pub fn format<'a>(tool_name: &str, output: &str) -> FormattedOutput<'a> {
        let formatter = Self::formatter_for(tool_name);
        formatter.format(tool_name, output)
    }

    /// Return the formatter that handles a given tool name.
    fn formatter_for(tool_name: &str) -> Box<dyn ToolFormatter> {
        match lookup_tool(tool_name).result_format {
            ResultFormat::Bash => Box::new(BashFormatter),
            ResultFormat::File => Box::new(FileFormatter),
            ResultFormat::Directory => Box::new(DirectoryFormatter),
            ResultFormat::Generic => Box::new(GenericFormatter),
            ResultFormat::Todo => Box::new(TodoFormatter),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dispatch_bash() {
        let result = FormatterFactory::format("Bash", "$ echo hi\nhi\nExit code: 0");
        let header_text: String = result
            .header
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(header_text.contains("echo hi"));
    }

    #[test]
    fn test_dispatch_read() {
        let result = FormatterFactory::format("Read", "line 1\nline 2");
        let header_text: String = result
            .header
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(header_text.contains("2 lines"));
    }

    #[test]
    fn test_dispatch_write() {
        let result = FormatterFactory::format("Write", "+new");
        let header_text: String = result
            .header
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(header_text.contains("Written"));
    }

    #[test]
    fn test_dispatch_edit() {
        let result = FormatterFactory::format("Edit", "-old\n+new");
        let header_text: String = result
            .header
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(header_text.contains("Edited"));
    }

    #[test]
    fn test_dispatch_glob() {
        let result = FormatterFactory::format("Glob", "a.rs\nb.rs");
        let header_text: String = result
            .header
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(header_text.contains("2 matching files"));
    }

    #[test]
    fn test_dispatch_grep() {
        let result = FormatterFactory::format("Grep", "file:1:match");
        let header_text: String = result
            .header
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(header_text.contains("1 matching results"));
    }

    #[test]
    fn test_dispatch_unknown_falls_to_generic() {
        let result = FormatterFactory::format("unknown_tool", "some output");
        let header_text: String = result
            .header
            .spans
            .iter()
            .map(|s| s.content.as_ref())
            .collect();
        assert!(header_text.contains("unknown_tool"));
    }
}
