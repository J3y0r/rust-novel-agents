# rust-novel-agents

[English](./README.md) | 中文

一个面向长篇 AI 网文创作的 Rust CLI 框架。

`rust-novel-agents` 把写作流程拆成三个职责清晰的 Agent：大纲生成、记忆提取、章节写作，并用 SQLite 持久化故事状态，让后续章节可以基于结构化记忆继续推进，而不是单纯依赖越来越脆弱的长上下文 Prompt。

它想解决的是一个非常具体的问题：**让长篇 AI 小说创作更可控、更可检查，也更不容易在中后期偏离大纲或写崩人物状态。**

## 为什么会有这个项目

很多 AI 写作工具在写单个片段时表现不错，但一旦进入真正的连载或长篇创作，就会越来越不稳定。

常见问题通常包括：

- 人物忘记前文经历，状态前后矛盾
- 写到后面不再遵守最初的大纲
- 世界观设定在若干轮之后逐渐丢失
- 为了续写只能不断把旧正文重新塞进上下文
- 一旦输出跑偏，很难定位到底是哪一层出了问题

这个项目的思路不是让一个模型一次记住所有内容，而是做职责拆分：

- **Outline Agent** 负责生成全局结构
- **Memory Agent** 负责从大纲和章节中提取结构化故事记忆
- **Writer Agent** 负责基于大纲、已存储记忆和最近摘要继续写下一章

最终得到的，不是一个单纯的 Prompt，而是一个更接近小型写作系统的工作流。

## 当前已经实现了什么

目前的实现已经支持一条完整的本地写作闭环：

1. 从一个创意生成大纲
2. 将人物与世界观提取进 SQLite
3. 手动修改大纲
4. 将修订后的大纲重新同步回记忆库
5. 基于大纲 + 长期记忆 + 最近摘要写下一章
6. 将章节摘要和人物状态变化重新写回 SQLite

这条核心链路主要分布在：

- `src/main.rs:32`
- `src/agents/outline_agent.rs`
- `src/agents/memory_agent.rs:202`
- `src/agents/writer_agent.rs:48`

## 核心设计

### 用结构化记忆替代不断膨胀的 Prompt

项目不会把所有历史正文都原样塞回模型上下文，而是把可复用的故事状态存进 SQLite：

- 人物
- 世界观设定
- 最近章节摘要

这样做的好处是，随着故事变长，系统仍然更容易续写，也更容易定位问题。

相关实现：
- `src/core/memory_db.rs`
- `src/agents/memory_agent.rs:191`
- `src/agents/writer_agent.rs:75`

### 手工修改过的大纲才是最终事实来源

生成的大纲会写入 `outline.txt`。你可以直接用编辑器打开它，做大幅修改，再同步回记忆库。

这是一个很重要的设计决策：这个系统不是为了盲目一键生成，而是为了支持**人来掌控最终设定**。

```bash
cargo run -- memory sync
```

同步后，最新的大纲内容会被重新提取并合并进 `memory.db`。

### 每个 Agent 独立配置模型

`config.toml` 中会分别配置三个 Agent：

- `outline_agent`
- `memory_agent`
- `writer_agent`

每个 Agent 都可以独立设置：

- provider
- api_base
- api_key
- model
- system_prompt
- temperature

相关实现：
- `src/config.rs:43`
- `src/cli.rs:147`

### 基于 OpenAI 兼容接口，便于切换服务

底层 LLM 客户端走的是 OpenAI 兼容的 `/chat/completions` 接口。

相关实现：
- `src/core/llm.rs:76`

这意味着，只要服务端暴露兼容的接口形式，就可以比较方便地接入不同供应商或自托管服务。

## 工作流

```text
创意
  ↓
生成大纲
  ↓
手动修改 outline.txt
  ↓
memory sync
  ↓
写章节
  ↓
提取章节摘要与状态变化
  ↓
继续下一章
```

运行过程中会在工作目录生成这些文件：

- `config.toml` — 每个 Agent 的运行时配置
- `outline.txt` — 当前大纲
- `memory.db` — SQLite 记忆库
- `chapters/chapter_<n>.txt` — 生成的章节文件

## 快速开始

### 克隆仓库

```bash
git clone https://github.com/J3y0r/rust-novel-agents.git
cd rust-novel-agents
```

### 编译项目

```bash
cargo build
```

或者直接查看 CLI：

```bash
cargo run -- --help
```

### 首次运行时配置模型

如果根目录下不存在 `config.toml`，程序会在启动时自动进入交互式配置流程。

你需要为三个 Agent 分别配置：

- provider
- api_base
- api_key
- model
- system prompt
- temperature

如果你使用的是第三方服务或自托管模型，请确认它提供 OpenAI 兼容的 chat completions 接口。

## 使用方法

### 生成大纲

```bash
cargo run -- outline "修仙界唯一的现代打工人，靠做 PPT 卷死宗门"
```

附加额外约束：

```bash
cargo run -- outline "修仙界唯一的现代打工人，靠做 PPT 卷死宗门" --requirements "偏轻松迪化风，前期多铺垫宗门生态"
```

执行后会：

- 调用 `outline_agent`
- 将结果写入 `outline.txt`
- 提取人物与世界观并写入 `memory.db`

### 同步手动修改后的大纲

在手动编辑完 `outline.txt` 后执行：

```bash
cargo run -- memory sync
```

执行后会：

- 重新读取 `outline.txt`
- 重新提取人物与世界观
- 把最新设定合并进 `memory.db`

### 写一章正文

```bash
cargo run -- writer 1
```

如果你想给这一章追加额外要求：

```bash
cargo run -- writer 1 --requirement "重点写主角第一次进入宗门议事厅的压迫感，并在结尾留下悬念"
```

执行后会：

- 读取 `outline.txt`
- 从 SQLite 载入长期记忆和最近 3 章摘要
- 调用 `writer_agent`
- 将章节保存到 `chapters/chapter_1.txt`
- 提取本章摘要和人物状态变化并回写 `memory.db`

## 架构概览

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

职责划分：

- `src/main.rs` — 命令分发与顶层流程
- `src/cli.rs` — 交互式配置输入
- `src/config.rs` — 配置加载、校验与默认值
- `src/core/llm.rs` — OpenAI 兼容聊天客户端
- `src/core/memory_db.rs` — 基于 SQLite 的故事记忆存储
- `src/agents/outline_agent.rs` — 大纲生成
- `src/agents/memory_agent.rs` — 记忆提取与同步
- `src/agents/writer_agent.rs` — 章节生成

## 这个项目有意思的地方

这个仓库不是一个单纯包一层模型 API 的小工具。

它更像一个紧凑、可扩展的 Rust AI 应用样板，展示了如何组合这些能力：

- 清晰的多 Agent 边界
- 持久化的结构化记忆
- 本地可检查的工作流
- 可人工介入的文件节点
- 基于统一 API 契约的模型切换能力

如果你关心的不只是“让模型输出一段文本”，而是“如何把 AI 写作做成一个真正可维护的系统”，这个项目会很有扩展价值。

## 当前范围

已经实现的部分：

- 大纲生成
- 大纲到记忆库的同步
- 单章写作
- 章节摘要提取
- 人物状态更新
- 每个 Agent 独立配置

自然的下一步扩展方向包括：

- 批量章节生成
- 跨章节节奏控制
- 更细粒度的人物状态模型
- 基于记忆库的设定一致性检查
- TUI 或 Web 界面
- 更适合作者工作流的导出格式

## 开发

常用命令：

```bash
cargo build
cargo check
cargo test
cargo fmt
cargo run -- --help
```

说明：当前仓库还没有较完整的自动化测试集，因此 `cargo test` 目前更接近编译与回归检查，而不是行为级测试验证。

## 仓库

GitHub: https://github.com/J3y0r/rust-novel-agents

如果这个项目对你有帮助，欢迎提 Issue、发 PR，或者点一个 Star。