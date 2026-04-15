# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Commands
- `cargo build`
- `cargo check`
- `cargo test`
- `cargo test <test_name>`
- `cargo fmt`
- `cargo fmt --check`
- `cargo run`

The repository currently does not contain checked-in Rust tests, so `cargo test` is mainly a compile/regression check until tests are added.

## Runtime files and configuration
- The app loads agent settings from `config.toml` in the repository root.
- Generated runtime artifacts are written into the working directory:
  - `outline.txt`
  - `chapters/chapter_<n>.txt`
  - `memory.db`
- Each agent is configured independently in `config.toml` with provider, API base, API key, model, system prompt, and temperature.
- The LLM client uses an OpenAI-compatible `/chat/completions` endpoint, so alternative providers must expose a compatible API surface.

## Architecture overview
This project is a small Rust CLI for long-form novel generation with three cooperating agents and a SQLite-backed memory store.

### Execution flow
1. `src/main.rs` loads `AppConfig`, opens `memory.db`, constructs the outline, memory, and writer agents, then prompts the user to choose outline mode or chapter-writing mode.
2. In outline mode, `OutlineAgent` builds a prompt from the user idea, extra requirements, and the current long-term memory snapshot, then saves the generated outline to `outline.txt`.
3. After generating the outline, `MemoryAgent` extracts structured character, world, and chapter information from the outline and persists it into SQLite.
4. In writer mode, `WriterAgent` reads `outline.txt`, rebuilds prompt context from the stored memory snapshot, generates chapter text, saves it under `chapters/`, and then asks `MemoryAgent` to persist newly extracted memory back into SQLite.

The core loop is: generate outline -> extract memory -> write chapter with outline + memory snapshot -> extract updated memory again.

### Module responsibilities
- `src/main.rs`: startup wiring and top-level mode dispatch.
- `src/cli.rs`: interactive stdin/stdout prompts for mode selection and required inputs.
- `src/config.rs`: `AppConfig` and `AgentConfig`, config loading, provider enum, and validation.
- `src/agents/mod.rs`: shared `Agent` trait and `BaseAgent`, which wraps the reusable LLM client.
- `src/agents/outline_agent.rs`: outline prompt construction, generation, output display, file persistence, and post-generation memory extraction.
- `src/agents/writer_agent.rs`: chapter prompt construction from outline + memory, chapter generation, file persistence, and post-generation memory extraction.
- `src/agents/memory_agent.rs`: strict JSON extraction pipeline that converts generated text into structured memory records and also formats stored memory back into prompt context.
- `src/core/llm.rs`: provider-agnostic chat client built on `reqwest` against an OpenAI-style API.
- `src/core/memory_db.rs`: SQLite schema plus read/write methods for characters, world settings, and chapter summaries.

## Important implementation details
- `MemoryAgent::build_context_prompt()` is the bridge between persisted SQLite state and the prompts used by creative generation agents.
- `BaseAgent` centralizes shared agent execution so specialized agents mainly differ in prompt construction and local file side effects.
- `MemoryDb::save_extraction()` writes extracted memory in a transaction, so outline/chapter extraction updates multiple memory tables together.
- `writer_agent` depends on `outline.txt` already existing. If chapter writing fails unexpectedly, verify that an outline has been generated first.
