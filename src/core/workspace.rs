use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

const NOVELS_DIR_NAME: &str = "novels";
const OUTLINE_FILE_NAME: &str = "outline.txt";
const CHAPTERS_DIR_NAME: &str = "chapters";
const DB_FILE_NAME: &str = "memory.db";

#[derive(Clone, Debug)]
pub struct Workspace {
    base_dir: PathBuf,
    novel_name: String,
}

impl Workspace {
    pub fn new(novel_name: &str) -> Result<Self> {
        if novel_name.trim().is_empty() {
            bail!("novel name must not be empty");
        }
        let base_dir = Self::resolve_base_dir(novel_name)?;
        Self::ensure_dirs(&base_dir)?;
        Ok(Self {
            base_dir,
            novel_name: novel_name.to_string(),
        })
    }

    fn resolve_base_dir(novel_name: &str) -> Result<PathBuf> {
        let home = std::env::var("HOME").or_else(|_| std::env::var("USERPROFILE"))
            .with_context(|| "failed to get home directory")?;
        Ok(PathBuf::from(home)
            .join(".novel-agent")
            .join(NOVELS_DIR_NAME)
            .join(novel_name))
    }

    fn ensure_dirs(base_dir: &Path) -> Result<()> {
        let chapters_dir = base_dir.join(CHAPTERS_DIR_NAME);
        fs::create_dir_all(&chapters_dir)
            .with_context(|| format!("failed to create chapters directory: {}", chapters_dir.display()))?;
        Ok(())
    }

    pub fn novel_name(&self) -> &str {
        &self.novel_name
    }

    pub fn outline_path(&self) -> PathBuf {
        self.base_dir.join(OUTLINE_FILE_NAME)
    }

    pub fn chapters_dir(&self) -> PathBuf {
        self.base_dir.join(CHAPTERS_DIR_NAME)
    }

    pub fn db_path(&self) -> PathBuf {
        self.base_dir.join(DB_FILE_NAME)
    }
}
