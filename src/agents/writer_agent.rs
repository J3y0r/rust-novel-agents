use std::fs;

use anyhow::{anyhow, Context, Result};
use colored::Colorize;

use crate::agents::{Agent, BaseAgent, MemoryAgent};
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

[本章任务]
请撰写第 {chapter_num} 章，要求：{requirement}

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
        requirement: &str,
        db: &MemoryDb,
        memory_agent: &MemoryAgent,
    ) -> Result<String> {
        let prompt = self.build_writer_prompt(chapter_num, requirement, db, memory_agent)?;
        let chapter_text = self
            .run(&prompt)
            .await
            .context("writer agent failed to generate chapter")?;

        self.print_chapter(chapter_num, &chapter_text);
        self.save_chapter(chapter_num, &chapter_text)?;

        Ok(chapter_text)
    }

    fn build_writer_prompt(
        &self,
        chapter_num: u32,
        requirement: &str,
        db: &MemoryDb,
        memory_agent: &MemoryAgent,
    ) -> Result<String> {
        let outline_content = self.read_outline_content()?;
        let long_term_memory = memory_agent.build_context_prompt(db)?;
        let history_summary = db.get_recent_summaries(3)?;
        let chapter_requirement = if requirement.trim().is_empty() {
            "无额外要求"
        } else {
            requirement.trim()
        };

        Ok(WRITER_PROMPT_TEMPLATE
            .replace("{long_term_memory}", &long_term_memory)
            .replace("{outline_content}", outline_content.trim())
            .replace("{history_summary}", &history_summary)
            .replace("{chapter_num}", &chapter_num.to_string())
            .replace("{requirement}", chapter_requirement))
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
