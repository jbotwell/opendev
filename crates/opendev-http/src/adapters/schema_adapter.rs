//! Provider-specific schema adaptation.
//!
//! Different LLM providers have different JSON Schema requirements for tool
//! definitions. This module applies provider-specific transformations to tool
//! schemas before they are sent to the LLM.

use serde_json::Value;
use tracing::debug;

/// Apply provider-specific schema transformations.
///
/// This is a pure function — does not mutate the input schemas.
/// Returns a (possibly deep-copied) list of adapted schemas.
pub fn adapt_for_provider(schemas: &[Value], provider: &str) -> Vec<Value> {
    let provider = provider.to_lowercase();

    // No adaptation needed for standard providers
    if matches!(provider.as_str(), "openai" | "anthropic" | "openrouter") {
        return schemas.to_vec();
    }

    // Deep copy to avoid mutating originals
    let mut adapted: Vec<Value> = schemas.to_vec();
    let mut modified = false;

    match provider.as_str() {
        "gemini" | "google" => {
            if adapt_gemini(&mut adapted) {
                modified = true;
            }
        }
        "xai" | "grok" => {
            if adapt_xai(&mut adapted) {
                modified = true;
            }
        }
        "mistral" => {
            if adapt_mistral(&mut adapted) {
                modified = true;
            }
        }
        _ => {}
    }

    // General cleanup for all non-standard providers
    if general_cleanup(&mut adapted) {
        modified = true;
    }

    if modified {
        debug!(
            count = adapted.len(),
            provider = provider.as_str(),
            "Adapted tool schemas for provider"
        );
    }

    adapted
}

/// Gemini rejects `additionalProperties`, `default`, `$schema`, `format`
/// in nested schemas.
fn adapt_gemini(schemas: &mut [Value]) -> bool {
    const KEYS_TO_STRIP: &[&str] = &["additionalProperties", "default", "$schema", "format"];
    let mut changed = false;
    for schema in schemas.iter_mut() {
        if let Some(params) = schema.pointer_mut("/function/parameters")
            && strip_keys_recursive(params, KEYS_TO_STRIP)
        {
            changed = true;
        }
    }
    changed
}

/// xAI/Grok has a native `web_search` that conflicts with our tool.
fn adapt_xai(schemas: &mut Vec<Value>) -> bool {
    let before = schemas.len();
    schemas.retain(|schema| {
        let name = schema
            .pointer("/function/name")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        name != "web_search"
    });
    let removed = before != schemas.len();
    if removed {
        debug!("Filtered out web_search tool for xAI provider (native conflict)");
    }
    removed
}

/// Mistral doesn't support `anyOf`/`oneOf`/`allOf` — flatten to simple types.
fn adapt_mistral(schemas: &mut [Value]) -> bool {
    let mut changed = false;
    for schema in schemas.iter_mut() {
        if let Some(params) = schema.pointer_mut("/function/parameters")
            && flatten_union_types(params)
        {
            changed = true;
        }
    }
    changed
}

/// Ensure schemas follow basic requirements for all providers.
fn general_cleanup(schemas: &mut [Value]) -> bool {
    let mut changed = false;
    for schema in schemas.iter_mut() {
        if let Some(params) = schema.pointer_mut("/function/parameters")
            && let Some(obj) = params.as_object_mut()
        {
            if !obj.contains_key("type") {
                obj.insert("type".to_string(), Value::String("object".to_string()));
                changed = true;
            }
            if !obj.contains_key("properties") {
                obj.insert(
                    "properties".to_string(),
                    Value::Object(serde_json::Map::new()),
                );
                changed = true;
            }
        }
    }
    changed
}

/// Recursively remove specified keys from a JSON value.
fn strip_keys_recursive(obj: &mut Value, keys: &[&str]) -> bool {
    match obj {
        Value::Object(map) => {
            let mut changed = false;
            let keys_present: Vec<String> = map
                .keys()
                .filter(|k| keys.contains(&k.as_str()))
                .cloned()
                .collect();
            for key in keys_present {
                map.remove(&key);
                changed = true;
            }
            for value in map.values_mut() {
                if strip_keys_recursive(value, keys) {
                    changed = true;
                }
            }
            changed
        }
        Value::Array(arr) => {
            let mut changed = false;
            for item in arr.iter_mut() {
                if strip_keys_recursive(item, keys) {
                    changed = true;
                }
            }
            changed
        }
        _ => false,
    }
}

/// Replace `anyOf`/`oneOf`/`allOf` with flattened variants (lossy but compatible).
fn flatten_union_types(obj: &mut Value) -> bool {
    let Some(map) = obj.as_object_mut() else {
        return false;
    };

    let mut changed = false;

    // Handle anyOf/oneOf: take first variant
    for union_key in &["anyOf", "oneOf"] {
        if let Some(variants) = map.remove(*union_key) {
            if let Some(arr) = variants.as_array()
                && let Some(first) = arr.first()
                && let Some(first_obj) = first.as_object()
            {
                for (k, v) in first_obj {
                    map.insert(k.clone(), v.clone());
                }
            }
            changed = true;
        }
    }

    // Handle allOf: merge all variants
    if let Some(variants) = map.remove("allOf") {
        if let Some(arr) = variants.as_array() {
            for variant in arr {
                if let Some(variant_obj) = variant.as_object() {
                    for (k, v) in variant_obj {
                        map.insert(k.clone(), v.clone());
                    }
                }
            }
        }
        changed = true;
    }

    // Recurse into nested objects and arrays
    let keys: Vec<String> = map.keys().cloned().collect();
    for key in keys {
        if let Some(value) = map.get_mut(&key) {
            match value {
                Value::Object(_) => {
                    if flatten_union_types(value) {
                        changed = true;
                    }
                }
                Value::Array(arr) => {
                    for item in arr.iter_mut() {
                        if flatten_union_types(item) {
                            changed = true;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    changed
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn make_tool_schema(name: &str, params: Value) -> Value {
        json!({
            "type": "function",
            "function": {
                "name": name,
                "description": "A test tool",
                "parameters": params
            }
        })
    }

    #[test]
    fn test_no_adaptation_for_openai() {
        let schemas = vec![make_tool_schema(
            "test",
            json!({"type": "object", "properties": {"a": {"type": "string"}}}),
        )];
        let result = adapt_for_provider(&schemas, "openai");
        assert_eq!(result, schemas);
    }

    #[test]
    fn test_no_adaptation_for_anthropic() {
        let schemas = vec![make_tool_schema(
            "test",
            json!({"type": "object", "properties": {}}),
        )];
        let result = adapt_for_provider(&schemas, "anthropic");
        assert_eq!(result, schemas);
    }

    #[test]
    fn test_gemini_strips_additional_properties() {
        let schemas = vec![make_tool_schema(
            "test",
            json!({
                "type": "object",
                "properties": {
                    "a": {"type": "string", "default": "hello", "format": "uri"}
                },
                "additionalProperties": false,
                "$schema": "http://json-schema.org/draft-07/schema#"
            }),
        )];
        let result = adapt_for_provider(&schemas, "gemini");
        let params = result[0].pointer("/function/parameters").unwrap();
        assert!(params.get("additionalProperties").is_none());
        assert!(params.get("$schema").is_none());
        let prop_a = &params["properties"]["a"];
        assert!(prop_a.get("default").is_none());
        assert!(prop_a.get("format").is_none());
        assert_eq!(prop_a["type"], "string");
    }

    #[test]
    fn test_gemini_strips_nested_keys() {
        let schemas = vec![make_tool_schema(
            "test",
            json!({
                "type": "object",
                "properties": {
                    "nested": {
                        "type": "object",
                        "properties": {
                            "deep": {"type": "number", "default": 42}
                        },
                        "additionalProperties": false
                    }
                }
            }),
        )];
        let result = adapt_for_provider(&schemas, "gemini");
        let nested = &result[0]["function"]["parameters"]["properties"]["nested"];
        assert!(nested.get("additionalProperties").is_none());
        assert!(nested["properties"]["deep"].get("default").is_none());
    }

    #[test]
    fn test_xai_filters_web_search() {
        let schemas = vec![
            make_tool_schema("web_search", json!({"type": "object", "properties": {}})),
            make_tool_schema("read_file", json!({"type": "object", "properties": {}})),
        ];
        let result = adapt_for_provider(&schemas, "xai");
        assert_eq!(result.len(), 1);
        assert_eq!(
            result[0].pointer("/function/name").unwrap().as_str(),
            Some("read_file")
        );
    }

    #[test]
    fn test_xai_no_web_search_unchanged() {
        let schemas = vec![make_tool_schema(
            "read_file",
            json!({"type": "object", "properties": {}}),
        )];
        let result = adapt_for_provider(&schemas, "xai");
        assert_eq!(result.len(), 1);
    }

    #[test]
    fn test_mistral_flattens_any_of() {
        let schemas = vec![make_tool_schema(
            "test",
            json!({
                "type": "object",
                "properties": {
                    "value": {
                        "anyOf": [
                            {"type": "string"},
                            {"type": "number"}
                        ]
                    }
                }
            }),
        )];
        let result = adapt_for_provider(&schemas, "mistral");
        let prop = &result[0]["function"]["parameters"]["properties"]["value"];
        assert!(prop.get("anyOf").is_none());
        assert_eq!(prop["type"], "string");
    }

    #[test]
    fn test_mistral_flattens_one_of() {
        let schemas = vec![make_tool_schema(
            "test",
            json!({
                "type": "object",
                "properties": {
                    "value": {
                        "oneOf": [
                            {"type": "integer"},
                            {"type": "boolean"}
                        ]
                    }
                }
            }),
        )];
        let result = adapt_for_provider(&schemas, "mistral");
        let prop = &result[0]["function"]["parameters"]["properties"]["value"];
        assert!(prop.get("oneOf").is_none());
        assert_eq!(prop["type"], "integer");
    }

    #[test]
    fn test_mistral_merges_all_of() {
        let schemas = vec![make_tool_schema(
            "test",
            json!({
                "type": "object",
                "properties": {
                    "value": {
                        "allOf": [
                            {"type": "string"},
                            {"minLength": 1}
                        ]
                    }
                }
            }),
        )];
        let result = adapt_for_provider(&schemas, "mistral");
        let prop = &result[0]["function"]["parameters"]["properties"]["value"];
        assert!(prop.get("allOf").is_none());
        assert_eq!(prop["type"], "string");
        assert_eq!(prop["minLength"], 1);
    }

    #[test]
    fn test_general_cleanup_adds_type() {
        let schemas = vec![make_tool_schema(
            "test",
            json!({"properties": {"a": {"type": "string"}}}),
        )];
        let result = adapt_for_provider(&schemas, "fireworks");
        let params = &result[0]["function"]["parameters"];
        assert_eq!(params["type"], "object");
    }

    #[test]
    fn test_general_cleanup_adds_properties() {
        let schemas = vec![make_tool_schema("test", json!({"type": "object"}))];
        let result = adapt_for_provider(&schemas, "fireworks");
        let params = &result[0]["function"]["parameters"];
        assert!(params.get("properties").is_some());
    }

    #[test]
    fn test_case_insensitive_provider() {
        let schemas = vec![make_tool_schema(
            "web_search",
            json!({"type": "object", "properties": {}}),
        )];
        let result = adapt_for_provider(&schemas, "XAI");
        assert_eq!(result.len(), 0);
    }

    #[test]
    fn test_does_not_mutate_input() {
        let schemas = vec![make_tool_schema(
            "test",
            json!({
                "type": "object",
                "properties": {"a": {"type": "string", "default": "hi"}},
                "additionalProperties": false
            }),
        )];
        let original = schemas[0].clone();
        let _result = adapt_for_provider(&schemas, "gemini");
        assert_eq!(schemas[0], original);
    }

    #[test]
    fn test_empty_schemas() {
        let schemas: Vec<Value> = vec![];
        let result = adapt_for_provider(&schemas, "gemini");
        assert!(result.is_empty());
    }

    #[test]
    fn test_google_alias_for_gemini() {
        let schemas = vec![make_tool_schema(
            "test",
            json!({
                "type": "object",
                "properties": {"a": {"type": "string", "default": "x"}},
            }),
        )];
        let result = adapt_for_provider(&schemas, "google");
        let prop = &result[0]["function"]["parameters"]["properties"]["a"];
        assert!(prop.get("default").is_none());
    }

    #[test]
    fn test_grok_alias_for_xai() {
        let schemas = vec![make_tool_schema(
            "web_search",
            json!({"type": "object", "properties": {}}),
        )];
        let result = adapt_for_provider(&schemas, "grok");
        assert!(result.is_empty());
    }
}
