use crate::config::cli::ProviderType;
use crate::error::LuminattiError;
use dirs::home_dir;
use indoc::indoc;
use serde::{Deserialize, Deserializer};
use serde_json::from_reader;
use std::collections::HashMap;
use std::fs::File;
use std::io::BufReader;

use crate::Cli;

#[derive(Debug, Deserialize)]
pub struct LuminattiConfig {
    #[serde(
        default = "default_ai_provider",
        deserialize_with = "deserialize_ai_provider"
    )]
    pub provider: ProviderType,

    #[serde(default = "default_model")]
    pub model: Option<String>,

    #[serde(default = "default_api_key")]
    pub api_key: Option<String>,

    #[serde(default = "default_draft_config")]
    pub draft: DraftConfig,

    #[serde(default)]
    pub theme: Option<String>,

    #[serde(default)]
    pub wrap: Option<bool>,
}

#[derive(Debug, Deserialize, Default)]
pub struct DraftConfig {
    #[serde(
        default = "default_commit_types",
        deserialize_with = "deserialize_commit_types"
    )]
    pub commit_types: String,
}

fn default_ai_provider() -> ProviderType {
    std::env::var("LUMINATTI_AI_PROVIDER")
        .unwrap_or_else(|_| "openai".to_string())
        .parse()
        .unwrap_or(ProviderType::Openai)
}

fn deserialize_ai_provider<'de, D>(deserializer: D) -> Result<ProviderType, D::Error>
where
    D: Deserializer<'de>,
{
    let s = String::deserialize(deserializer)?;
    s.parse().map_err(serde::de::Error::custom)
}

fn default_commit_types() -> String {
    indoc! {r#"
    {
        "docs": "Documentation only changes",
        "style": "Changes that do not affect the meaning of the code",
        "refactor": "A code change that neither fixes a bug nor adds a feature",
        "perf": "A code change that improves performance",
        "test": "Adding missing tests or correcting existing tests",
        "build": "Changes that affect the build system or external dependencies",
        "ci": "Changes to our CI configuration files and scripts",
        "chore": "Other changes that don't modify src or test files",
        "revert": "Reverts a previous commit",
        "feat": "A new feature",
        "fix": "A bug fix"
    }
    "#}
    .to_string()
}

fn default_model() -> Option<String> {
    std::env::var("LUMINATTI_AI_MODEL").ok()
}

fn default_api_key() -> Option<String> {
    std::env::var("LUMINATTI_API_KEY").ok()
}

fn deserialize_commit_types<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let commit_types_map: HashMap<String, String> = HashMap::deserialize(deserializer)?;
    serde_json::to_string(&commit_types_map).map_err(serde::de::Error::custom)
}

fn default_draft_config() -> DraftConfig {
    DraftConfig {
        commit_types: default_commit_types(),
    }
}

fn default_config_path() -> Option<String> {
    home_dir().and_then(|mut path| {
        path.push(".config/luminatti/luminatti.config.json");
        path.exists()
            .then_some(path)
            .and_then(|p| p.to_str().map(|s| s.to_string()))
    })
}

impl LuminattiConfig {
    pub fn build(cli: &Cli) -> Result<Self, LuminattiError> {
        let config = if let Some(config_path) = &cli.config {
            LuminattiConfig::from_file(config_path)?
        } else {
            match default_config_path() {
                Some(path) => LuminattiConfig::from_file(&path)?,
                None => LuminattiConfig::default(),
            }
        };

        let provider = cli.provider.as_ref().cloned().unwrap_or(config.provider);
        let api_key = cli.api_key.clone().or(config.api_key);
        let model = cli.model.clone().or(config.model);

        Ok(LuminattiConfig {
            provider,
            model,
            api_key,
            draft: config.draft,
            theme: config.theme,
            wrap: config.wrap,
        })
    }

    pub fn from_file(file_path: &str) -> Result<Self, LuminattiError> {
        let file = File::open(file_path)?;
        let reader = BufReader::new(file);

        // Deserialize JSON data into the LuminattiConfig struct
        let config: LuminattiConfig = match from_reader(reader) {
            Ok(config) => config,
            Err(e) => return Err(LuminattiError::InvalidConfiguration(e.to_string())),
        };

        Ok(config)
    }
}

impl Default for LuminattiConfig {
    fn default() -> Self {
        LuminattiConfig {
            provider: default_ai_provider(),
            model: default_model(),
            api_key: default_api_key(),
            draft: default_draft_config(),
            theme: None,
            wrap: None,
        }
    }
}
