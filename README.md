# rust-novel-agents

English | [中文](./README.zh-CN.md)

A Rust-first CLI framework for long-form AI novel writing.

`rust-novel-agents` splits the writing pipeline into three specialized agents—outline generation, memory extraction, and chapter writing—and persists story state in SQLite so later chapters can continue from structured memory instead of relying on fragile full-context prompting.

It is built for a very specific problem: **making long-form AI fiction writing more controllable, more inspectable, and less likely to drift off-outline or break character state halfway through a book.**

## Why this project exists

Most AI writing workflows work well for a single scene, but become unstable once you try to write a real serial story.

Common failure modes are easy to recognize:

- characters forget earlier events or change state without explanation
- later chapters stop respecting the original outline
- important worldbuilding disappears after enough turns
- the only way to continue is to keep stuffing more raw text back into context
- once the output is wrong, fixing it is tedious because the workflow is opaque

This project takes a different approach.

Instead of asking one model to remember everything at once, it separates responsibilities:

- **Outline Agent** generates the high-level structure
- **Memory Agent** extracts structured story state from outlines and chapters
- **Writer Agent** writes the next chapter using the outline, stored memory, and recent summaries

The result is a workflow that feels closer to a small writing system than a single prompt.

## What it does today

The current implementation already supports a complete local writing workflow:

1. generate an outline from an idea
2. extract characters and world settings into SQLite
3. manually revise the outline if needed
4. sync the revised outline back into memory
5. inspect and patch character/world memory from the CLI
6. write a single chapter or a chapter range from outline + long-term memory + recent summaries
7. rebuild memory from local outline and chapter files when the database needs to be repaired
8. export generated chapters as Markdown or EPUB

That core loop is implemented across:

- `src/main.rs:32`
- `src/agents/outline_agent.rs`
- `src/agents/memory_agent.rs:202`
- `src/agents/writer_agent.rs:48`

## Core ideas

### Structured memory over prompt bloat

Instead of treating every previous chapter as raw context, the project stores reusable story state in SQLite:

- characters
- world settings
- recent chapter summaries

This makes the workflow easier to continue and easier to reason about as the story grows.

Relevant implementation:
- `src/core/memory_db.rs`
- `src/agents/memory_agent.rs:191`
- `src/agents/writer_agent.rs:75`

### Human-edited outline remains the source of truth

The generated outline is written to `outline.txt`, which means you can open it in your editor, rewrite it aggressively, and sync it back into the memory store.

That is an important design choice: the system is not optimized for blind one-click generation, but for **human-controlled iteration**.

```bash
cargo run -- memory sync
```

After syncing, the latest outline content is re-extracted and merged into `memory.db`.

### Per-agent model configuration

Each agent is configured independently in `config.toml`:

- `outline_agent`
- `memory_agent`
- `writer_agent`

Each one can define its own:

- provider
- api_base
- api_key
- model
- system_prompt
- temperature

Relevant implementation:
- `src/config.rs:43`
- `src/cli.rs:147`

### OpenAI-compatible API surface

The LLM client uses an OpenAI-compatible `/chat/completions` endpoint.

Relevant implementation:
- `src/core/llm.rs:76`

That makes it practical to point different agents at different compatible providers or self-hosted services, as long as they expose the expected API shape.

## Workflow

```text
idea
  ↓
outline generation
  ↓
manual outline revision
  ↓
memory sync
  ↓
optional character/lore patching from CLI
  ↓
single chapter writing or batch chapter writing
  ↓
chapter summary + state update extraction
  ↓
export / continue next chapter
```

Runtime files generated in the working directory:

- `config.toml` — per-agent runtime configuration
- `outline.txt` — current outline
- `memory.db` — SQLite memory store
- `chapters/chapter_<n>.txt` — generated chapter files

## Quick start

### Clone the repository

```bash
git clone https://github.com/J3y0r/rust-novel-agents.git
cd rust-novel-agents
```

### Build the project

```bash
cargo build
```

Or inspect the CLI directly:

```bash
cargo run -- --help
```

### Configure models on first run

If `config.toml` does not exist, the app creates it interactively on startup.

You will be prompted to configure all three agents with:

- provider (`openai`, `ollama`, or `anthropic`)
- api_base
- api_key
- model
- system prompt
- temperature

If you are using a third-party or self-hosted model service, make sure it exposes an OpenAI-compatible chat completions endpoint.

## Usage

### Show help

```bash
cargo run -- --help
```

Current top-level commands include:

- `outline`
- `memory sync`
- `memory rebuild`
- `char list | add | kill`
- `lore list | add`
- `write`
- `batch-write`
- `export`

### Generate an outline

```bash
cargo run -- outline "修仙界唯一的现代打工人，靠做 PPT 卷死宗门"
```

With extra constraints:

```bash
cargo run -- outline "修仙界唯一的现代打工人，靠做 PPT 卷死宗门" --requirements "偏轻松迪化风，前期多铺垫宗门生态"
```

This will:

- call `outline_agent`
- write the result to `outline.txt`
- extract characters and world settings into `memory.db`

### Sync a manually edited outline

After editing `outline.txt` by hand:

```bash
cargo run -- memory sync
```

This will:

- reload `outline.txt`
- re-extract characters and world settings
- merge the latest story state into `memory.db`

### Write a chapter

```bash
cargo run -- write 1
```

With chapter-specific instructions:

```bash
cargo run -- write 1 "重点写主角第一次进入宗门议事厅的压迫感，并在结尾留下悬念"
```

This will:

- read `outline.txt`
- load long-term memory and the most recent three chapter summaries from SQLite
- call `writer_agent`
- save the chapter to `chapters/chapter_1.txt`
- extract chapter summary and character status updates back into `memory.db`

### Write a chapter range

```bash
cargo run -- batch-write 10 12 "主角第一次正式参与宗门大比，三章内完成铺垫、爆发和收尾"
```

This will:

- write chapters from `start_chapter` to `end_chapter`
- inject pacing guidance so the shared requirement is split across the whole arc
- automatically clear summaries from the current chapter onward before regenerating
- retry the same chapter interactively if generation or memory extraction fails

### Inspect and patch story memory

List current characters:

```bash
cargo run -- char list
```

Add or update a character:

```bash
cargo run -- char add "林舟" "外门杂役出身，极善观察局势" "活跃"
```

Mark a character as dead:

```bash
cargo run -- char kill "林舟"
```

List world settings:

```bash
cargo run -- lore list
```

Add a world setting:

```bash
cargo run -- lore add "宗门制度" "外门弟子每月考核一次，末位会被降级"
```

### Rebuild memory from local files

```bash
cargo run -- memory rebuild
```

This command clears the existing memory tables, re-imports `outline.txt`, then replays every local chapter file in `chapters/` back into SQLite.

Use it when `memory.db` is stale, corrupted, or no longer matches your local outline and chapter files.

### Export the novel

Export all generated chapters as Markdown:

```bash
cargo run -- export --output 全书导出.md
```

Export as EPUB:

```bash
cargo run -- export --output 全书导出.epub
```

## Architecture

```text
src/
├── agents/
│   ├── memory_agent.rs
│   ├── outline_agent.rs
│   ├── writer_agent.rs
│   └── mod.rs
├── core/
│   ├── llm.rs
│   └── memory_db.rs
├── cli.rs
├── config.rs
└── main.rs
```

Responsibilities:

- `src/main.rs` — command dispatch and top-level flow
- `src/cli.rs` — interactive config prompts
- `src/config.rs` — config loading, validation, and defaults
- `src/core/llm.rs` — OpenAI-compatible chat client
- `src/core/memory_db.rs` — SQLite-backed story memory
- `src/agents/outline_agent.rs` — outline generation
- `src/agents/memory_agent.rs` — memory extraction and synchronization
- `src/agents/writer_agent.rs` — chapter generation

## Why it is interesting

This repository is not just a thin wrapper around a model API.

It is a compact example of how to build a stateful AI application in Rust with:

- a clear multi-agent boundary
- persistent structured memory
- an inspectable local workflow
- file-based human intervention points
- model-provider flexibility behind a shared API contract

If you care about AI tooling beyond chat interfaces, this is exactly the kind of project that becomes more useful as you keep extending it.

## Current scope

What is already implemented:

- outline generation
- outline-to-memory synchronization
- full memory rebuild from local files
- single chapter writing
- batch chapter writing with pacing guidance and retry flow
- chapter summary extraction
- character state updates
- CLI character and lore management
- Markdown / EPUB export
- independent configuration per agent

What would be natural next steps:

- batch chapter generation
- pacing control across chapter ranges
- richer character state models
- consistency checking against stored memory
- TUI or web interface
- export formats for author workflows

## Development

Common commands:

```bash
cargo build
cargo check
cargo test
cargo fmt
cargo run -- --help
```

Note: the repository does not yet include a substantial automated test suite, so `cargo test` is currently closer to a compile/regression check than a behavior-level validation suite.

## Repository

GitHub: https://github.com/J3y0r/rust-novel-agents

If this project is useful to you, feel free to open an issue, send a PR, or give it a star.