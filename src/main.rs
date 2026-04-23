mod agents;
mod cli;
mod config;
mod core;

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};
use clap::Parser;
use colored::Colorize;
use comfy_table::{presets::UTF8_FULL, ContentArrangement, Table};
use epub_builder::{EpubBuilder, EpubContent, ZipLibrary};

use crate::agents::{MemoryAgent, OutlineAgent, WriterAgent};
use crate::cli::{CharAction, Cli, Commands, LoreAction, MemoryCommands};
use crate::config::AppConfig;
use crate::core::memory_db::MemoryDb;

const OUTLINE_FILE_PATH: &str = "outline.txt";
const CHAPTERS_DIR: &str = "chapters";

fn load_config() -> Result<AppConfig> {
    AppConfig::load_or_create_interactively().context("failed to load application config")
}

fn init_db() -> Result<MemoryDb> {
    MemoryDb::new().context("failed to initialize memory database")
}

fn init_memory_agent() -> Result<MemoryAgent> {
    let config = load_config()?;
    MemoryAgent::new(config.memory_agent).context("failed to initialize memory agent")
}

fn init_outline_and_memory_agents() -> Result<(OutlineAgent, MemoryAgent)> {
    let config = load_config()?;
    let outline_agent =
        OutlineAgent::new(config.outline_agent).context("failed to initialize outline agent")?;
    let memory_agent =
        MemoryAgent::new(config.memory_agent).context("failed to initialize memory agent")?;
    Ok((outline_agent, memory_agent))
}

fn init_writer_and_memory_agents() -> Result<(WriterAgent, MemoryAgent)> {
    let config = load_config()?;
    let writer_agent =
        WriterAgent::new(config.writer_agent).context("failed to initialize writer agent")?;
    let memory_agent =
        MemoryAgent::new(config.memory_agent).context("failed to initialize memory agent")?;
    Ok((writer_agent, memory_agent))
}

fn collect_chapter_files(chapters_dir: &Path) -> Result<Vec<(u32, PathBuf)>> {
    let mut chapter_files = Vec::new();

    for entry in fs::read_dir(chapters_dir)
        .with_context(|| format!("failed to read directory: {}", chapters_dir.display()))?
    {
        let entry = entry.with_context(|| {
            format!("failed to read directory entry in {}", chapters_dir.display())
        })?;
        let file_name = entry.file_name();
        let Some(file_name) = file_name.to_str() else {
            continue;
        };
        let Some(chapter_num) = file_name
            .strip_prefix("chapter_")
            .and_then(|name| name.strip_suffix(".txt"))
            .and_then(|num| num.parse::<u32>().ok())
        else {
            continue;
        };
        chapter_files.push((chapter_num, entry.path()));
    }

    chapter_files.sort_by_key(|(chapter_num, _)| *chapter_num);
    Ok(chapter_files)
}

async fn run_chapter_range(
    start_chapter: u32,
    end_chapter: u32,
    requirement: Option<String>,
    command_name: &str,
) -> Result<()> {
    if end_chapter < start_chapter {
        return Err(anyhow!(
            "end_chapter 必须大于或等于 start_chapter，当前输入: {} < {}",
            end_chapter,
            start_chapter
        ));
    }

    let db = init_db()?;
    let (writer_agent, memory_agent) = init_writer_and_memory_agents()?;
    let total_chapters = end_chapter - start_chapter + 1;
    let requirement = requirement.unwrap_or_default();

    for chapter_num in start_chapter..=end_chapter {
        let current_index = chapter_num - start_chapter + 1;

        loop {
            db.delete_future_memories(chapter_num)?;
            println!(
                "{} {}",
                "[!]".yellow().bold(),
                format!(
                    "检测到重新生成，已自动清理第 {} 章及之后的旧记忆残留...",
                    chapter_num
                )
                .yellow()
            );

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
                "第 {} 章处理失败，是否重试？选择 e 将立即退出 {}",
                chapter_num, command_name
            ))? {
                println!(
                    "{} {}",
                    "[Stopped]".yellow().bold(),
                    format!("{} 已在第 {} 章停止。", command_name, chapter_num).yellow()
                );
                return Ok(());
            }
        }
    }

    println!(
        "{} {}",
        "[✅]".green().bold(),
        format!(
            "{} 已完成，共处理章节 {}-{}。",
            command_name, start_chapter, end_chapter
        )
        .green()
    );

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Outline { idea, requirements } => {
            let db = init_db()?;
            let (outline_agent, memory_agent) = init_outline_and_memory_agents()?;
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
            let db = init_db()?;
            let memory_agent = init_memory_agent()?;
            let outline_text = fs::read_to_string(OUTLINE_FILE_PATH)
                .with_context(|| format!("failed to read outline file: {OUTLINE_FILE_PATH}"))?;
            memory_agent.sync_from_outline(&outline_text, &db).await?;
            println!(
                "{} {}",
                "[Memory]".green().bold(),
                "已从 outline.txt 重新同步记忆到 memory.db".green()
            );
        }
        Commands::Memory {
            command: MemoryCommands::Rebuild,
        } => {
            let db = init_db()?;
            let memory_agent = init_memory_agent()?;
            println!(
                "{} {}",
                "[!]".yellow().bold(),
                "即将清空并彻底重建 memory.db ...".yellow()
            );
            db.clear_all_tables()?;

            let outline_text = fs::read_to_string(OUTLINE_FILE_PATH)
                .with_context(|| format!("failed to read outline file: {OUTLINE_FILE_PATH}"))?;
            memory_agent.sync_from_outline(&outline_text, &db).await?;

            let chapters_dir = Path::new(CHAPTERS_DIR);
            let chapter_files = if chapters_dir.exists() {
                collect_chapter_files(chapters_dir)?
            } else {
                Vec::new()
            };

            for (chapter_num, chapter_path) in chapter_files {
                println!(
                    "{} {}",
                    "[⏳]".yellow().bold(),
                    format!("正在重建第 {} 章记忆...", chapter_num).yellow()
                );
                let chapter_text = fs::read_to_string(&chapter_path).with_context(|| {
                    format!("failed to read chapter file: {}", chapter_path.display())
                })?;
                memory_agent
                    .summarize_chapter(chapter_num, &chapter_text, &db)
                    .await?;
            }

            println!(
                "{} {}",
                "[✅]".green().bold(),
                "记忆库彻底重建完成！所有人物状态和历史摘要已与本地文件完美同步。"
                    .green()
            );
        }
        Commands::Char { action } => {
            let db = init_db()?;
            match action {
                CharAction::List => {
                    let characters = db.get_all_characters()?;
                    let mut table = Table::new();
                    table
                        .load_preset(UTF8_FULL)
                        .set_content_arrangement(ContentArrangement::Dynamic)
                        .set_header(vec!["ID", "姓名", "设定", "当前状态"]);

                    for (id, name, description, status) in characters {
                        table.add_row(vec![id.to_string(), name, description, status]);
                    }

                    println!("{table}");
                }
                CharAction::Add { name, desc, status } => {
                    db.add_or_update_character(&name, &desc, &status)?;
                    println!(
                        "{} {}",
                        "[✅]".green().bold(),
                        format!("角色 [{}] 已写入，当前状态：{}", name, status).green()
                    );
                }
                CharAction::Kill { name } => {
                    let status = "已死亡";
                    db.update_character_status(&name, status)?;
                    println!(
                        "{} {}",
                        "[✅]".green().bold(),
                        format!("角色 [{}] 已被标记为：{}", name, status).green()
                    );
                }
            }
        }
        Commands::Lore { action } => {
            let db = init_db()?;
            match action {
                LoreAction::List => {
                    let lores = db.get_all_lores()?;
                    let mut table = Table::new();
                    table
                        .load_preset(UTF8_FULL)
                        .set_content_arrangement(ContentArrangement::Dynamic)
                        .set_header(vec!["ID", "分类", "设定"]);

                    for (id, category, description) in lores {
                        table.add_row(vec![id.to_string(), category, description]);
                    }

                    println!("{table}");
                }
                LoreAction::Add { category, desc } => {
                    db.add_lore(&category, &desc)?;
                    println!(
                        "{} {}",
                        "[✅]".green().bold(),
                        format!("世界观设定 [{}] 已写入。", category).green()
                    );
                }
            }
        }
        Commands::Export { output } => {
            let chapters_dir = Path::new(CHAPTERS_DIR);
            if !chapters_dir.exists() {
                return Err(anyhow!("chapters 文件夹不存在：{}", CHAPTERS_DIR));
            }

            let chapter_files = collect_chapter_files(chapters_dir)?;
            let exported_count = chapter_files.len();

            if output.ends_with(".md") {
                let mut output_file = fs::File::create(&output)
                    .with_context(|| format!("failed to create output file: {output}"))?;

                for (chapter_num, chapter_path) in &chapter_files {
                    let chapter_text = fs::read_to_string(chapter_path).with_context(|| {
                        format!("failed to read chapter file: {}", chapter_path.display())
                    })?;
                    write!(output_file, "## 第 {} 章\n\n", chapter_num)
                        .with_context(|| format!("failed to write chapter header: {output}"))?;
                    write!(output_file, "{}\n\n---\n\n", chapter_text)
                        .with_context(|| format!("failed to write chapter content: {output}"))?;
                }

                println!(
                    "{} {}",
                    "[✅]".green().bold(),
                    format!("成功导出 {} 章，共计打包至：{}", exported_count, output).green()
                );
            } else if output.ends_with(".epub") {
                let mut builder = EpubBuilder::new(ZipLibrary::new().map_err(|err| anyhow!(err.to_string()))?)
                    .map_err(|err| anyhow!(err.to_string()))?;
                let title = Path::new(&output)
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("全书导出")
                    .to_string();
                builder
                    .metadata("title", &title)
                    .map_err(|err| anyhow!(err.to_string()))?;
                builder
                    .metadata("author", "Novel Agent")
                    .map_err(|err| anyhow!(err.to_string()))?;

                for (chapter_num, chapter_path) in &chapter_files {
                    let chapter_text = fs::read_to_string(chapter_path).with_context(|| {
                        format!("failed to read chapter file: {}", chapter_path.display())
                    })?;
                    let wrapped_paragraphs = chapter_text
                        .split("\n\n")
                        .flat_map(|block| block.split('\n'))
                        .map(str::trim)
                        .filter(|paragraph| !paragraph.is_empty())
                        .map(|paragraph| format!("<p>{}</p>", paragraph))
                        .collect::<String>();
                    let xhtml_content = format!(
                        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><html xmlns=\"http://www.w3.org/1999/xhtml\"><head><title>第 {} 章</title></head><body><h1>第 {} 章</h1>{}</body></html>",
                        chapter_num, chapter_num, wrapped_paragraphs
                    );

                    builder
                        .add_content(
                            EpubContent::new(
                                format!("chapter_{}.xhtml", chapter_num),
                                xhtml_content.as_bytes(),
                            )
                            .title(format!("第 {} 章", chapter_num)),
                        )
                        .map_err(|err| anyhow!(err.to_string()))?;
                }

                let mut file = fs::File::create(&output)
                    .with_context(|| format!("failed to create output file: {output}"))?;
                builder
                    .generate(&mut file)
                    .map_err(|err| anyhow!(err.to_string()))?;

                println!(
                    "{} {}",
                    "[📚]".green().bold(),
                    format!(
                        "魔法完成！已成功将 {} 章打包为精美的 EPUB 电子书：{}",
                        exported_count, output
                    )
                    .green()
                );
            } else {
                return Err(anyhow!("仅支持导出 .md 或 .epub 文件：{}", output));
            }
        }
        Commands::Write {
            chapter_num,
            requirement,
        } => {
            let db = init_db()?;
            let (writer_agent, memory_agent) = init_writer_and_memory_agents()?;
            db.delete_future_memories(chapter_num)?;
            println!(
                "{} {}",
                "[!]".yellow().bold(),
                format!(
                    "检测到重新生成，已自动清理第 {} 章及之后的旧记忆残留...",
                    chapter_num
                )
                .yellow()
            );

            println!(
                "{} {}",
                "[⏳]".yellow().bold(),
                format!("正在撰写第 {} 章...", chapter_num).yellow()
            );

            let chapter_text = writer_agent
                .write_chapter(chapter_num, requirement.as_deref(), 1, 1, &db)
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
        Commands::Continue {
            start_chapter,
            end_chapter,
            requirement,
        } => {
            run_chapter_range(start_chapter, end_chapter, requirement, "continue").await?;
        }
        Commands::BatchWrite {
            start_chapter,
            end_chapter,
            requirement,
        } => {
            run_chapter_range(start_chapter, end_chapter, requirement, "batch-write").await?;
        }
    }

    Ok(())
}
