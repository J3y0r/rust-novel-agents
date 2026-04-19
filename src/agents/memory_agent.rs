use anyhow::{bail, Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};
use serde::Deserialize;
use std::time::Duration;

use crate::agents::{Agent, BaseAgent};
use crate::config::AgentConfig;
use crate::core::memory_db::{
    ExtractedChapterSummary, ExtractedCharacter, ExtractedWorldSetting, MemoryDb,
    MemoryExtractionBatch, UpsertOutcome,
};

const OUTLINE_EXTRACTION_PROMPT_TEMPLATE: &str = r#"
你是一个小说长期记忆提取器。
请分析下面提供的大纲文本，只输出严格 JSON，不要输出 Markdown，不要输出解释。
JSON 结构必须如下：
{
  "characters": [
    {
      "name": "人物名",
      "description": "人物描述",
      "status": "当前状态"
    }
  ],
  "world_settings": [
    {
      "category": "分类",
      "description": "设定描述"
    }
  ],
  "chapter_summary": null
}
要求：
1. 如果没有可提取的人物或世界观信息，对应字段返回空数组。
2. chapter_summary 必须返回 null。
3. 所有字段必须存在，键名必须与上面完全一致。
4. 只提取文本中明确出现的信息，不要编造。

待分析文本：
{text}
"#;

const OUTLINE_SYNC_PROMPT_TEMPLATE: &str = r#"
这是用户确认后的最新小说大纲。
请提取出所有出场的人物设定、核心世界观设定。
请严格以 JSON 对象格式返回，只允许包含 characters 和 world_settings 两个字段，不要输出 Markdown，不要输出解释。

JSON 结构必须如下：
{
  "characters": [
    {
      "name": "人物名",
      "description": "人物描述",
      "status": "当前状态"
    }
  ],
  "world_settings": [
    {
      "category": "分类",
      "description": "设定描述"
    }
  ]
}

要求：
1. 如果没有可提取的人物或世界观信息，对应字段返回空数组。
2. 所有字段必须存在，键名必须与上面完全一致。
3. 只提取文本中明确出现的信息，不要编造。

待分析大纲：
{text}
"#;

const CHAPTER_EXTRACTION_PROMPT_TEMPLATE: &str = r#"
你是一个小说长期记忆提取器。
请分析下面提供的章节正文，只输出严格 JSON，不要输出 Markdown，不要输出解释。
JSON 结构必须如下：
{
  "characters": [
    {
      "name": "人物名",
      "description": "人物描述",
      "status": "当前状态"
    }
  ],
  "world_settings": [
    {
      "category": "分类",
      "description": "设定描述"
    }
  ],
  "chapter_summary": {
    "summary": "本章摘要"
  }
}
要求：
1. 如果没有可提取的人物或世界观变化，对应字段返回空数组。
2. 如果无法形成章节摘要，则 chapter_summary 返回 null。
3. 不要输出 chapter_num，章节号由系统单独处理。
4. 所有字段必须存在，键名必须与上面完全一致。
5. 只提取文本中明确出现的信息，不要编造。

待分析文本：
{text}
"#;

const CHAPTER_SUMMARY_PROMPT_TEMPLATE: &str = r#"
请阅读这篇刚刚写好的小说章节（第 {chapter_num} 章）。
【任务】：1. 写出100字以内的剧情摘要。2. 提取本章发生状态变化的人物，以及本章全新登场的人物。
【JSON 格式要求】：请严格返回如下 JSON：
{
  "summary": "...",
  "character_updates": [
    {
      "name": "人物名",
      "status": "最新状态（如：活跃、重伤、死亡）",
      "description": "如果是新登场人物，请简短概括其身份/外貌；如果是老人物，可留空"
    }
  ]
}

要求：
1. 只返回 JSON，不要输出 Markdown，不要输出解释。
2. summary 必须是单个字符串。
3. character_updates 中只包含本章状态确实发生变化的人物，以及本章全新登场的人物。
4. 如果没有可更新人物，character_updates 返回空数组。

章节正文：
{text}
"#;

const SPINNER_TEMPLATE: &str = "{spinner} {msg}";
const MAX_ERROR_SNIPPET_CHARS: usize = 400;

#[derive(Debug, Clone)]
pub struct MemoryAgent {
    base: BaseAgent,
}

#[derive(Debug, Deserialize)]
struct OutlineExtractionResponse {
    characters: Vec<MemoryExtractionCharacter>,
    world_settings: Vec<MemoryExtractionWorldSetting>,
}

#[derive(Debug, Deserialize)]
struct ChapterExtractionResponse {
    characters: Vec<MemoryExtractionCharacter>,
    world_settings: Vec<MemoryExtractionWorldSetting>,
    chapter_summary: Option<MemoryExtractionChapterSummary>,
}

#[derive(Debug, Deserialize)]
struct ChapterSummaryResponse {
    summary: String,
    character_updates: Vec<CharacterUpdate>,
}

#[derive(Debug, Deserialize)]
struct MemoryExtractionCharacter {
    name: String,
    description: String,
    status: String,
}

#[derive(Debug, Deserialize)]
struct MemoryExtractionWorldSetting {
    category: String,
    description: String,
}

#[derive(Debug, Deserialize)]
struct MemoryExtractionChapterSummary {
    summary: String,
}

#[derive(Debug, Deserialize)]
struct CharacterUpdate {
    name: String,
    status: String,
    #[serde(default)]
    description: String,
}

#[derive(Default)]
struct SyncStats {
    inserted_characters: usize,
    updated_characters: usize,
    inserted_world_settings: usize,
    unchanged_world_settings: usize,
}

impl MemoryAgent {
    pub fn new(config: AgentConfig) -> Result<Self> {
        Ok(Self {
            base: BaseAgent::new("memory_agent", config)?,
        })
    }

    pub async fn extract_and_save_outline(&self, text: &str, db: &MemoryDb) -> Result<()> {
        self.save_outline_extraction(text, db).await
    }

    pub async fn sync_from_outline(&self, outline_text: &str, db: &MemoryDb) -> Result<()> {
        let spinner = self.start_spinner("正在从 outline.txt 同步记忆到数据库...");
        let prompt = OUTLINE_SYNC_PROMPT_TEMPLATE.replace("{text}", outline_text);
        let raw_response = self
            .run(&prompt)
            .await
            .context("memory agent failed to sync outline memory")?;

        spinner.set_message("正在解析记忆提取结果...");
        let json_payload = extract_json_object(&raw_response)?;
        let extraction = serde_json::from_str::<OutlineExtractionResponse>(&json_payload)
            .with_context(|| {
                format!(
                    "memory agent returned invalid outline sync JSON: {}",
                    truncate_for_error(&json_payload)
                )
            })?;

        spinner.set_message("正在合并记忆到 SQLite...");
        let stats = self.merge_outline_extraction(extraction, db)?;
        spinner.finish_and_clear();

        println!(
            "{} {} {} {}",
            "[Memory]".green().bold(),
            "人物：新增".green(),
            stats.inserted_characters.to_string().green().bold(),
            format!("，更新 {}", stats.updated_characters).green()
        );
        println!(
            "{} {} {} {}",
            "[Memory]".cyan().bold(),
            "设定：新增".cyan(),
            stats.inserted_world_settings.to_string().cyan().bold(),
            format!("，已存在 {}", stats.unchanged_world_settings).cyan()
        );

        Ok(())
    }

    pub async fn extract_and_save_chapter(
        &self,
        text: &str,
        chapter_num: i64,
        db: &MemoryDb,
    ) -> Result<()> {
        let prompt = CHAPTER_EXTRACTION_PROMPT_TEMPLATE.replace("{text}", text);
        let raw_response = self
            .run(&prompt)
            .await
            .context("memory agent failed to extract chapter memory")?;

        let json_payload = extract_json_object(&raw_response)?;
        let extraction = serde_json::from_str::<ChapterExtractionResponse>(&json_payload)
            .with_context(|| {
                format!(
                    "memory agent returned invalid chapter extraction JSON: {}",
                    truncate_for_error(&json_payload)
                )
            })?;

        db.save_extraction(&MemoryExtractionBatch {
            characters: extraction
                .characters
                .into_iter()
                .map(|character| ExtractedCharacter {
                    name: character.name,
                    description: character.description,
                    status: character.status,
                })
                .collect(),
            world_settings: extraction
                .world_settings
                .into_iter()
                .map(|world_setting| ExtractedWorldSetting {
                    category: world_setting.category,
                    description: world_setting.description,
                })
                .collect(),
            chapter_summary: extraction.chapter_summary.map(|chapter_summary| {
                ExtractedChapterSummary {
                    chapter_num,
                    summary: chapter_summary.summary,
                }
            }),
        })?;

        self.summarize_chapter(chapter_num as u32, text, db).await
    }

    pub async fn summarize_chapter(
        &self,
        chapter_num: u32,
        chapter_text: &str,
        db: &MemoryDb,
    ) -> Result<()> {
        println!(
            "{} {}",
            "[Memory]".yellow().bold(),
            format!("正在生成第 {} 章摘要并更新人物状态...", chapter_num).yellow()
        );

        let prompt = CHAPTER_SUMMARY_PROMPT_TEMPLATE
            .replace("{chapter_num}", &chapter_num.to_string())
            .replace("{text}", chapter_text);
        let raw_response = self
            .run(&prompt)
            .await
            .context("memory agent failed to summarize chapter")?;

        let json_payload = extract_json_object(&raw_response)?;
        let summary_response = serde_json::from_str::<ChapterSummaryResponse>(&json_payload)
            .with_context(|| {
                format!(
                    "memory agent returned invalid chapter summary JSON: {}",
                    truncate_for_error(&json_payload)
                )
            })?;

        db.upsert_chapter_summary(chapter_num, summary_response.summary.trim())?;

        for update in summary_response.character_updates {
            if let Err(err) = db.upsert_character_from_summary(
                &update.name,
                &update.description,
                &update.status,
            ) {
                eprintln!(
                    "[Memory] failed to upsert character from chapter summary: {} ({})",
                    update.name, err
                );
            }
        }

        Ok(())
    }

    pub fn build_context_prompt(&self, db: &MemoryDb) -> Result<String> {
        let snapshot = db.load_all_memory()?;
        let mut sections = Vec::new();

        let characters = if snapshot.characters.is_empty() {
            "- None yet".to_string()
        } else {
            snapshot
                .characters
                .into_iter()
                .map(|character| match character.location {
                    Some(location) if !location.trim().is_empty() => format!(
                        "- **{}**\n  - Description: {}\n  - Status: {}\n  - Location: {}",
                        character.name, character.description, character.status, location
                    ),
                    _ => format!(
                        "- **{}**\n  - Description: {}\n  - Status: {}",
                        character.name, character.description, character.status
                    ),
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        sections.push(format!("## Characters\n{}", characters));

        let world_settings = if snapshot.world_settings.is_empty() {
            "- None yet".to_string()
        } else {
            snapshot
                .world_settings
                .into_iter()
                .map(|setting| format!("- **{}**: {}", setting.category, setting.description))
                .collect::<Vec<_>>()
                .join("\n")
        };
        sections.push(format!("## World Settings\n{}", world_settings));

        let chapter_summaries = if snapshot.chapter_summaries.is_empty() {
            "- None yet".to_string()
        } else {
            snapshot
                .chapter_summaries
                .into_iter()
                .map(|chapter| format!("- Chapter {}: {}", chapter.chapter_num, chapter.summary))
                .collect::<Vec<_>>()
                .join("\n")
        };
        sections.push(format!("## Chapter Summaries\n{}", chapter_summaries));

        Ok(sections.join("\n\n"))
    }

    async fn save_outline_extraction(&self, text: &str, db: &MemoryDb) -> Result<()> {
        let prompt = OUTLINE_EXTRACTION_PROMPT_TEMPLATE.replace("{text}", text);
        let raw_response = self
            .run(&prompt)
            .await
            .context("memory agent failed to extract outline memory")?;

        let json_payload = extract_json_object(&raw_response)?;
        let extraction = serde_json::from_str::<OutlineExtractionResponse>(&json_payload)
            .with_context(|| {
                format!(
                    "memory agent returned invalid outline extraction JSON: {}",
                    truncate_for_error(&json_payload)
                )
            })?;

        db.save_extraction(&MemoryExtractionBatch {
            characters: extraction
                .characters
                .into_iter()
                .map(|character| ExtractedCharacter {
                    name: character.name,
                    description: character.description,
                    status: character.status,
                })
                .collect(),
            world_settings: extraction
                .world_settings
                .into_iter()
                .map(|world_setting| ExtractedWorldSetting {
                    category: world_setting.category,
                    description: world_setting.description,
                })
                .collect(),
            chapter_summary: None,
        })
    }

    fn merge_outline_extraction(
        &self,
        extraction: OutlineExtractionResponse,
        db: &MemoryDb,
    ) -> Result<SyncStats> {
        let mut stats = SyncStats::default();

        for character in extraction.characters {
            match db.upsert_character_with_outcome(
                &character.name,
                &character.description,
                &character.status,
            )? {
                UpsertOutcome::Inserted => stats.inserted_characters += 1,
                UpsertOutcome::Updated => stats.updated_characters += 1,
                UpsertOutcome::Unchanged => {}
            }
        }

        for world_setting in extraction.world_settings {
            match db.upsert_world_setting_with_outcome(
                &world_setting.category,
                &world_setting.description,
            )? {
                UpsertOutcome::Inserted => stats.inserted_world_settings += 1,
                UpsertOutcome::Updated => {
                    unreachable!("world settings do not update in multi-row mode")
                }
                UpsertOutcome::Unchanged => stats.unchanged_world_settings += 1,
            }
        }

        Ok(stats)
    }

    fn start_spinner(&self, message: &str) -> ProgressBar {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::with_template(SPINNER_TEMPLATE)
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        spinner.set_message(message.to_string());
        spinner.enable_steady_tick(Duration::from_millis(120));
        spinner
    }
}

impl Agent for MemoryAgent {
    fn name(&self) -> &str {
        self.base.name()
    }

    fn run<'a>(
        &'a self,
        user_prompt: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<String>> + Send + 'a>> {
        self.base.run(user_prompt)
    }
}

fn extract_json_object(raw_response: &str) -> Result<String> {
    let trimmed = raw_response.trim();
    if trimmed.starts_with('{') && trimmed.ends_with('}') {
        return Ok(trimmed.to_string());
    }

    if let Some(fenced) = extract_fenced_json(trimmed) {
        return Ok(fenced);
    }

    if let Some((start, end)) = find_outer_json_object(trimmed) {
        return Ok(trimmed[start..=end].to_string());
    }

    bail!(
        "memory agent did not return a recognizable JSON object: {}",
        truncate_for_error(trimmed)
    )
}

fn extract_fenced_json(raw_response: &str) -> Option<String> {
    let stripped = raw_response.strip_prefix("```json")?;
    let stripped = stripped.trim();
    let stripped = stripped.strip_suffix("```")?;
    Some(stripped.trim().to_string())
}

fn find_outer_json_object(raw_response: &str) -> Option<(usize, usize)> {
    let mut depth = 0usize;
    let mut start = None;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in raw_response.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }

            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(idx);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }

                depth -= 1;
                if depth == 0 {
                    return start.map(|start_idx| (start_idx, idx));
                }
            }
            _ => {}
        }
    }

    None
}

fn truncate_for_error(value: &str) -> String {
    let truncated: String = value.chars().take(MAX_ERROR_SNIPPET_CHARS).collect();
    if value.chars().count() > MAX_ERROR_SNIPPET_CHARS {
        format!("{truncated}...")
    } else {
        truncated
    }
}
