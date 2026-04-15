mod agents;
mod cli;
mod config;
mod core;

use std::fs;

use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;

use crate::agents::{MemoryAgent, OutlineAgent, WriterAgent};
use crate::cli::{Cli, Commands, MemoryCommands};
use crate::config::AppConfig;
use crate::core::memory_db::MemoryDb;

const OUTLINE_FILE_PATH: &str = "outline.txt";
const CHAPTERS_DIR: &str = "chapters";

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let config =
        AppConfig::load_or_create_interactively().context("failed to load application config")?;
    let db = MemoryDb::new().context("failed to initialize memory database")?;
    let outline_agent = OutlineAgent::new(config.outline_agent.clone())
        .context("failed to initialize outline agent")?;
    let memory_agent = MemoryAgent::new(config.memory_agent.clone())
        .context("failed to initialize memory agent")?;
    let writer_agent = WriterAgent::new(config.writer_agent.clone())
        .context("failed to initialize writer agent")?;

    match cli.command {
        Commands::Outline { idea, requirements } => {
            outline_agent
                .generate_outline(
                    &idea,
                    requirements.as_deref().unwrap_or(""),
                    &db,
                    &memory_agent,
                )
                .await?;
        }
        Commands::Memory {
            command: MemoryCommands::Sync,
        } => {
            let outline_text = fs::read_to_string(OUTLINE_FILE_PATH)
                .with_context(|| format!("failed to read outline file: {OUTLINE_FILE_PATH}"))?;
            memory_agent.sync_from_outline(&outline_text, &db).await?;
            println!(
                "{} {}",
                "[Memory]".green().bold(),
                "已从 outline.txt 重新同步记忆到 memory.db".green()
            );
        }
        Commands::Writer {
            chapter_num,
            requirement,
        } => {
            println!(
                "{} {}",
                "[⏳]".yellow().bold(),
                format!("正在撰写第 {} 章...", chapter_num).yellow()
            );

            writer_agent
                .write_chapter(
                    chapter_num,
                    requirement.as_deref().unwrap_or(""),
                    &db,
                    &memory_agent,
                )
                .await?;

            let chapter_path = format!("{CHAPTERS_DIR}/chapter_{chapter_num}.txt");
            println!(
                "{} {}",
                "[✅]".green().bold(),
                format!("第 {} 章已保存至 {}", chapter_num, chapter_path).green()
            );

            println!(
                "{} {}",
                "[⏳]".yellow().bold(),
                "正在提取本章摘要并更新记忆库...".yellow()
            );

            let chapter_text = fs::read_to_string(&chapter_path)
                .with_context(|| format!("failed to read chapter file: {chapter_path}"))?;
            memory_agent
                .summarize_chapter(chapter_num, &chapter_text, &db)
                .await?;

            println!(
                "{} {}",
                "[✅]".green().bold(),
                "记忆库更新完成！系统已记住当前剧情进度。".green()
            );
        }
    }

    Ok(())
}
