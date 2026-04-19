use anyhow::{Context, Result};
use colored::Colorize;
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use tokio::time::{sleep, Duration};

use crate::config::AgentConfig;

const CHAT_COMPLETIONS_PATH: &str = "/chat/completions";

#[derive(Debug, Clone)]
pub struct LlmClient {
    http_client: Client,
    api_base: String,
    api_key: Option<String>,
    model: String,
    temperature: f32,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequest {
    model: String,
    temperature: f32,
    messages: Vec<ChatCompletionRequestMessage>,
    #[serde(skip_serializing_if = "Option::is_none")]
    response_format: Option<ResponseFormat>,
}

#[derive(Debug, Serialize)]
struct ResponseFormat {
    #[serde(rename = "type")]
    r#type: String,
}

#[derive(Debug, Serialize)]
struct ChatCompletionRequestMessage {
    role: String,
    content: String,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponse {
    choices: Vec<ChatChoice>,
}

#[derive(Debug, Deserialize)]
struct ChatChoice {
    message: ChatCompletionResponseMessage,
}

#[derive(Debug, Deserialize)]
struct ChatCompletionResponseMessage {
    content: Option<String>,
}

impl LlmClient {
    pub fn from_config(config: &AgentConfig) -> Result<Self> {
        let api_base = config
            .api_base
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.trim_end_matches('/').to_string())
            .context(
                "agent config is missing api_base; set api_base in config.toml for this provider",
            )?;

        let http_client = Client::builder()
            .timeout(Duration::from_secs(600))
            .connect_timeout(Duration::from_secs(120))
            .build()
            .context("failed to build HTTP client for LLM requests")?;

        Ok(Self {
            http_client,
            api_base,
            api_key: config
                .api_key
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
            model: config.model.clone(),
            temperature: config.temperature,
        })
    }

    pub async fn chat(&self, system_prompt: &str, user_prompt: &str) -> Result<String> {
        let endpoint = format!("{}{}", self.api_base, CHAT_COMPLETIONS_PATH);
        let payload = ChatCompletionRequest {
            model: self.model.clone(),
            temperature: self.temperature,
            messages: vec![
                ChatCompletionRequestMessage {
                    role: "system".to_string(),
                    content: system_prompt.to_string(),
                },
                ChatCompletionRequestMessage {
                    role: "user".to_string(),
                    content: user_prompt.to_string(),
                },
            ],
            response_format: Some(ResponseFormat {
                r#type: "json_object".to_string(),
            }),
        };

        let max_retries = 3;

        for attempt in 1..=max_retries {
            let mut request = self
                .http_client
                .post(&endpoint)
                .header(CONTENT_TYPE, "application/json")
                .json(&payload);

            if let Some(api_key) = &self.api_key {
                request = request.header(AUTHORIZATION, format!("Bearer {api_key}"));
            }

            match request.send().await {
                Ok(response) => {
                    let status = response.status();
                    if !status.is_success() {
                        let body = response
                            .text()
                            .await
                            .with_context(|| {
                                format!("failed to read error response body from {endpoint}")
                            })?;
                        let body = body.trim();

                        let error = if body.is_empty() {
                            anyhow::anyhow!(
                                "LLM API returned {status} for {endpoint} with an empty response body"
                            )
                        } else {
                            anyhow::anyhow!("LLM API returned {status} for {endpoint}: {body}")
                        };

                        if attempt < max_retries {
                            println!(
                                "{}",
                                format!(
                                    "[⚠️] LLM 请求超时或失败 (第 {attempt}/{max_retries} 次)，正在准备重试: {error}"
                                )
                                .yellow()
                            );
                            sleep(Duration::from_secs(3)).await;
                            continue;
                        }

                        return Err(error);
                    }

                    let response_body = response
                        .json::<ChatCompletionResponse>()
                        .await
                        .with_context(|| format!("failed to parse LLM response JSON from {endpoint}"))?;

                    let content = response_body
                        .choices
                        .into_iter()
                        .next()
                        .and_then(|choice| choice.message.content)
                        .map(|content| content.trim().to_string())
                        .filter(|content| !content.is_empty())
                        .with_context(|| {
                            format!(
                                "LLM response from {endpoint} did not contain a valid assistant message"
                            )
                        })?;

                    return Ok(content);
                }
                Err(error) => {
                    if attempt < max_retries {
                        println!(
                            "{}",
                            format!(
                                "[⚠️] LLM 请求超时或失败 (第 {attempt}/{max_retries} 次)，正在准备重试: {error}"
                            )
                            .yellow()
                        );
                        sleep(Duration::from_secs(3)).await;
                        continue;
                    }

                    return Err(anyhow::anyhow!(
                        "failed to send LLM request to {endpoint} after {max_retries} attempts: {error}"
                    ));
                }
            }
        }

        unreachable!("retry loop should always return or continue")
    }
}
