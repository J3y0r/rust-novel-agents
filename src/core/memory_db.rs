use std::cell::RefCell;
use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

const DEFAULT_DB_PATH: &str = "memory.db";
const BUSY_TIMEOUT_SECONDS: u64 = 5;

#[derive(Debug)]
pub struct MemoryDb {
    conn: RefCell<Connection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpsertOutcome {
    Inserted,
    Updated,
    Unchanged,
}

#[derive(Debug, Clone)]
pub struct CharacterMemory {
    pub id: i64,
    pub name: String,
    pub description: String,
    pub status: String,
    pub location: Option<String>,
}

#[derive(Debug, Clone)]
pub struct WorldSettingMemory {
    pub id: i64,
    pub category: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct ChapterSummaryMemory {
    pub chapter_num: i64,
    pub summary: String,
}

#[derive(Debug, Clone, Default)]
pub struct MemorySnapshot {
    pub characters: Vec<CharacterMemory>,
    pub world_settings: Vec<WorldSettingMemory>,
    pub chapter_summaries: Vec<ChapterSummaryMemory>,
}

impl MemoryDb {
    pub fn new() -> Result<Self> {
        Self::open(DEFAULT_DB_PATH)
    }

    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let conn = Connection::open(path)
            .with_context(|| format!("failed to open SQLite database: {}", path.display()))?;

        conn.busy_timeout(Duration::from_secs(BUSY_TIMEOUT_SECONDS))
            .with_context(|| {
                format!(
                    "failed to configure SQLite busy timeout for {}",
                    path.display()
                )
            })?;

        let db = Self {
            conn: RefCell::new(conn),
        };
        db.init_tables()?;
        Ok(db)
    }

    pub fn init_tables(&self) -> Result<()> {
        self.conn
            .borrow()
            .execute_batch(
                "
                CREATE TABLE IF NOT EXISTS characters (
                    id INTEGER PRIMARY KEY,
                    name TEXT UNIQUE,
                    description TEXT,
                    status TEXT,
                    location TEXT
                );

                CREATE TABLE IF NOT EXISTS world_settings (
                    id INTEGER PRIMARY KEY,
                    category TEXT,
                    description TEXT
                );

                CREATE TABLE IF NOT EXISTS chapter_summaries (
                    chapter_num INTEGER PRIMARY KEY,
                    summary TEXT
                );
                ",
            )
            .context("failed to initialize memory tables")?;

        self.conn
            .borrow()
            .execute_batch(
                "
                ALTER TABLE characters ADD COLUMN location TEXT;
                ",
            )
            .ok();

        self.conn
            .borrow()
            .execute_batch(
                "
                DELETE FROM world_settings
                WHERE id NOT IN (
                    SELECT MIN(id)
                    FROM world_settings
                    GROUP BY category, description
                );

                DROP INDEX IF EXISTS idx_world_settings_category;

                CREATE UNIQUE INDEX IF NOT EXISTS idx_world_settings_category_description
                ON world_settings(category, description);
                ",
            )
            .context("failed to normalize world settings table")?;

        Ok(())
    }

    pub fn upsert_character(&self, name: &str, description: &str, status: &str) -> Result<()> {
        self.upsert_character_with_location(name, description, status, None)
            .map(|_| ())
    }

    pub fn upsert_character_with_outcome(
        &self,
        name: &str,
        description: &str,
        status: &str,
    ) -> Result<UpsertOutcome> {
        self.upsert_character_with_location(name, description, status, None)
    }

    pub fn upsert_character_with_location(
        &self,
        name: &str,
        description: &str,
        status: &str,
        location: Option<&str>,
    ) -> Result<UpsertOutcome> {
        let conn = self.conn.borrow();
        let existing = conn
            .query_row(
                "SELECT description, status, location FROM characters WHERE name = ?1",
                params![name],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                    ))
                },
            )
            .optional()
            .with_context(|| format!("failed to query character before upsert: {name}"))?;

        let normalized_location = location.map(str::to_string);
        let outcome = match existing {
            None => UpsertOutcome::Inserted,
            Some((existing_description, existing_status, existing_location))
                if existing_description == description
                    && existing_status == status
                    && (normalized_location.is_none()
                        || existing_location == normalized_location) =>
            {
                UpsertOutcome::Unchanged
            }
            Some(_) => UpsertOutcome::Updated,
        };

        conn.execute(
            "
            INSERT INTO characters (name, description, status, location)
            VALUES (?1, ?2, ?3, ?4)
            ON CONFLICT(name) DO UPDATE SET
                description = excluded.description,
                status = excluded.status,
                location = COALESCE(excluded.location, characters.location)
            ",
            params![name, description, status, location],
        )
        .with_context(|| format!("failed to upsert character: {name}"))?;

        Ok(outcome)
    }

    pub fn upsert_character_from_summary(
        &self,
        name: &str,
        description: &str,
        status: &str,
    ) -> Result<()> {
        self.conn
            .borrow()
            .execute(
                "
                INSERT INTO characters (name, description, status)
                VALUES (?1, ?2, ?3)
                ON CONFLICT(name) DO UPDATE SET status=excluded.status, description=CASE WHEN excluded.description != '' THEN excluded.description ELSE characters.description END;
                ",
                params![name, description, status],
            )
            .with_context(|| format!("failed to upsert character from summary: {name}"))?;

        Ok(())
    }

    pub fn add_or_update_character(
        &self,
        name: &str,
        description: &str,
        status: &str,
    ) -> Result<()> {
        self.upsert_character(name, description, status)
    }

    pub fn get_all_characters(&self) -> Result<Vec<(i32, String, String, String)>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, status FROM characters ORDER BY name COLLATE NOCASE ASC",
            )
            .context("failed to prepare all characters query")?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i32>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                ))
            })
            .context("failed to query all characters")?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to collect all characters")
    }

    pub fn get_all_lores(&self) -> Result<Vec<(i32, String, String)>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare("SELECT id, category, description FROM world_settings ORDER BY id ASC")
            .context("failed to prepare all lores query")?;

        let rows = stmt
            .query_map([], |row| {
                Ok((
                    row.get::<_, i32>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .context("failed to query all lores")?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to collect all lores")
    }

    pub fn add_lore(&self, category: &str, description: &str) -> Result<()> {
        self.upsert_world_setting(category, description)
    }

    pub fn upsert_world_setting(&self, category: &str, description: &str) -> Result<()> {
        self.upsert_world_setting_with_outcome(category, description)
            .map(|_| ())
    }

    pub fn upsert_world_setting_with_outcome(
        &self,
        category: &str,
        description: &str,
    ) -> Result<UpsertOutcome> {
        let conn = self.conn.borrow();
        let existing = conn
            .query_row(
                "SELECT 1 FROM world_settings WHERE category = ?1 AND description = ?2",
                params![category, description],
                |_| Ok(()),
            )
            .optional()
            .with_context(|| format!("failed to query world setting before upsert: {category}"))?;

        let outcome = if existing.is_some() {
            UpsertOutcome::Unchanged
        } else {
            UpsertOutcome::Inserted
        };

        conn.execute(
            "
            INSERT INTO world_settings (category, description)
            VALUES (?1, ?2)
            ON CONFLICT(category, description) DO NOTHING
            ",
            params![category, description],
        )
        .with_context(|| format!("failed to upsert world setting for category: {category}"))?;

        Ok(outcome)
    }

    pub fn upsert_chapter_summary(&self, chapter_num: u32, summary: &str) -> Result<()> {
        self.conn
            .borrow()
            .execute(
                "
                INSERT INTO chapter_summaries (chapter_num, summary)
                VALUES (?1, ?2)
                ON CONFLICT(chapter_num) DO UPDATE SET
                    summary = excluded.summary
                ",
                params![i64::from(chapter_num), summary],
            )
            .with_context(|| format!("failed to upsert chapter summary: chapter {chapter_num}"))?;

        Ok(())
    }

    pub fn get_recent_summaries(&self, limit: u32, current_chapter: u32) -> Result<String> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(
                "SELECT chapter_num, summary FROM chapter_summaries WHERE chapter_num < ?1 ORDER BY chapter_num DESC LIMIT ?2",
            )
            .context("failed to prepare recent summaries query")?;

        let rows = stmt
            .query_map(params![i64::from(current_chapter), i64::from(limit)], |row| {
                Ok(ChapterSummaryMemory {
                    chapter_num: row.get(0)?,
                    summary: row.get(1)?,
                })
            })
            .context("failed to query recent chapter summaries")?;

        let summaries = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to collect recent chapter summaries")?;

        if summaries.is_empty() {
            return Ok("- 暂无历史章节摘要".to_string());
        }

        Ok(summaries
            .into_iter()
            .map(|chapter| format!("- 第 {} 章：{}", chapter.chapter_num, chapter.summary))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    pub fn delete_future_memories(&self, from_chapter: u32) -> Result<()> {
        self.conn
            .borrow()
            .execute(
                "DELETE FROM chapter_summaries WHERE chapter_num >= ?1",
                params![i64::from(from_chapter)],
            )
            .with_context(|| {
                format!(
                    "failed to delete future chapter summaries from chapter {from_chapter}"
                )
            })?;

        Ok(())
    }

    pub fn clear_all_memory(&self) -> Result<()> {
        self.conn
            .borrow()
            .execute_batch(
                "
                DELETE FROM chapter_summaries;
                DELETE FROM world_settings;
                DELETE FROM characters;
                ",
            )
            .context("failed to clear memory tables")?;

        Ok(())
    }

    pub fn clear_all_tables(&self) -> Result<()> {
        self.clear_all_memory()?;
        Ok(())
    }

    pub fn clear_outline_memory(&self) -> Result<()> {
        self.conn
            .borrow()
            .execute_batch(
                "
                DELETE FROM world_settings;
                DELETE FROM characters;
                ",
            )
            .context("failed to clear outline-derived memory tables")?;

        Ok(())
    }

    pub fn list_characters(&self) -> Result<Vec<CharacterMemory>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare(
                "SELECT id, name, description, status, location FROM characters ORDER BY name COLLATE NOCASE ASC",
            )
            .context("failed to prepare characters query")?;

        let rows = stmt
            .query_map([], |row| {
                Ok(CharacterMemory {
                    id: row.get(0)?,
                    name: row.get(1)?,
                    description: row.get(2)?,
                    status: row.get(3)?,
                    location: row.get(4)?,
                })
            })
            .context("failed to query characters")?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to collect characters")
    }

    pub fn list_world_settings(&self) -> Result<Vec<WorldSettingMemory>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare("SELECT id, category, description FROM world_settings ORDER BY id ASC")
            .context("failed to prepare world settings query")?;

        let rows = stmt
            .query_map([], |row| {
                Ok(WorldSettingMemory {
                    id: row.get(0)?,
                    category: row.get(1)?,
                    description: row.get(2)?,
                })
            })
            .context("failed to query world settings")?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to collect world settings")
    }

    pub fn list_chapter_summaries(&self) -> Result<Vec<ChapterSummaryMemory>> {
        let conn = self.conn.borrow();
        let mut stmt = conn
            .prepare("SELECT chapter_num, summary FROM chapter_summaries ORDER BY chapter_num ASC")
            .context("failed to prepare chapter summaries query")?;

        let rows = stmt
            .query_map([], |row| {
                Ok(ChapterSummaryMemory {
                    chapter_num: row.get(0)?,
                    summary: row.get(1)?,
                })
            })
            .context("failed to query chapter summaries")?;

        rows.collect::<rusqlite::Result<Vec<_>>>()
            .context("failed to collect chapter summaries")
    }

    pub fn load_all_memory(&self) -> Result<MemorySnapshot> {
        Ok(MemorySnapshot {
            characters: self.list_characters()?,
            world_settings: self.list_world_settings()?,
            chapter_summaries: self.list_chapter_summaries()?,
        })
    }

    pub fn save_extraction(&self, extraction: &MemoryExtractionBatch) -> Result<()> {
        let mut conn = self.conn.borrow_mut();
        let tx = conn
            .transaction()
            .context("failed to start SQLite transaction for memory save")?;

        for character in &extraction.characters {
            Self::upsert_character_tx(
                &tx,
                &character.name,
                &character.description,
                &character.status,
            )?;
        }

        for world_setting in &extraction.world_settings {
            Self::upsert_world_setting_tx(
                &tx,
                &world_setting.category,
                &world_setting.description,
            )?;
        }

        if let Some(chapter_summary) = &extraction.chapter_summary {
            Self::upsert_chapter_summary_tx(
                &tx,
                chapter_summary.chapter_num,
                &chapter_summary.summary,
            )?;
        }

        tx.commit()
            .context("failed to commit SQLite transaction for memory save")?;

        Ok(())
    }

    fn upsert_character_tx(
        tx: &Transaction<'_>,
        name: &str,
        description: &str,
        status: &str,
    ) -> Result<()> {
        tx.execute(
            "
            INSERT INTO characters (name, description, status, location)
            VALUES (?1, ?2, ?3, NULL)
            ON CONFLICT(name) DO UPDATE SET
                description = excluded.description,
                status = excluded.status,
                location = COALESCE(excluded.location, characters.location)
            ",
            params![name, description, status],
        )
        .with_context(|| format!("failed to upsert character in transaction: {name}"))?;

        Ok(())
    }

    fn upsert_world_setting_tx(
        tx: &Transaction<'_>,
        category: &str,
        description: &str,
    ) -> Result<()> {
        tx.execute(
            "
            INSERT INTO world_settings (category, description)
            VALUES (?1, ?2)
            ON CONFLICT(category, description) DO NOTHING
            ",
            params![category, description],
        )
        .with_context(|| format!("failed to upsert world setting in transaction: {category}"))?;

        Ok(())
    }

    fn upsert_chapter_summary_tx(
        tx: &Transaction<'_>,
        chapter_num: i64,
        summary: &str,
    ) -> Result<()> {
        tx.execute(
            "
            INSERT INTO chapter_summaries (chapter_num, summary)
            VALUES (?1, ?2)
            ON CONFLICT(chapter_num) DO UPDATE SET
                summary = excluded.summary
            ",
            params![chapter_num, summary],
        )
        .with_context(|| {
            format!("failed to upsert chapter summary in transaction: chapter {chapter_num}")
        })?;

        Ok(())
    }
}

#[derive(Debug, Clone, Default)]
pub struct MemoryExtractionBatch {
    pub characters: Vec<ExtractedCharacter>,
    pub world_settings: Vec<ExtractedWorldSetting>,
    pub chapter_summary: Option<ExtractedChapterSummary>,
}

#[derive(Debug, Clone)]
pub struct ExtractedCharacter {
    pub name: String,
    pub description: String,
    pub status: String,
}

#[derive(Debug, Clone)]
pub struct ExtractedWorldSetting {
    pub category: String,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct ExtractedChapterSummary {
    pub chapter_num: i64,
    pub summary: String,
}
