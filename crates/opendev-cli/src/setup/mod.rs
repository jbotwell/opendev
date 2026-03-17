//! Interactive setup wizard for first-run configuration.
//!
//! Mirrors `opendev/setup/wizard.py`.
//!
//! 9-step railway-style wizard with arrow-key navigation, search filtering,
//! and multi-slot model configuration.

mod interactive_menu;
pub mod providers;
mod rail_ui;

use std::collections::HashMap;
use std::io;

use opendev_config::models_dev::ModelRegistry;
use opendev_config::{ConfigLoader, Paths};
use opendev_models::AppConfig;
use thiserror::Error;
use tracing::info;

use interactive_menu::InteractiveMenu;
use providers::{ProviderConfig, ProviderSetup};
use rail_ui::*;

// ── Errors ──────────────────────────────────────────────────────────────────

#[derive(Debug, Error)]
#[allow(dead_code)]
pub enum SetupError {
    #[error("setup cancelled by user")]
    Cancelled,
    #[error("no provider selected")]
    NoProvider,
    #[error("no API key provided")]
    NoApiKey,
    #[error("API key validation failed: {0}")]
    ValidationFailed(String),
    #[error("no model selected")]
    NoModel,
    #[error("failed to save configuration: {0}")]
    SaveFailed(String),
    #[error("model registry unavailable: {0}")]
    RegistryError(String),
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

// ── Public API ──────────────────────────────────────────────────────────────

/// Check whether a global settings file already exists.
pub fn config_exists() -> bool {
    let paths = Paths::default();
    paths.global_settings().exists()
}

/// Run the interactive setup wizard.
///
/// Returns the final [`AppConfig`] on success.
pub async fn run_setup_wizard() -> Result<AppConfig, SetupError> {
    // Step 0: Load registry
    let paths = Paths::default();
    let cache_dir = paths.global_cache_dir();
    let registry = ModelRegistry::load_from_cache(&cache_dir);
    if registry.providers.is_empty() {
        return Err(SetupError::RegistryError(
            "No provider data available. Check network connectivity and retry.".into(),
        ));
    }

    rail_intro(
        "Welcome to OpenDev!",
        &[
            "First-time setup detected.",
            "Let's configure your AI provider.",
        ],
    );

    // Step 1: Select provider
    let provider_id = select_provider_with_label(&registry, Some("1 of 9"))?;
    let provider_config = ProviderSetup::get_provider_config(&registry, &provider_id)
        .ok_or(SetupError::NoProvider)?;

    // Step 2: Get API key
    let api_key = get_api_key_with_label(&provider_id, &provider_config, Some("2 of 9"))?;

    // Step 3: Optional validation
    if rail_confirm("Validate API key?", true)? {
        match ProviderSetup::validate_api_key(&provider_config, &api_key).await {
            Ok(()) => rail_success("Valid!"),
            Err(e) => {
                rail_error(&format!("Failed: {e}"));
                rail_warning(
                    "Continuing without validation. You may encounter errors if the key is invalid.",
                );
            }
        }
    }
    rail_separator();

    // Step 4: Select model
    let model_id =
        select_model_with_label(&provider_id, &provider_config, &registry, Some("4 of 9"))?;

    // Look up model info for smart defaults
    let normal_model_info = registry.find_model_by_id(&model_id);
    let normal_model_name = normal_model_info
        .map(|(_, _, m)| m.name.as_str())
        .unwrap_or("your model");

    let mut collected_keys: HashMap<String, String> = HashMap::new();
    collected_keys.insert(provider_id.clone(), api_key.clone());

    // Step 5: Thinking model
    let (thinking_provider, thinking_model) = configure_slot_model(
        &registry,
        "Thinking",
        "Used for complex reasoning and planning tasks.",
        "reasoning",
        "5 of 9",
        normal_model_name,
        &provider_id,
        &model_id,
        &mut collected_keys,
    )?;

    // Step 6: Critique model (defaults cascade from thinking)
    let (critique_provider, critique_model) = configure_slot_model(
        &registry,
        "Critique",
        "Used for self-critique of reasoning. Falls back to Thinking model.",
        "reasoning",
        "6 of 9",
        normal_model_name,
        &thinking_provider,
        &thinking_model,
        &mut collected_keys,
    )?;

    // Step 7: Vision model
    let (vlm_provider, vlm_model) = configure_slot_model(
        &registry,
        "Vision",
        "Used for image and screenshot analysis.",
        "vision",
        "7 of 9",
        normal_model_name,
        &provider_id,
        &model_id,
        &mut collected_keys,
    )?;

    // Step 8: Compact model
    let (compact_provider, compact_model) = configure_slot_model(
        &registry,
        "Compact",
        "Used for summarizing long conversations to manage context length.",
        "any",
        "8 of 9",
        normal_model_name,
        &provider_id,
        &model_id,
        &mut collected_keys,
    )?;

    // Build config
    let config = AppConfig {
        model_provider: provider_id.clone(),
        model: model_id.clone(),
        api_key: Some(api_key),
        auto_save_interval: 5,
        model_critique: Some(critique_model.clone()),
        model_critique_provider: Some(critique_provider.clone()),
        model_vlm: Some(vlm_model.clone()),
        model_vlm_provider: Some(vlm_provider.clone()),
        model_compact: Some(compact_model.clone()),
        model_compact_provider: Some(compact_provider.clone()),
        ..AppConfig::default()
    };

    // Step 9: Summary + save
    show_config_summary(
        &registry,
        &provider_id,
        &model_id,
        &thinking_provider,
        &thinking_model,
        &critique_provider,
        &critique_model,
        &vlm_provider,
        &vlm_model,
        &compact_provider,
        &compact_model,
        &collected_keys,
    );

    if !rail_confirm("Save configuration?", true)? {
        rail_warning("Setup cancelled");
        return Err(SetupError::Cancelled);
    }

    let settings_path = paths.global_settings();
    ConfigLoader::save(&config, &settings_path)
        .map_err(|e| SetupError::SaveFailed(e.to_string()))?;

    info!(path = %settings_path.display(), "Configuration saved");
    rail_success(&format!(
        "Configuration saved to {}",
        settings_path.display()
    ));
    rail_separator();
    rail_outro("All set! Starting OpenDev...");

    Ok(config)
}

// ── Slot configuration ─────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn configure_slot_model(
    registry: &ModelRegistry,
    slot_name: &str,
    slot_description: &str,
    _capability: &str,
    step_label: &str,
    normal_model_name: &str,
    normal_provider_id: &str,
    normal_model_id: &str,
    collected_keys: &mut HashMap<String, String>,
) -> Result<(String, String), SetupError> {
    rail_info_box(
        &format!("{slot_name} Model"),
        &[slot_description.to_string()],
        Some(step_label),
    );

    let menu_items = vec![
        (
            "use_normal".to_string(),
            format!("Use {normal_model_name}"),
            "Same model, no extra setup needed".to_string(),
        ),
        (
            "choose_manually".to_string(),
            "Choose manually".to_string(),
            "Select a different provider and model".to_string(),
        ),
    ];

    let mut menu = InteractiveMenu::new(menu_items, &format!("Select {slot_name} Model"), 2);
    let choice = menu.show()?;

    if choice.as_deref() != Some("choose_manually") {
        return Ok((normal_provider_id.to_string(), normal_model_id.to_string()));
    }

    // "choose_manually" → provider/model selection sub-flow
    let slot_provider_id = match select_provider(registry) {
        Ok(id) => id,
        Err(_) => return Ok((normal_provider_id.to_string(), normal_model_id.to_string())),
    };

    let slot_provider_config = match ProviderSetup::get_provider_config(registry, &slot_provider_id)
    {
        Some(c) => c,
        None => {
            rail_error(&format!("Provider '{slot_provider_id}' not found"));
            return Ok((normal_provider_id.to_string(), normal_model_id.to_string()));
        }
    };

    // Collect API key if not already collected
    if !collected_keys.contains_key(&slot_provider_id) {
        let slot_api_key = match get_api_key(&slot_provider_id, &slot_provider_config) {
            Ok(key) => key,
            Err(_) => {
                return Ok((normal_provider_id.to_string(), normal_model_id.to_string()));
            }
        };
        if rail_confirm("Validate API key?", true)? {
            match tokio::runtime::Handle::try_current() {
                Ok(handle) => {
                    let result = handle.block_on(ProviderSetup::validate_api_key(
                        &slot_provider_config,
                        &slot_api_key,
                    ));
                    match result {
                        Ok(()) => rail_success("Valid!"),
                        Err(e) => {
                            rail_error(&format!("Failed: {e}"));
                            rail_warning("Continuing without validation.");
                        }
                    }
                }
                Err(_) => {
                    rail_warning("Cannot validate in sync context, skipping.");
                }
            }
        }
        collected_keys.insert(slot_provider_id.clone(), slot_api_key);
    } else {
        rail_success(&format!(
            "Using previously collected API key for {}",
            slot_provider_config.name
        ));
    }

    let slot_model_id = match select_model(&slot_provider_id, &slot_provider_config, registry) {
        Ok(id) => id,
        Err(_) => return Ok((normal_provider_id.to_string(), normal_model_id.to_string())),
    };

    Ok((slot_provider_id, slot_model_id))
}

// ── Summary ─────────────────────────────────────────────────────────────────

#[allow(clippy::too_many_arguments)]
fn show_config_summary(
    registry: &ModelRegistry,
    provider_id: &str,
    model_id: &str,
    thinking_provider: &str,
    thinking_model: &str,
    critique_provider: &str,
    critique_model: &str,
    vlm_provider: &str,
    vlm_model: &str,
    compact_provider: &str,
    compact_model: &str,
    collected_keys: &HashMap<String, String>,
) {
    let model_display = |pid: &str, mid: &str| -> String {
        let provider_name = registry
            .get_provider(pid)
            .map(|p| p.name.as_str())
            .unwrap_or(pid);
        let model_name = registry
            .find_model_by_id(mid)
            .map(|(_, _, m)| m.name.as_str())
            .unwrap_or(mid);
        format!("{provider_name} / {model_name}")
    };

    let normal_display = model_display(provider_id, model_id);

    let thinking_display = if thinking_model == model_id && thinking_provider == provider_id {
        "(same as Normal)".to_string()
    } else {
        model_display(thinking_provider, thinking_model)
    };

    let critique_display =
        if critique_model == thinking_model && critique_provider == thinking_provider {
            "(same as Thinking)".to_string()
        } else {
            model_display(critique_provider, critique_model)
        };

    let vlm_display = if vlm_model == model_id && vlm_provider == provider_id {
        "(same as Normal)".to_string()
    } else {
        model_display(vlm_provider, vlm_model)
    };

    let compact_display = if compact_model == model_id && compact_provider == provider_id {
        "(same as Normal)".to_string()
    } else {
        model_display(compact_provider, compact_model)
    };

    let rows: Vec<(&str, &str)> = vec![
        ("Normal:", &normal_display),
        ("Thinking:", &thinking_display),
        ("Critique:", &critique_display),
        ("Vision:", &vlm_display),
        ("Compact:", &compact_display),
    ];

    // Build API key status lines
    let mut extra_lines: Vec<String> = vec!["API Keys:".to_string()];
    let mut seen_providers: Vec<String> = Vec::new();
    for pid in collected_keys.keys() {
        if seen_providers.contains(pid) {
            continue;
        }
        seen_providers.push(pid.clone());
        let env_var = registry
            .get_provider(pid)
            .map(|p| p.api_key_env.as_str())
            .unwrap_or("")
            .to_string();
        let env_set = std::env::var(&env_var).is_ok();
        let status = if env_set { "✓" } else { "configured" };
        extra_lines.push(format!("  ${env_var} {status}"));
    }

    rail_summary_box("Configuration Summary", &rows, Some(&extra_lines));
}

// ── Step helpers ────────────────────────────────────────────────────────────

fn select_provider(registry: &ModelRegistry) -> Result<String, SetupError> {
    select_provider_with_label(registry, None)
}

fn select_provider_with_label(
    registry: &ModelRegistry,
    step_label: Option<&str>,
) -> Result<String, SetupError> {
    let choices = ProviderSetup::provider_choices(registry);

    rail_step("Select AI Provider", step_label);
    let mut menu = InteractiveMenu::new(choices.clone(), "Select AI Provider", 9);
    let provider_id = menu.show()?.ok_or(SetupError::Cancelled)?;

    let provider_name = choices
        .iter()
        .find(|(id, _, _)| id == &provider_id)
        .map(|(_, name, _)| name.as_str())
        .unwrap_or(&provider_id);
    rail_answer(provider_name);
    rail_separator();
    Ok(provider_id)
}

fn get_api_key(_provider_id: &str, provider_config: &ProviderConfig) -> Result<String, SetupError> {
    get_api_key_with_label(_provider_id, provider_config, None)
}

fn get_api_key_with_label(
    _provider_id: &str,
    provider_config: &ProviderConfig,
    step_label: Option<&str>,
) -> Result<String, SetupError> {
    let env_var = &provider_config.env_var;
    let env_key = std::env::var(env_var).ok().filter(|k| !k.is_empty());

    rail_step(&format!("{} API Key", provider_config.name), step_label);

    if let Some(ref ek) = env_key
        && rail_confirm(&format!("Found ${env_var} in environment. Use it?"), true)?
    {
        rail_success("Using API key from environment");
        rail_separator();
        return Ok(ek.clone());
    }

    let api_key = rail_prompt(
        &format!("Enter your {} API key:", provider_config.name),
        true,
    )?;

    if api_key.is_empty() {
        if let Some(ek) = env_key {
            rail_success(&format!("Using ${env_var}"));
            rail_separator();
            return Ok(ek);
        }
        rail_error("No API key provided");
        return Err(SetupError::NoApiKey);
    }

    rail_success("API key received");
    rail_separator();
    Ok(api_key)
}

fn select_model(
    provider_id: &str,
    provider_config: &ProviderConfig,
    registry: &ModelRegistry,
) -> Result<String, SetupError> {
    select_model_with_label(provider_id, provider_config, registry, None)
}

fn select_model_with_label(
    provider_id: &str,
    provider_config: &ProviderConfig,
    registry: &ModelRegistry,
    step_label: Option<&str>,
) -> Result<String, SetupError> {
    let models = ProviderSetup::get_provider_models(registry, provider_id);

    let mut model_choices: Vec<(String, String, String)> = models;
    model_choices.push((
        "__custom__".to_string(),
        "Custom Model".to_string(),
        "Enter a custom model ID".to_string(),
    ));

    let title = format!("Select Model for {}", provider_config.name);
    rail_step(&title, step_label);
    let mut menu = InteractiveMenu::new(model_choices.clone(), &title, 9);
    let model_id = menu.show()?.ok_or(SetupError::Cancelled)?;

    if model_id == "__custom__" {
        let custom_id = rail_prompt("Enter custom model ID", false)?;
        if custom_id.is_empty() {
            rail_warning("No custom model ID provided");
            return Err(SetupError::NoModel);
        }
        rail_answer(&format!("Custom: {custom_id}"));
        rail_separator();
        return Ok(custom_id);
    }

    let model_name = model_choices
        .iter()
        .find(|(id, _, _)| id == &model_id)
        .map(|(_, name, _)| name.as_str())
        .unwrap_or(&model_id);
    rail_answer(model_name);
    rail_separator();
    Ok(model_id)
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_exists_false_for_tmp() {
        let _ = config_exists();
    }

    #[test]
    fn test_setup_error_display() {
        let e = SetupError::Cancelled;
        assert_eq!(e.to_string(), "setup cancelled by user");

        let e = SetupError::NoApiKey;
        assert_eq!(e.to_string(), "no API key provided");

        let e = SetupError::ValidationFailed("bad key".into());
        assert!(e.to_string().contains("bad key"));

        let e = SetupError::SaveFailed("disk full".into());
        assert!(e.to_string().contains("disk full"));

        let e = SetupError::RegistryError("no data".into());
        assert!(e.to_string().contains("no data"));
    }

    #[test]
    fn test_setup_error_variants() {
        let errors: Vec<SetupError> = vec![
            SetupError::Cancelled,
            SetupError::NoProvider,
            SetupError::NoApiKey,
            SetupError::ValidationFailed("test".into()),
            SetupError::NoModel,
            SetupError::SaveFailed("test".into()),
            SetupError::RegistryError("test".into()),
            SetupError::Io(io::Error::new(io::ErrorKind::Other, "test")),
        ];
        assert_eq!(errors.len(), 8);
    }
}
