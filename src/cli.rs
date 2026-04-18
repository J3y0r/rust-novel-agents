use std::io::{self, Write};

use anyhow::{anyhow, Context, Result};
use clap::{Parser, Subcommand};

use crate::config::{AgentConfig, AppConfig, Provider};

#[derive(Debug, Parser)]
#[command(name = "novel")]
#[command(about = "纯 CLI 网文三 Agent 框架")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Commands,
}

#[derive(Debug, Subcommand)]
pub enum Commands {
    Outline {
        idea: String,
        #[arg(long)]
        requirements: Option<String>,
    },
    Memory {
        #[command(subcommand)]
        command: MemoryCommands,
    },
    Char {
        #[command(subcommand)]
        action: CharAction,
    },
    Lore {
        #[command(subcommand)]
        action: LoreAction,
    },
    Export {
        #[arg(long, default_value = "全书导出.md")]
        output: String,
    },
    Write {
        chapter_num: u32,
        requirement: Option<String>,
    },
    BatchWrite {
        start_chapter: u32,
        end_chapter: u32,
        requirement: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
pub enum MemoryCommands {
    Sync,
    Rebuild,
}

#[derive(Debug, Subcommand)]
pub enum CharAction {
    List,
    Add {
        name: String,
        desc: String,
        #[arg(default_value = "活跃")]
        status: String,
    },
    Kill {
        name: String,
    },
}

#[derive(Debug, Subcommand)]
pub enum LoreAction {
    List,
    Add {
        category: String,
        desc: String,
    },
}

fn read_line(prompt: &str) -> Result<String> {
    print!("{prompt}");
    io::stdout()
        .flush()
        .context("failed to flush stdout before reading input")?;

    let mut input = String::new();
    io::stdin()
        .read_line(&mut input)
        .context("failed to read line from stdin")?;

    Ok(input.trim().to_string())
}

fn read_non_empty_line(prompt: &str, default_value: &str) -> Result<String> {
    let input = read_line(&format!("{prompt} [{default_value}]: "))?;
    let value = if input.is_empty() {
        default_value.to_string()
    } else {
        input
    };

    if value.trim().is_empty() {
        return Err(anyhow!("input must not be empty"));
    }

    Ok(value)
}

fn read_optional_line(prompt: &str, default_value: Option<&str>) -> Result<Option<String>> {
    let input = match default_value {
        Some(value) => read_line(&format!("{prompt} [{value}]: "))?,
        None => read_line(&format!("{prompt}: "))?,
    };

    if input.is_empty() {
        return Ok(default_value.map(str::to_string));
    }

    let value = input.trim();
    if value.is_empty() {
        return Ok(None);
    }

    Ok(Some(value.to_string()))
}

fn prompt_provider(default_provider: &Provider) -> Result<Provider> {
    let default_selection = match default_provider {
        Provider::OpenAi => "1",
        Provider::Ollama => "2",
        Provider::Anthropic => "3",
    };
    let input = read_line(&format!(
        "请选择 provider（1=openai, 2=ollama, 3=anthropic）[{default_selection}]: "
    ))?;
    let selection = if input.is_empty() {
        default_selection
    } else {
        input.as_str()
    };

    match selection {
        "1" => Ok(Provider::OpenAi),
        "2" => Ok(Provider::Ollama),
        "3" => Ok(Provider::Anthropic),
        _ => Err(anyhow!("invalid provider selection, expected 1, 2, or 3")),
    }
}

fn prompt_temperature(default_temperature: f32) -> Result<f32> {
    let input = read_line(&format!("请输入 temperature [{default_temperature}]: "))?;
    let value = if input.is_empty() {
        default_temperature
    } else {
        input
            .parse::<f32>()
            .context("temperature must be a valid number")?
    };

    if !(0.0..=2.0).contains(&value) {
        return Err(anyhow!("temperature must be between 0.0 and 2.0"));
    }

    Ok(value)
}

fn prompt_agent_config(agent_name: &str, defaults: &AgentConfig) -> Result<AgentConfig> {
    println!("开始配置 {agent_name}：");

    let provider = prompt_provider(&defaults.provider)?;
    let api_base = read_optional_line("请输入 api_base", defaults.api_base.as_deref())?;
    let api_key = read_optional_line("请输入 api_key", defaults.api_key.as_deref())?;
    let model = read_non_empty_line("请输入 model", &defaults.model)?;
    let system_prompt = read_non_empty_line("请输入 system_prompt", &defaults.system_prompt)?;
    let temperature = prompt_temperature(defaults.temperature)?;

    println!();

    Ok(AgentConfig {
        provider,
        api_base,
        api_key,
        model,
        system_prompt,
        temperature,
    })
}

pub fn prompt_app_config(
    outline_defaults: &AgentConfig,
    memory_defaults: &AgentConfig,
    writer_defaults: &AgentConfig,
) -> Result<AppConfig> {
    Ok(AppConfig {
        outline_agent: prompt_agent_config("outline_agent", outline_defaults)?,
        memory_agent: prompt_agent_config("memory_agent", memory_defaults)?,
        writer_agent: prompt_agent_config("writer_agent", writer_defaults)?,
    })
}

pub fn prompt_confirm(message: &str) -> Result<bool> {
    let answer = read_line(&format!("{message} [y/N]: "))?;
    Ok(matches!(answer.to_ascii_lowercase().as_str(), "y" | "yes"))
}

pub fn prompt_retry_or_exit(message: &str) -> Result<bool> {
    loop {
        let answer = read_line(&format!("{message} [r=retry/e=exit]: "))?;
        match answer.to_ascii_lowercase().as_str() {
            "r" | "retry" => return Ok(true),
            "e" | "exit" | "" => return Ok(false),
            _ => println!("请输入 r（重试）或 e（退出）"),
        }
    }
}
