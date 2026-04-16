mod agents;
mod cli;
mod config;
mod core;

use std::fs;

use anyhow::{anyhow, Context, Result};
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
        Commands::Write {
            chapter_num,
            requirement,
        } => {
            println!(
                "{} {}",
                "[⏳]".yellow().bold(),
                format!("正在撰写第 {} 章...", chapter_num).yellow()
            );

            let chapter_text = writer_agent
                .write_chapter(
                    chapter_num,
                    requirement.as_deref(),
                    1,
                    1,
                    &db,
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

            memory_agent
                .summarize_chapter(chapter_num, &chapter_text, &db)
                .await?;

            println!(
                "{} {}",
                "[✅]".green().bold(),
                "记忆库更新完成！系统已记住当前剧情进度。".green()
            );
        }
        Commands::BatchWrite {
            start_chapter,
            end_chapter,
            requirement,
        } => {
            if end_chapter < start_chapter {
                return Err(anyhow!(
                    "end_chapter 必须大于或等于 start_chapter，当前输入: {} < {}",
                    end_chapter,
                    start_chapter
                ));
            }

            let total_chapters = end_chapter - start_chapter + 1;
            let requirement = requirement.unwrap_or_default();

            for chapter_num in start_chapter..=end_chapter {
                let current_index = chapter_num - start_chapter + 1;

                loop {
                    println!(
                        "{} {}",
                        "[⏳]".yellow().bold(),
                        format!(
                            "正在撰写第 {} 章 (剧情进度: {}/{})...",
                            chapter_num, current_index, total_chapters
                        )
                        .yellow()
                    );

                    match writer_agent
                        .write_chapter(
                            chapter_num,
                            Some(requirement.as_str()),
                            current_index,
                            total_chapters,
                            &db,
                        )
                        .await
                    {
                        Ok(chapter_text) => {
                            let chapter_path = format!("{CHAPTERS_DIR}/chapter_{chapter_num}.txt");
                            println!(
                                "{} {}",
                                "[✅]".green().bold(),
                                format!("第 {} 章已保存至 {}", chapter_num, chapter_path).green()
                            );

                            println!(
                                "{} {}",
                                "[⏳]".yellow().bold(),
                                format!("正在总结第 {} 章并写入记忆库...", chapter_num).yellow()
                            );

                            match memory_agent
                                .summarize_chapter(chapter_num, &chapter_text, &db)
                                .await
                            {
                                Ok(()) => {
                                    println!(
                                        "{} {}",
                                        "[✅]".green().bold(),
                                        format!(
                                            "第 {} 章摘要已写入记忆库，继续下一章。",
                                            chapter_num
                                        )
                                        .green()
                                    );
                                    break;
                                }
                                Err(error) => {
                                    println!(
                                        "{} {}",
                                        "[错误]".red().bold(),
                                        format!(
                                            "第 {} 章摘要更新失败：{:#}",
                                            chapter_num, error
                                        )
                                        .red()
                                    );
                                }
                            }
                        }
                        Err(error) => {
                            println!(
                                "{} {}",
                                "[错误]".red().bold(),
                                format!("第 {} 章生成失败：{:#}", chapter_num, error).red()
                            );
                        }
                    }

                    if !cli::prompt_retry_or_exit(&format!(
                        "第 {} 章处理失败，是否重试？选择 e 将立即退出 batch-write",
                        chapter_num
                    ))? {
                        println!(
                            "{} {}",
                            "[Stopped]".yellow().bold(),
                            format!("batch-write 已在第 {} 章停止。", chapter_num).yellow()
                        );
                        return Ok(());
                    }
                }
            }

            println!(
                "{} {}",
                "[✅]".green().bold(),
                format!(
                    "batch-write 已完成，共处理章节 {}-{}。",
                    start_chapter, end_chapter
                )
                .green()
            );
        }
    }

    Ok(())
}
