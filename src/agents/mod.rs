pub mod memory_agent;
pub mod outline_agent;
pub mod writer_agent;

use std::future::Future;
use std::pin::Pin;

use anyhow::Result;

use crate::config::AgentConfig;
use crate::core::llm::LlmClient;

pub use memory_agent::MemoryAgent;
pub use outline_agent::OutlineAgent;
pub use writer_agent::WriterAgent;

pub trait Agent {
    fn name(&self) -> &str;

    fn run<'a>(
        &'a self,
        user_prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>>;
}

#[derive(Debug, Clone)]
pub struct BaseAgent {
    name: String,
    config: AgentConfig,
    client: LlmClient,
}

impl BaseAgent {
    pub fn new(name: impl Into<String>, config: AgentConfig) -> Result<Self> {
        let client = LlmClient::from_config(&config)?;

        Ok(Self {
            name: name.into(),
            config,
            client,
        })
    }

    pub fn config(&self) -> &AgentConfig {
        &self.config
    }
}

impl BaseAgent {
    pub async fn run_stream<F>(&self, user_prompt: &str, on_token: F) -> Result<String>
    where
        F: FnMut(&str),
    {
        self.client
            .chat_stream(&self.config.system_prompt, user_prompt, on_token)
            .await
    }
}

impl Agent for BaseAgent {
    fn name(&self) -> &str {
        &self.name
    }

    fn run<'a>(
        &'a self,
        user_prompt: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<String>> + Send + 'a>> {
        Box::pin(async move {
            self.client
                .chat(&self.config.system_prompt, user_prompt)
                .await
        })
    }
}
