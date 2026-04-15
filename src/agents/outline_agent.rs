use std::fs;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use colored::Colorize;
use indicatif::{ProgressBar, ProgressStyle};

use crate::agents::{Agent, BaseAgent, MemoryAgent};
use crate::cli;
use crate::config::AgentConfig;
use crate::core::memory_db::MemoryDb;

const OUTLINE_FILE_PATH: &str = "outline.txt";
const SPINNER_TEMPLATE: &str = "{spinner} {msg}";
const OUTLINE_PROMPT_TEMPLATE: &str = r#"
请基于下面信息生成一份详细的中文网文大纲。

## 当前长期记忆
{memory_context}

## 用户创意
{idea}

## 补充要求
{requirements}

## 输出要求
1. 使用清晰的小节标题输出。
2. 至少包含：故事定位、世界观设定、核心人物、主线矛盾、阶段剧情推进、卷级规划、伏笔与爽点设计。
3. 内容要适合连载型网文，强调冲突、升级节奏和持续追读钩子。
4. 只输出大纲正文，不要附加解释。
"#;

#[derive(Debug, Clone)]
pub struct OutlineAgent {
    base: BaseAgent,
}

impl OutlineAgent {
    pub fn new(config: AgentConfig) -> Result<Self> {
        Ok(Self {
            base: BaseAgent::new("outline_agent", config)?,
        })
    }

    pub async fn generate_outline(
        &self,
        idea: &str,
        requirements: &str,
        db: &MemoryDb,
        memory_agent: &MemoryAgent,
    ) -> Result<String> {
        let prompt = self.build_outline_prompt(idea, requirements, db, memory_agent)?;
        let spinner = self.start_spinner();

        let outline = self
            .run(&prompt)
            .await
            .context("outline agent failed to generate outline")?;

        spinner.finish_and_clear();

        self.print_outline(&outline);
        self.save_outline(&outline)?;

        println!(
            "{} {}",
            "[Saved]".green().bold(),
            "大纲已生成至 outline.txt，请手动修改。修改满意后，请运行 `novel memory sync` 更新记忆库。"
                .green()
        );

        Ok(outline)
    }

    fn build_outline_prompt(
        &self,
        idea: &str,
        requirements: &str,
        db: &MemoryDb,
        memory_agent: &MemoryAgent,
    ) -> Result<String> {
        let memory_context = memory_agent.build_context_prompt(db)?;
        Ok(OUTLINE_PROMPT_TEMPLATE
            .replace("{memory_context}", &memory_context)
            .replace("{idea}", idea.trim())
            .replace("{requirements}", requirements.trim()))
    }

    fn start_spinner(&self) -> ProgressBar {
        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::with_template(SPINNER_TEMPLATE)
                .unwrap_or_else(|_| ProgressStyle::default_spinner()),
        );
        spinner.set_message("正在推演大纲...");
        spinner.enable_steady_tick(Duration::from_millis(120));
        spinner
    }

    fn print_outline(&self, outline: &str) {
        println!();
        println!(
            "{}",
            "================ 大纲生成结果 ================"
                .bright_blue()
                .bold()
        );
        println!("{}", outline.bright_white());
        println!();
    }

    fn save_outline(&self, outline: &str) -> Result<()> {
        if fs::metadata(OUTLINE_FILE_PATH).is_ok()
            && !cli::prompt_confirm("outline.txt 已存在，是否覆盖？")?
        {
            bail!("outline generation canceled because outline.txt already exists")
        }

        fs::write(OUTLINE_FILE_PATH, outline.as_bytes())
            .with_context(|| format!("failed to write outline file: {OUTLINE_FILE_PATH}"))?;
        Ok(())
    }
}

impl Agent for OutlineAgent {
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
