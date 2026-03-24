//! LSP client wrapper managing multiple language server instances.
//!
//! `LspWrapper` is the main entry point. It maps file extensions to language
//! servers, lazily starts them on demand, and routes LSP requests to the
//! appropriate server.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use tracing::{debug, info, warn};

use crate::cache::SymbolCache;
use crate::error::LspError;
use crate::handler::LspHandler;
use crate::protocol::{self, SourceLocation, UnifiedSymbolInfo, WorkspaceEdit};
use crate::servers::{ServerConfig, default_server_configs};
use crate::utils::PathUtils;

/// LSP client wrapper managing language server lifecycles.
pub struct LspWrapper {
    /// Map from file extension to server config.
    extension_map: HashMap<String, ServerConfig>,
    /// Active server handlers keyed by (language_id, workspace_root).
    handlers: HashMap<(String, PathBuf), LspHandler>,
    /// Symbol cache.
    cache: SymbolCache,
}

impl LspWrapper {
    /// Create a new LSP wrapper with default server configurations.
    pub fn new(cache_dir: Option<PathBuf>) -> Self {
        let mut extension_map = HashMap::new();
        for config in default_server_configs() {
            for ext in &config.extensions {
                extension_map.insert(ext.clone(), config.clone());
            }
        }

        Self {
            extension_map,
            handlers: HashMap::new(),
            cache: SymbolCache::new(cache_dir, None),
        }
    }

    /// Register a custom server configuration.
    pub fn register_server(&mut self, config: ServerConfig) {
        for ext in &config.extensions {
            self.extension_map.insert(ext.clone(), config.clone());
        }
    }

    /// Get a running handler for a file, starting the server if needed.
    async fn ensure_handler(
        &mut self,
        file_path: &Path,
        workspace_root: &Path,
    ) -> Result<&LspHandler, LspError> {
        let ext = PathUtils::extension(file_path)
            .ok_or_else(|| LspError::NoServer("no file extension".to_string()))?;

        let config = self
            .extension_map
            .get(&ext)
            .ok_or_else(|| LspError::NoServer(ext.clone()))?
            .clone();

        let key = (config.language_id.clone(), workspace_root.to_path_buf());

        if !self.handlers.contains_key(&key) || !self.handlers[&key].is_ready() {
            info!(
                "Starting LSP server {} for {} in {}",
                config.command,
                config.language_id,
                workspace_root.display()
            );
            let mut handler = LspHandler::new(config, workspace_root.to_path_buf());
            handler.start().await?;
            self.handlers.insert(key.clone(), handler);
        }

        Ok(self.handlers.get(&key).unwrap())
    }

    /// Find symbols matching a query pattern in the workspace.
    pub async fn find_symbols(
        &mut self,
        query: &str,
        file_path: &Path,
        workspace_root: &Path,
    ) -> Result<Vec<UnifiedSymbolInfo>, LspError> {
        // Check cache first
        if let Some(cached) = self.cache.get(workspace_root, query) {
            return Ok(cached);
        }

        let handler = self.ensure_handler(file_path, workspace_root).await?;

        let params = serde_json::json!({
            "query": query
        });

        let result = handler.send_request("workspace/symbol", params).await?;

        let symbols = parse_symbol_response(&result);

        // Cache the result
        self.cache.put(workspace_root, query, symbols.clone());

        Ok(symbols)
    }

    /// Find all references to a symbol at a position.
    pub async fn find_references(
        &mut self,
        file_path: &Path,
        line: u32,
        character: u32,
        workspace_root: &Path,
    ) -> Result<Vec<SourceLocation>, LspError> {
        let handler = self.ensure_handler(file_path, workspace_root).await?;
        let uri = protocol::path_to_uri_string(file_path)
            .ok_or_else(|| LspError::FileNotFound(file_path.display().to_string()))?;

        // Notify the server about the document
        notify_did_open(handler, file_path, &uri).await?;

        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "context": { "includeDeclaration": true }
        });

        let result = handler
            .send_request("textDocument/references", params)
            .await?;

        Ok(parse_locations(&result))
    }

    /// Get the definition location of a symbol at a position.
    pub async fn goto_definition(
        &mut self,
        file_path: &Path,
        line: u32,
        character: u32,
        workspace_root: &Path,
    ) -> Result<Vec<SourceLocation>, LspError> {
        let handler = self.ensure_handler(file_path, workspace_root).await?;
        let uri = protocol::path_to_uri_string(file_path)
            .ok_or_else(|| LspError::FileNotFound(file_path.display().to_string()))?;

        notify_did_open(handler, file_path, &uri).await?;

        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let result = handler
            .send_request("textDocument/definition", params)
            .await?;

        // Can be a single Location or an array
        let locations = if result.is_array() {
            parse_locations(&result)
        } else if result.is_object() {
            parse_locations(&serde_json::json!([result]))
        } else {
            vec![]
        };

        Ok(locations)
    }

    /// Rename a symbol at a position.
    pub async fn rename_symbol(
        &mut self,
        file_path: &Path,
        line: u32,
        character: u32,
        new_name: &str,
        workspace_root: &Path,
    ) -> Result<WorkspaceEdit, LspError> {
        let handler = self.ensure_handler(file_path, workspace_root).await?;
        let uri = protocol::path_to_uri_string(file_path)
            .ok_or_else(|| LspError::FileNotFound(file_path.display().to_string()))?;

        notify_did_open(handler, file_path, &uri).await?;

        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character },
            "newName": new_name
        });

        let result = handler.send_request("textDocument/rename", params).await?;

        let edit = WorkspaceEdit::from_json(&result);

        // Invalidate cache since files changed
        self.cache.invalidate_workspace(workspace_root);

        Ok(edit)
    }

    /// Get document symbols for a file.
    pub async fn document_symbols(
        &mut self,
        file_path: &Path,
        workspace_root: &Path,
    ) -> Result<Vec<UnifiedSymbolInfo>, LspError> {
        let handler = self.ensure_handler(file_path, workspace_root).await?;
        let uri = protocol::path_to_uri_string(file_path)
            .ok_or_else(|| LspError::FileNotFound(file_path.display().to_string()))?;

        notify_did_open(handler, file_path, &uri).await?;

        let params = serde_json::json!({
            "textDocument": { "uri": uri }
        });

        let result = handler
            .send_request("textDocument/documentSymbol", params)
            .await?;

        let symbols = parse_document_symbols(&result, file_path);
        Ok(symbols)
    }

    /// Get hover information for a symbol at a position.
    pub async fn hover(
        &mut self,
        file_path: &Path,
        line: u32,
        character: u32,
        workspace_root: &Path,
    ) -> Result<Option<String>, LspError> {
        let handler = self.ensure_handler(file_path, workspace_root).await?;
        let uri = protocol::path_to_uri_string(file_path)
            .ok_or_else(|| LspError::FileNotFound(file_path.display().to_string()))?;

        notify_did_open(handler, file_path, &uri).await?;

        let params = serde_json::json!({
            "textDocument": { "uri": uri },
            "position": { "line": line, "character": character }
        });

        let result = handler.send_request("textDocument/hover", params).await?;

        // Parse hover result — extract contents from the response
        if result.is_null() {
            return Ok(None);
        }

        let contents = result.get("contents");
        let text = match contents {
            Some(serde_json::Value::String(s)) => Some(s.clone()),
            Some(serde_json::Value::Object(obj)) => {
                // MarkedString or MarkupContent
                obj.get("value")
                    .and_then(|v| v.as_str())
                    .map(|s| s.to_string())
            }
            Some(serde_json::Value::Array(arr)) => {
                // Array of MarkedString
                let parts: Vec<String> = arr
                    .iter()
                    .filter_map(|item| match item {
                        serde_json::Value::String(s) => Some(s.clone()),
                        serde_json::Value::Object(obj) => obj
                            .get("value")
                            .and_then(|v| v.as_str())
                            .map(|s| s.to_string()),
                        _ => None,
                    })
                    .collect();
                if parts.is_empty() {
                    None
                } else {
                    Some(parts.join("\n\n"))
                }
            }
            _ => None,
        };

        Ok(text)
    }

    /// Shutdown all running language servers.
    pub async fn shutdown_all(&mut self) {
        for (key, handler) in self.handlers.iter_mut() {
            debug!("Shutting down LSP server: {:?}", key);
            if let Err(e) = handler.shutdown().await {
                warn!("Error shutting down {:?}: {}", key, e);
            }
        }
        self.handlers.clear();
    }

    /// Check if a server is available for a file extension.
    pub fn has_server_for(&self, file_path: &Path) -> bool {
        PathUtils::extension(file_path)
            .map(|ext| self.extension_map.contains_key(&ext))
            .unwrap_or(false)
    }

    /// Get the symbol cache (for testing/inspection).
    pub fn cache(&mut self) -> &mut SymbolCache {
        &mut self.cache
    }
}

/// Send textDocument/didOpen notification for a file.
async fn notify_did_open(
    handler: &LspHandler,
    file_path: &Path,
    uri: &str,
) -> Result<(), LspError> {
    let content = std::fs::read_to_string(file_path)
        .map_err(|e| LspError::FileNotFound(format!("{}: {}", file_path.display(), e)))?;

    let language_id = handler.config().language_id.clone();

    handler
        .send_notification(
            "textDocument/didOpen",
            serde_json::json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": language_id,
                    "version": 1,
                    "text": content
                }
            }),
        )
        .await
}

/// Parse workspace/symbol response into UnifiedSymbolInfo list.
fn parse_symbol_response(result: &serde_json::Value) -> Vec<UnifiedSymbolInfo> {
    let arr = match result.as_array() {
        Some(a) => a,
        None => return vec![],
    };

    arr.iter().filter_map(parse_symbol_info).collect()
}

/// Parse a single SymbolInformation JSON into UnifiedSymbolInfo.
fn parse_symbol_info(value: &serde_json::Value) -> Option<UnifiedSymbolInfo> {
    let name = value.get("name")?.as_str()?.to_string();
    let kind_num = value.get("kind")?.as_i64()? as i32;
    let kind = protocol::SymbolKind::from_lsp(kind_num);

    let location = value.get("location")?;
    let uri_str = location.get("uri")?.as_str()?;
    let file_path = protocol::uri_string_to_path(uri_str)?;
    let range = protocol::parse_range_json(location.get("range")?)?;

    let container_name = value
        .get("containerName")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(UnifiedSymbolInfo {
        name,
        kind,
        file_path,
        range,
        selection_range: None,
        container_name,
        detail: None,
    })
}

/// Parse document symbols (can be DocumentSymbol or SymbolInformation).
fn parse_document_symbols(result: &serde_json::Value, file_path: &Path) -> Vec<UnifiedSymbolInfo> {
    let arr = match result.as_array() {
        Some(a) => a,
        None => return vec![],
    };

    let mut symbols = Vec::new();
    for item in arr {
        // Try DocumentSymbol format first (has range + selectionRange)
        if item.get("range").is_some() && item.get("selectionRange").is_some() {
            if let Some(sym) = parse_document_symbol(item, file_path, None) {
                flatten_document_symbol(&sym, item, file_path, &mut symbols);
                symbols.push(sym);
            }
        } else if let Some(sym) = parse_symbol_info(item) {
            symbols.push(sym);
        }
    }
    symbols
}

fn parse_document_symbol(
    value: &serde_json::Value,
    file_path: &Path,
    container: Option<&str>,
) -> Option<UnifiedSymbolInfo> {
    let name = value.get("name")?.as_str()?.to_string();
    let kind_num = value.get("kind")?.as_i64()? as i32;
    let kind = protocol::SymbolKind::from_lsp(kind_num);
    let range = protocol::parse_range_json(value.get("range")?)?;
    let selection_range = value
        .get("selectionRange")
        .and_then(protocol::parse_range_json);
    let detail = value
        .get("detail")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(UnifiedSymbolInfo {
        name,
        kind,
        file_path: file_path.to_path_buf(),
        range,
        selection_range: Some(selection_range.unwrap_or(range)),
        container_name: container.map(|s| s.to_string()),
        detail,
    })
}

fn flatten_document_symbol(
    parent: &UnifiedSymbolInfo,
    value: &serde_json::Value,
    file_path: &Path,
    out: &mut Vec<UnifiedSymbolInfo>,
) {
    if let Some(children) = value.get("children").and_then(|v| v.as_array()) {
        for child in children {
            if let Some(sym) = parse_document_symbol(child, file_path, Some(&parent.name)) {
                flatten_document_symbol(&sym, child, file_path, out);
                out.push(sym);
            }
        }
    }
}

/// Parse an array of Location objects into SourceLocations.
fn parse_locations(result: &serde_json::Value) -> Vec<SourceLocation> {
    let arr = match result.as_array() {
        Some(a) => a,
        None => return vec![],
    };

    arr.iter()
        .filter_map(|item| {
            let uri_str = item.get("uri")?.as_str()?;
            let file_path = protocol::uri_string_to_path(uri_str)?;
            let range = protocol::parse_range_json(item.get("range")?)?;
            Some(SourceLocation::new(file_path, range))
        })
        .collect()
}

impl std::fmt::Debug for LspWrapper {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspWrapper")
            .field("extensions", &self.extension_map.keys().collect::<Vec<_>>())
            .field("active_servers", &self.handlers.len())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_wrapper() {
        let wrapper = LspWrapper::new(None);
        assert!(wrapper.has_server_for(Path::new("test.rs")));
        assert!(wrapper.has_server_for(Path::new("test.py")));
        assert!(wrapper.has_server_for(Path::new("test.ts")));
        assert!(!wrapper.has_server_for(Path::new("test.unknown_ext")));
        assert!(!wrapper.has_server_for(Path::new("noext")));
    }

    #[test]
    fn test_register_custom_server() {
        let mut wrapper = LspWrapper::new(None);
        assert!(!wrapper.has_server_for(Path::new("test.xyz")));

        wrapper.register_server(ServerConfig::new(
            "xyz-server",
            vec![],
            "xyz",
            vec!["xyz".to_string()],
        ));

        assert!(wrapper.has_server_for(Path::new("test.xyz")));
    }

    #[cfg(unix)]
    #[test]
    fn test_parse_symbol_info() {
        let json = serde_json::json!({
            "name": "my_function",
            "kind": 12,
            "location": {
                "uri": "file:///src/main.rs",
                "range": {
                    "start": { "line": 5, "character": 0 },
                    "end": { "line": 15, "character": 1 }
                }
            },
            "containerName": "MyStruct"
        });
        let sym = parse_symbol_info(&json).unwrap();
        assert_eq!(sym.name, "my_function");
        assert_eq!(sym.kind, protocol::SymbolKind::Function);
        assert_eq!(sym.container_name.as_deref(), Some("MyStruct"));
    }

    #[cfg(unix)]
    #[test]
    fn test_parse_locations() {
        let json = serde_json::json!([
            {
                "uri": "file:///a.rs",
                "range": {
                    "start": { "line": 1, "character": 0 },
                    "end": { "line": 1, "character": 10 }
                }
            },
            {
                "uri": "file:///b.rs",
                "range": {
                    "start": { "line": 5, "character": 2 },
                    "end": { "line": 5, "character": 8 }
                }
            }
        ]);
        let locs = parse_locations(&json);
        assert_eq!(locs.len(), 2);
        assert_eq!(locs[0].file_path, PathBuf::from("/a.rs"));
        assert_eq!(locs[1].range.start.line, 5);
    }

    #[test]
    fn test_parse_empty_symbol_response() {
        let json = serde_json::json!([]);
        let syms = parse_symbol_response(&json);
        assert!(syms.is_empty());
    }

    #[test]
    fn test_parse_null_symbol_response() {
        let json = serde_json::Value::Null;
        let syms = parse_symbol_response(&json);
        assert!(syms.is_empty());
    }

    #[test]
    fn test_parse_document_symbol() {
        let json = serde_json::json!({
            "name": "MyClass",
            "kind": 5,
            "range": {
                "start": { "line": 0, "character": 0 },
                "end": { "line": 50, "character": 1 }
            },
            "selectionRange": {
                "start": { "line": 0, "character": 6 },
                "end": { "line": 0, "character": 13 }
            },
            "detail": "class",
            "children": [
                {
                    "name": "my_method",
                    "kind": 6,
                    "range": {
                        "start": { "line": 5, "character": 4 },
                        "end": { "line": 10, "character": 5 }
                    },
                    "selectionRange": {
                        "start": { "line": 5, "character": 8 },
                        "end": { "line": 5, "character": 17 }
                    }
                }
            ]
        });

        let result = serde_json::json!([json]);
        let syms = parse_document_symbols(&result, Path::new("/test.py"));
        // Should have child + parent = 2
        assert_eq!(syms.len(), 2);
        let names: Vec<&str> = syms.iter().map(|s| s.name.as_str()).collect();
        assert!(names.contains(&"MyClass"));
        assert!(names.contains(&"my_method"));
    }
}
