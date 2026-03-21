//! ModelRegistry: loading, querying, and caching provider/model data.

use std::collections::HashMap;
use std::path::Path;
use tracing::{debug, warn};

use super::sync::{is_cache_stale, sync_provider_cache, sync_provider_cache_async};
use super::{DEFAULT_CACHE_TTL, ModelInfo, PRIORITY_PROVIDERS, ProviderInfo};

/// Sort key for providers: priority providers first (in order), then alphabetical.
fn provider_sort_key(provider_id: &str) -> (u8, usize, String) {
    if let Some(idx) = PRIORITY_PROVIDERS.iter().position(|&p| p == provider_id) {
        (0, idx, String::new())
    } else {
        (1, 0, provider_id.to_lowercase())
    }
}

/// Registry for managing model and provider configurations.
#[derive(Debug, Clone)]
pub struct ModelRegistry {
    pub providers: HashMap<String, ProviderInfo>,
}

impl ModelRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            providers: HashMap::new(),
        }
    }

    /// Load registry from cache directory.
    pub fn load_from_cache(cache_dir: &Path) -> Self {
        let mut registry = Self::new();
        let providers_dir = cache_dir.join("providers");

        if !registry.load_providers_from_dir(&providers_dir) {
            // Cache empty — try sync
            if let Err(e) = sync_provider_cache(Some(cache_dir), None) {
                warn!("Failed to sync provider cache: {}", e);
            }
            registry.load_providers_from_dir(&providers_dir);
        }

        if registry.providers.is_empty() {
            warn!(
                "No provider configurations loaded. \
                 Check network connectivity and retry, or run: opendev setup"
            );
        }

        // Schedule background refresh if stale
        if !registry.providers.is_empty() && is_cache_stale(&providers_dir, DEFAULT_CACHE_TTL) {
            let cache_dir = cache_dir.to_path_buf();
            // Use tokio::spawn if inside a runtime, otherwise fall back to a thread
            if let Ok(_handle) = tokio::runtime::Handle::try_current() {
                let cache_dir_clone = cache_dir.clone();
                tokio::spawn(async move {
                    let _ = sync_provider_cache_async(Some(&cache_dir_clone), None).await;
                });
            } else {
                std::thread::Builder::new()
                    .name("models-dev-sync".to_string())
                    .spawn(move || {
                        let _ = sync_provider_cache(Some(&cache_dir), None);
                    })
                    .ok();
            }
        }

        registry
    }

    /// Load all provider JSON files from a directory.
    fn load_providers_from_dir(&mut self, directory: &Path) -> bool {
        if !directory.exists() {
            return false;
        }

        let mut count = 0;
        let mut entries: Vec<_> = match std::fs::read_dir(directory) {
            Ok(entries) => entries.filter_map(|e| e.ok()).collect(),
            Err(_) => return false,
        };
        entries.sort_by_key(|e| e.file_name());

        for entry in entries {
            let path = entry.path();
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            if path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with('.'))
            {
                continue;
            }

            match Self::load_provider_file(&path) {
                Ok(provider) => {
                    self.providers.insert(provider.id.clone(), provider);
                    count += 1;
                }
                Err(e) => {
                    debug!("Failed to load provider {:?}: {}", path.file_name(), e);
                }
            }
        }

        count > 0
    }

    /// Load a single provider JSON file.
    fn load_provider_file(path: &Path) -> Result<ProviderInfo, super::RegistryError> {
        let content = std::fs::read_to_string(path)?;
        let data: serde_json::Value = serde_json::from_str(&content)?;

        let provider_id = data["id"].as_str().unwrap_or_default().to_string();

        let mut models = HashMap::new();
        if let Some(models_obj) = data["models"].as_object() {
            for (model_key, model_data) in models_obj {
                let pricing = model_data.get("pricing").cloned().unwrap_or_default();
                models.insert(
                    model_key.clone(),
                    ModelInfo {
                        id: model_data["id"].as_str().unwrap_or(model_key).to_string(),
                        name: model_data["name"].as_str().unwrap_or(model_key).to_string(),
                        provider: model_data["provider"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string(),
                        context_length: model_data["context_length"].as_u64().unwrap_or(0),
                        capabilities: model_data["capabilities"]
                            .as_array()
                            .map(|arr| {
                                arr.iter()
                                    .filter_map(|v| v.as_str().map(String::from))
                                    .collect()
                            })
                            .unwrap_or_default(),
                        pricing_input: pricing["input"].as_f64().unwrap_or(0.0),
                        pricing_output: pricing["output"].as_f64().unwrap_or(0.0),
                        pricing_unit: pricing["unit"]
                            .as_str()
                            .unwrap_or("per million tokens")
                            .to_string(),
                        serverless: model_data["serverless"].as_bool().unwrap_or(false),
                        tunable: model_data["tunable"].as_bool().unwrap_or(false),
                        recommended: model_data["recommended"].as_bool().unwrap_or(false),
                        max_tokens: model_data["max_tokens"].as_u64(),
                        supports_temperature: model_data["supports_temperature"]
                            .as_bool()
                            .unwrap_or(true),
                        api_type: model_data["api_type"]
                            .as_str()
                            .unwrap_or("chat")
                            .to_string(),
                    },
                );
            }
        }

        Ok(ProviderInfo {
            id: provider_id,
            name: data["name"].as_str().unwrap_or_default().to_string(),
            description: data["description"].as_str().unwrap_or_default().to_string(),
            api_key_env: data["api_key_env"].as_str().unwrap_or_default().to_string(),
            api_base_url: data["api_base_url"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            models,
        })
    }

    /// Get provider information by ID.
    pub fn get_provider(&self, provider_id: &str) -> Option<&ProviderInfo> {
        self.providers.get(provider_id)
    }

    /// List all available providers, sorted by priority then alphabetically.
    pub fn list_providers(&self) -> Vec<&ProviderInfo> {
        let mut providers: Vec<&ProviderInfo> = self.providers.values().collect();
        providers.sort_by(|a, b| provider_sort_key(&a.id).cmp(&provider_sort_key(&b.id)));
        providers
    }

    /// Get model information by provider and model key.
    pub fn get_model(&self, provider_id: &str, model_key: &str) -> Option<&ModelInfo> {
        self.providers
            .get(provider_id)
            .and_then(|p| p.models.get(model_key))
    }

    /// Find a model by its full ID across all providers.
    ///
    /// When the same model ID exists under multiple providers, prefer providers
    /// whose API key environment variable is set (i.e. usable providers).
    pub fn find_model_by_id(&self, model_id: &str) -> Option<(&str, &str, &ModelInfo)> {
        let mut fallback: Option<(&str, &str, &ModelInfo)> = None;
        for (provider_id, provider) in &self.providers {
            for (model_key, model) in &provider.models {
                if model.id == model_id {
                    // Prefer providers with an available API key
                    if provider.api_key_env.is_empty()
                        || std::env::var(&provider.api_key_env).is_ok()
                    {
                        return Some((provider_id, model_key, model));
                    }
                    if fallback.is_none() {
                        fallback = Some((provider_id, model_key, model));
                    }
                }
            }
        }
        fallback
    }

    /// List all models across all providers with optional filters.
    pub fn list_all_models(
        &self,
        capability: Option<&str>,
        max_price: Option<f64>,
    ) -> Vec<(&str, &ModelInfo)> {
        let mut models = Vec::new();
        for (provider_id, provider) in &self.providers {
            for model in provider.models.values() {
                if let Some(cap) = capability
                    && !model.capabilities.contains(&cap.to_string())
                {
                    continue;
                }
                if let Some(max) = max_price
                    && model.pricing_output > max
                {
                    continue;
                }
                models.push((provider_id.as_str(), model));
            }
        }
        models.sort_by(|a, b| a.1.pricing_output.partial_cmp(&b.1.pricing_output).unwrap());
        models
    }
}

impl Default for ModelRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_from_cache() {
        let tmp = tempfile::TempDir::new().unwrap();
        let providers_dir = tmp.path().join("providers");
        std::fs::create_dir_all(&providers_dir).unwrap();

        let provider_json = serde_json::json!({
            "id": "test-provider",
            "name": "Test Provider",
            "description": "A test provider",
            "api_key_env": "TEST_KEY",
            "api_base_url": "https://api.test.com",
            "models": {
                "model-1": {
                    "id": "model-1",
                    "name": "Model One",
                    "provider": "Test Provider",
                    "context_length": 4096,
                    "capabilities": ["text"],
                    "pricing": {"input": 1.0, "output": 2.0, "unit": "per 1M tokens"},
                    "recommended": true
                }
            }
        });

        std::fs::write(
            providers_dir.join("test-provider.json"),
            serde_json::to_string_pretty(&provider_json).unwrap(),
        )
        .unwrap();

        let mut registry = ModelRegistry::new();
        assert!(registry.load_providers_from_dir(&providers_dir));
        assert_eq!(registry.providers.len(), 1);

        let provider = registry.get_provider("test-provider").unwrap();
        assert_eq!(provider.name, "Test Provider");
        assert_eq!(provider.models.len(), 1);

        let model = registry.get_model("test-provider", "model-1").unwrap();
        assert_eq!(model.context_length, 4096);

        let found = registry.find_model_by_id("model-1").unwrap();
        assert_eq!(found.0, "test-provider");
    }

    #[test]
    fn test_provider_sort_order() {
        let mut ids = vec!["zebra", "openai", "alpha", "anthropic"];
        ids.sort_by(|a, b| provider_sort_key(a).cmp(&provider_sort_key(b)));
        assert_eq!(ids, vec!["openai", "anthropic", "alpha", "zebra"]);
    }

    #[test]
    fn test_find_model_prefers_provider_with_api_key() {
        let mut registry = ModelRegistry::new();

        // Provider without API key set (use a unique env var name that won't exist)
        let no_key_env = "OPENDEV_TEST_NO_KEY_SET_12345";

        let mut models_a = HashMap::new();
        models_a.insert(
            "shared-model".to_string(),
            ModelInfo {
                id: "shared-model".to_string(),
                name: "Shared Model".to_string(),
                provider: "No Key Provider".to_string(),
                context_length: 4096,
                capabilities: vec!["text".to_string()],
                pricing_input: 1.0,
                pricing_output: 2.0,
                pricing_unit: "per 1M tokens".to_string(),
                recommended: false,
                max_tokens: None,
                supports_temperature: true,
                serverless: false,
                tunable: false,
                api_type: "chat".to_string(),
            },
        );
        registry.providers.insert(
            "no-key-provider".to_string(),
            ProviderInfo {
                id: "no-key-provider".to_string(),
                name: "No Key Provider".to_string(),
                description: String::new(),
                api_key_env: no_key_env.to_string(),
                api_base_url: String::new(),
                models: models_a,
            },
        );

        // Provider with empty api_key_env (no key required — always usable)
        let mut models_b = HashMap::new();
        models_b.insert(
            "shared-model".to_string(),
            ModelInfo {
                id: "shared-model".to_string(),
                name: "Shared Model".to_string(),
                provider: "Free Provider".to_string(),
                context_length: 4096,
                capabilities: vec!["text".to_string()],
                pricing_input: 1.0,
                pricing_output: 2.0,
                pricing_unit: "per 1M tokens".to_string(),
                recommended: false,
                max_tokens: None,
                supports_temperature: true,
                serverless: false,
                tunable: false,
                api_type: "chat".to_string(),
            },
        );
        registry.providers.insert(
            "free-provider".to_string(),
            ProviderInfo {
                id: "free-provider".to_string(),
                name: "Free Provider".to_string(),
                description: String::new(),
                api_key_env: String::new(),
                api_base_url: String::new(),
                models: models_b,
            },
        );

        // Should prefer the provider that doesn't require a missing API key
        let result = registry.find_model_by_id("shared-model").unwrap();
        assert_eq!(
            result.0, "free-provider",
            "Should prefer provider with available API key over one without"
        );
    }
}
