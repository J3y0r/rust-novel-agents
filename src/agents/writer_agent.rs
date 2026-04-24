use std::fs;

use anyhow::{anyhow, Context, Result};
use colored::Colorize;

use crate::agents::{Agent, BaseAgent};
use crate::config::AgentConfig;
use crate::core::memory_db::MemoryDb;

const OUTLINE_FILE_PATH: &str = "outline.txt";
const CHAPTERS_DIR: &str = "chapters";
const WRITER_PROMPT_TEMPLATE: &str = r#"
[系统设定]
你是专业网文主笔，负责严格遵循既定大纲、长期记忆和历史剧情，创作连贯、有冲突推进和追读钩子的中文网文正文。

[长期记忆]
{long_term_memory}

[全书大纲]
{outline_content}

[前情提要]
{history_summary}

[剧情节奏控制（Pacing）]
{pacing_guidance}

[本章任务]
请撰写第 {chapter_num} 章。

{requirement_instruction}

[输出要求]
1. 直接输出小说正文，不要附加解释、标题说明或创作备注。
2. 严格遵循全书大纲、人物当前状态、世界观设定和历史剧情，不得与本地 outline.txt 冲突。
3. 优先延续“前情提要”中的最近剧情因果与人物状态，保证承接自然。
4. 强调网文节奏，包含有效推进、冲突、人物互动和章节结尾钩子。
5. 本章内容必须围绕本章任务展开，不能偏题。
6. 使用自然流畅的中文叙事风格。
"#;

#[derive(Debug, Clone)]
pub struct WriterAgent {
    base: BaseAgent,
}

impl WriterAgent {
    pub fn new(config: AgentConfig) -> Result<Self> {
        Ok(Self {
            base: BaseAgent::new("writer_agent", config)?,
        })
    }

    pub async fn write_chapter(
        &self,
        chapter_num: u32,
        requirement: Option<&str>,
        arc_current_index: u32,
        arc_total_chapters: u32,
        db: &MemoryDb,
    ) -> Result<String> {
        let prompt = self.build_writer_prompt(
            chapter_num,
            requirement,
            arc_current_index,
            arc_total_chapters,
            db,
        )?;

        println!();
        println!(
            "{}",
            format!(
                "================ 第 {} 章正文 ================",
                chapter_num
            )
            .bright_magenta()
            .bold()
        );

        let mut chapter_text = String::new();
        self.base
            .run_stream(&prompt, |token| {
                print!("{}", token.bright_white());
                std::io::Write::flush(&mut std::io::stdout()).ok();
                chapter_text.push_str(token);
            })
            .await
            .context("writer agent failed to generate chapter")?;

        println!();
        println!();

        self.save_chapter(chapter_num, &chapter_text)?;

        Ok(chapter_text)
    }

    fn build_writer_prompt(
        &self,
        chapter_num: u32,
        requirement: Option<&str>,
        arc_current_index: u32,
        arc_total_chapters: u32,
        db: &MemoryDb,
    ) -> Result<String> {
        let outline_content = self.read_outline_content()?;
        let long_term_memory = self.build_long_term_memory_prompt(db)?;
        let history_summary = db.get_recent_summaries(3, chapter_num)?;
        let trimmed_requirement = requirement.and_then(|req| {
            let trimmed = req.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed)
            }
        });
        let requirement_instruction = match trimmed_requirement {
            Some(req) => format!(
                "【本章核心定制要求】：用户特别指定本章必须写这些内容：{}。请你务必围绕这个核心要求来安排剧情！",
                req
            ),
            None => "【本章核心定制要求】：用户未提供特定细节，请根据大纲和前情提要，自然地推进剧情。"
                .to_string(),
        };
        let pacing_guidance = self.build_pacing_guidance(
            trimmed_requirement.unwrap_or("自然推进剧情"),
            arc_current_index,
            arc_total_chapters,
        );

        Ok(WRITER_PROMPT_TEMPLATE
            .replace("{long_term_memory}", &long_term_memory)
            .replace("{outline_content}", outline_content.trim())
            .replace("{history_summary}", &history_summary)
            .replace("{pacing_guidance}", &pacing_guidance)
            .replace("{chapter_num}", &chapter_num.to_string())
            .replace("{requirement_instruction}", &requirement_instruction))
    }

    fn build_pacing_guidance(
        &self,
        requirement: &str,
        arc_current_index: u32,
        arc_total_chapters: u32,
    ) -> String {
        if arc_total_chapters <= 1 {
            return "本次任务为单章写作，不涉及跨章拆分。请在本章内完整推进用户要求的剧情，同时保留自然的章节钩子。"
                .to_string();
        }

        let mut guidance = format!(
            "用户要求的大剧情是【{}】。本次大剧情共分为 {} 章，你现在正在撰写其中的第 {} 章。",
            requirement, arc_total_chapters, arc_current_index
        );

        let stage_guidance = if arc_current_index == 1 {
            "这是剧情的开端，请注重环境渲染和矛盾铺垫，绝对不要在本章解决核心冲突，请在结尾留下强烈悬念。"
        } else if arc_current_index < arc_total_chapters {
            "这是剧情的中段，请将冲突推向高潮，保持剧情张力，结尾留置下文。"
        } else {
            "这是本段剧情的收尾，请解决核心冲突，安排合理的爽点爆发，并为下一个大事件做好过渡。"
        };

        guidance.push('\n');
        guidance.push_str(stage_guidance);
        guidance
    }

    fn build_long_term_memory_prompt(&self, db: &MemoryDb) -> Result<String> {
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

    fn read_outline_content(&self) -> Result<String> {
        match fs::read_to_string(OUTLINE_FILE_PATH) {
            Ok(content) => Ok(content),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Err(anyhow!(
                "未找到 outline.txt，请先运行 novel outline 生成或手动创建大纲"
            )),
            Err(error) => Err(error)
                .with_context(|| format!("failed to read outline file: {OUTLINE_FILE_PATH}")),
        }
    }

    fn print_chapter(&self, chapter_num: u32, chapter_text: &str) {
        println!();
        println!(
            "{}",
            format!(
                "================ 第 {} 章正文 ================",
                chapter_num
            )
            .bright_magenta()
            .bold()
        );
        println!("{}", chapter_text.bright_white());
        println!();
    }

    fn save_chapter(&self, chapter_num: u32, chapter_text: &str) -> Result<String> {
        fs::create_dir_all(CHAPTERS_DIR)
            .with_context(|| format!("failed to create chapter directory: {CHAPTERS_DIR}"))?;

        let chapter_path = format!("{CHAPTERS_DIR}/chapter_{chapter_num}.txt");
        fs::write(&chapter_path, chapter_text.as_bytes())
            .with_context(|| format!("failed to write chapter file: {chapter_path}"))?;

        println!(
            "{} {}",
            "[Saved]".green().bold(),
            format!("第 {} 章已成功写入本地 TXT：{}", chapter_num, chapter_path).green()
        );

        Ok(chapter_path)
    }
}

impl Agent for WriterAgent {
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
