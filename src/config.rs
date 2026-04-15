use std::fs;
use std::path::Path;

use anyhow::{bail, Context, Result};
use serde::{Deserialize, Serialize};

use crate::cli;

const CONFIG_FILE_NAME: &str = "config.toml";
const DEFAULT_OPENAI_API_BASE: &str = "https://api.openai.com/v1";
const DEFAULT_OPENAI_MODEL: &str = "gpt-4o-mini";
const DEFAULT_OUTLINE_SYSTEM_PROMPT: &str =
    "你是一个专业的大纲 Agent，负责生成小说结构、章节规划和情节走向。";
const DEFAULT_MEMORY_SYSTEM_PROMPT: &str =
    "你是一个专业的记忆 Agent，负责维护世界观、人物设定、伏笔和上下文一致性。";
const DEFAULT_WRITER_SYSTEM_PROMPT: &str =
    "你是一个专业的主笔 Agent，负责根据大纲和记忆信息撰写高质量正文。";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub outline_agent: AgentConfig,
    pub memory_agent: AgentConfig,
    pub writer_agent: AgentConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub provider: Provider,
    pub api_base: Option<String>,
    pub api_key: Option<String>,
    pub model: String,
    pub system_prompt: String,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Provider {
    OpenAi,
    Ollama,
    Anthropic,
}

impl AppConfig {
    pub fn load_or_create_interactively() -> Result<Self> {
        let path = Path::new(CONFIG_FILE_NAME);
        if path.exists() {
            return Self::load();
        }

        println!("未检测到 config.toml，开始交互式生成配置。\n");
        let config = cli::prompt_app_config(
            &Self::outline_agent_defaults(),
            &Self::memory_agent_defaults(),
            &Self::writer_agent_defaults(),
        )?;
        config.validate()?;
        config.save()?;
        println!("\n已生成 config.toml，后续启动将直接复用该配置。\n");
        Ok(config)
    }

    fn load() -> Result<Self> {
        let path = Path::new(CONFIG_FILE_NAME);
        let content = fs::read_to_string(path)
            .with_context(|| format!("failed to read config file: {}", path.display()))?;
        let config = toml::from_str::<AppConfig>(&content)
            .with_context(|| format!("failed to parse config file: {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn save(&self) -> Result<()> {
        let content = toml::to_string_pretty(self)
            .context("failed to serialize application config into TOML")?;
        fs::write(CONFIG_FILE_NAME, content.as_bytes())
            .with_context(|| format!("failed to write config file: {CONFIG_FILE_NAME}"))?;
        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.outline_agent.validate("outline_agent")?;
        self.memory_agent.validate("memory_agent")?;
        self.writer_agent.validate("writer_agent")?;
        Ok(())
    }

    fn outline_agent_defaults() -> AgentConfig {
        AgentConfig {
            provider: Provider::OpenAi,
            api_base: Some(DEFAULT_OPENAI_API_BASE.to_string()),
            api_key: Some("YOUR_API_KEY".to_string()),
            model: DEFAULT_OPENAI_MODEL.to_string(),
            system_prompt: DEFAULT_OUTLINE_SYSTEM_PROMPT.to_string(),
            temperature: 0.7,
        }
    }

    fn memory_agent_defaults() -> AgentConfig {
        AgentConfig {
            provider: Provider::OpenAi,
            api_base: Some(DEFAULT_OPENAI_API_BASE.to_string()),
            api_key: Some("YOUR_API_KEY".to_string()),
            model: DEFAULT_OPENAI_MODEL.to_string(),
            system_prompt: DEFAULT_MEMORY_SYSTEM_PROMPT.to_string(),
            temperature: 0.4,
        }
    }

    fn writer_agent_defaults() -> AgentConfig {
        AgentConfig {
            provider: Provider::OpenAi,
            api_base: Some(DEFAULT_OPENAI_API_BASE.to_string()),
            api_key: Some("YOUR_API_KEY".to_string()),
            model: DEFAULT_OPENAI_MODEL.to_string(),
            system_prompt: DEFAULT_WRITER_SYSTEM_PROMPT.to_string(),
            temperature: 0.9,
        }
    }
}

impl AgentConfig {
    fn validate(&self, name: &str) -> Result<()> {
        if !(0.0..=2.0).contains(&self.temperature) {
            bail!("{name}.temperature must be between 0.0 and 2.0");
        }

        if self.model.trim().is_empty() {
            bail!("{name}.model must not be empty");
        }

        if self.system_prompt.trim().is_empty() {
            bail!("{name}.system_prompt must not be empty");
        }

        let api_base = self
            .api_base
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty());
        if api_base.is_none() {
            bail!("{name}.api_base must not be empty");
        }

        Ok(())
    }
}
