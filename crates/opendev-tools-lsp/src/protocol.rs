//! LSP protocol types for OpenDev's internal representation.
//!
//! Provides unified symbol information, workspace edits, and text edits.
//! URI conversion uses the `url` crate for file path handling.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

use url::Url;

/// A unified symbol representation combining information from various LSP responses.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UnifiedSymbolInfo {
    /// Symbol name.
    pub name: String,
    /// Symbol kind (function, class, variable, etc.).
    pub kind: SymbolKind,
    /// File path where the symbol is defined.
    pub file_path: PathBuf,
    /// Range of the symbol in the file.
    pub range: SourceRange,
    /// Range of the symbol name itself (for rename operations).
    pub selection_range: Option<SourceRange>,
    /// Container name (e.g., class for a method).
    pub container_name: Option<String>,
    /// Detail string (e.g., type signature).
    pub detail: Option<String>,
}

/// Symbol kind enumeration matching LSP SymbolKind values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SymbolKind {
    File,
    Module,
    Namespace,
    Package,
    Class,
    Method,
    Property,
    Field,
    Constructor,
    Enum,
    Interface,
    Function,
    Variable,
    Constant,
    String,
    Number,
    Boolean,
    Array,
    Object,
    Key,
    Null,
    EnumMember,
    Struct,
    Event,
    Operator,
    TypeParameter,
    Unknown,
}

impl SymbolKind {
    /// Convert from LSP SymbolKind numeric value.
    pub fn from_lsp(value: i32) -> Self {
        match value {
            1 => Self::File,
            2 => Self::Module,
            3 => Self::Namespace,
            4 => Self::Package,
            5 => Self::Class,
            6 => Self::Method,
            7 => Self::Property,
            8 => Self::Field,
            9 => Self::Constructor,
            10 => Self::Enum,
            11 => Self::Interface,
            12 => Self::Function,
            13 => Self::Variable,
            14 => Self::Constant,
            15 => Self::String,
            16 => Self::Number,
            17 => Self::Boolean,
            18 => Self::Array,
            19 => Self::Object,
            20 => Self::Key,
            21 => Self::Null,
            22 => Self::EnumMember,
            23 => Self::Struct,
            24 => Self::Event,
            25 => Self::Operator,
            26 => Self::TypeParameter,
            _ => Self::Unknown,
        }
    }

    /// Display name for the symbol kind.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::File => "file",
            Self::Module => "module",
            Self::Namespace => "namespace",
            Self::Package => "package",
            Self::Class => "class",
            Self::Method => "method",
            Self::Property => "property",
            Self::Field => "field",
            Self::Constructor => "constructor",
            Self::Enum => "enum",
            Self::Interface => "interface",
            Self::Function => "function",
            Self::Variable => "variable",
            Self::Constant => "constant",
            Self::String => "string",
            Self::Number => "number",
            Self::Boolean => "boolean",
            Self::Array => "array",
            Self::Object => "object",
            Self::Key => "key",
            Self::Null => "null",
            Self::EnumMember => "enum_member",
            Self::Struct => "struct",
            Self::Event => "event",
            Self::Operator => "operator",
            Self::TypeParameter => "type_parameter",
            Self::Unknown => "unknown",
        }
    }
}

/// A position in a text document (0-indexed line and character).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: u32,
    pub character: u32,
}

impl Position {
    pub fn new(line: u32, character: u32) -> Self {
        Self { line, character }
    }
}

/// A range in a text document.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceRange {
    pub start: Position,
    pub end: Position,
}

impl SourceRange {
    pub fn new(start: Position, end: Position) -> Self {
        Self { start, end }
    }

    /// Check if this range contains a position.
    pub fn contains(&self, pos: Position) -> bool {
        if pos.line < self.start.line || pos.line > self.end.line {
            return false;
        }
        if pos.line == self.start.line && pos.character < self.start.character {
            return false;
        }
        if pos.line == self.end.line && pos.character > self.end.character {
            return false;
        }
        true
    }
}

/// A location in a source file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceLocation {
    pub file_path: PathBuf,
    pub range: SourceRange,
}

impl SourceLocation {
    pub fn new(file_path: PathBuf, range: SourceRange) -> Self {
        Self { file_path, range }
    }
}

/// A text edit to apply to a file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextEdit {
    pub range: SourceRange,
    pub new_text: String,
}

impl TextEdit {
    pub fn new(range: SourceRange, new_text: impl Into<String>) -> Self {
        Self {
            range,
            new_text: new_text.into(),
        }
    }
}

/// A workspace edit containing changes across multiple files.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceEdit {
    /// Map from file path to list of text edits.
    pub changes: HashMap<PathBuf, Vec<TextEdit>>,
}

impl WorkspaceEdit {
    pub fn new() -> Self {
        Self::default()
    }

    /// Parse from a JSON workspace edit response.
    pub fn from_json(value: &serde_json::Value) -> Self {
        let mut changes: HashMap<PathBuf, Vec<TextEdit>> = HashMap::new();

        if let Some(lsp_changes) = value.get("changes").and_then(|v| v.as_object()) {
            for (uri_str, edits) in lsp_changes {
                if let Some(path) = uri_string_to_path(uri_str)
                    && let Some(edit_arr) = edits.as_array()
                {
                    let text_edits: Vec<TextEdit> = edit_arr
                        .iter()
                        .filter_map(|e| {
                            let range = parse_range_json(e.get("range")?)?;
                            let new_text = e.get("newText")?.as_str()?.to_string();
                            Some(TextEdit { range, new_text })
                        })
                        .collect();
                    changes.insert(path, text_edits);
                }
            }
        }

        Self { changes }
    }

    /// Total number of edits across all files.
    pub fn edit_count(&self) -> usize {
        self.changes.values().map(|v| v.len()).sum()
    }

    /// Number of files affected.
    pub fn file_count(&self) -> usize {
        self.changes.len()
    }
}

/// Convert a file URI string to a filesystem path.
pub fn uri_string_to_path(uri: &str) -> Option<PathBuf> {
    let url = Url::parse(uri).ok()?;
    if url.scheme() == "file" {
        url.to_file_path().ok()
    } else {
        None
    }
}

/// Convert a filesystem path to a file URI string.
pub fn path_to_uri_string(path: &Path) -> Option<String> {
    Url::from_file_path(path).ok().map(|u| u.to_string())
}

/// Parse a JSON range object into SourceRange.
pub fn parse_range_json(value: &serde_json::Value) -> Option<SourceRange> {
    let start = value.get("start")?;
    let end = value.get("end")?;
    Some(SourceRange::new(
        Position::new(
            start.get("line")?.as_u64()? as u32,
            start.get("character")?.as_u64()? as u32,
        ),
        Position::new(
            end.get("line")?.as_u64()? as u32,
            end.get("character")?.as_u64()? as u32,
        ),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_range_contains() {
        let range = SourceRange::new(Position::new(5, 0), Position::new(10, 20));
        assert!(range.contains(Position::new(7, 10)));
        assert!(range.contains(Position::new(5, 0)));
        assert!(range.contains(Position::new(10, 20)));
        assert!(!range.contains(Position::new(4, 0)));
        assert!(!range.contains(Position::new(11, 0)));
    }

    #[test]
    fn test_range_does_not_contain_before_start_char() {
        let range = SourceRange::new(Position::new(5, 3), Position::new(10, 20));
        assert!(!range.contains(Position::new(5, 2)));
        assert!(range.contains(Position::new(5, 3)));
    }

    #[test]
    fn test_symbol_kind_from_lsp() {
        assert_eq!(SymbolKind::from_lsp(12), SymbolKind::Function);
        assert_eq!(SymbolKind::from_lsp(5), SymbolKind::Class);
        assert_eq!(SymbolKind::from_lsp(999), SymbolKind::Unknown);
    }

    #[test]
    fn test_symbol_kind_display() {
        assert_eq!(SymbolKind::Function.display_name(), "function");
        assert_eq!(SymbolKind::Class.display_name(), "class");
    }

    #[test]
    fn test_workspace_edit_counts() {
        let mut edit = WorkspaceEdit::new();
        edit.changes.insert(
            PathBuf::from("/a.rs"),
            vec![
                TextEdit::new(
                    SourceRange::new(Position::new(0, 0), Position::new(0, 5)),
                    "hello",
                ),
                TextEdit::new(
                    SourceRange::new(Position::new(1, 0), Position::new(1, 3)),
                    "world",
                ),
            ],
        );
        edit.changes.insert(
            PathBuf::from("/b.rs"),
            vec![TextEdit::new(
                SourceRange::new(Position::new(0, 0), Position::new(0, 1)),
                "x",
            )],
        );
        assert_eq!(edit.file_count(), 2);
        assert_eq!(edit.edit_count(), 3);
    }

    #[cfg(unix)]
    #[test]
    fn test_uri_path_roundtrip() {
        let path = PathBuf::from("/tmp/test.rs");
        let uri = path_to_uri_string(&path).unwrap();
        let back = uri_string_to_path(&uri).unwrap();
        assert_eq!(back, path);
    }

    #[test]
    fn test_uri_string_to_path_non_file() {
        assert!(uri_string_to_path("http://example.com").is_none());
    }

    #[test]
    fn test_text_edit_serde() {
        let edit = TextEdit::new(
            SourceRange::new(Position::new(1, 2), Position::new(3, 4)),
            "replacement",
        );
        let json = serde_json::to_string(&edit).unwrap();
        let back: TextEdit = serde_json::from_str(&json).unwrap();
        assert_eq!(back.new_text, "replacement");
        assert_eq!(back.range.start.line, 1);
    }

    #[test]
    fn test_unified_symbol_info_serde() {
        let sym = UnifiedSymbolInfo {
            name: "my_func".to_string(),
            kind: SymbolKind::Function,
            file_path: PathBuf::from("/src/main.rs"),
            range: SourceRange::new(Position::new(10, 0), Position::new(20, 1)),
            selection_range: Some(SourceRange::new(
                Position::new(10, 4),
                Position::new(10, 11),
            )),
            container_name: Some("MyStruct".to_string()),
            detail: None,
        };
        let json = serde_json::to_string(&sym).unwrap();
        let back: UnifiedSymbolInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(back.name, "my_func");
        assert_eq!(back.kind, SymbolKind::Function);
    }

    #[test]
    fn test_parse_range_json() {
        let json = serde_json::json!({
            "start": { "line": 10, "character": 5 },
            "end": { "line": 20, "character": 15 }
        });
        let range = parse_range_json(&json).unwrap();
        assert_eq!(range.start.line, 10);
        assert_eq!(range.end.character, 15);
    }

    #[cfg(unix)]
    #[test]
    fn test_workspace_edit_from_json() {
        let json = serde_json::json!({
            "changes": {
                "file:///src/main.rs": [
                    {
                        "range": {
                            "start": { "line": 0, "character": 0 },
                            "end": { "line": 0, "character": 3 }
                        },
                        "newText": "let"
                    }
                ]
            }
        });
        let edit = WorkspaceEdit::from_json(&json);
        assert_eq!(edit.file_count(), 1);
        assert_eq!(edit.edit_count(), 1);
    }
}
