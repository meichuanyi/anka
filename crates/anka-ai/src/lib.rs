//! Minimal OpenAI-compatible chat-completions client.
//!
//! Works with any `{base_url}/chat/completions` endpoint (DeepSeek, Qwen
//! compatible-mode, Kimi, OpenAI, Ollama, …). Blocking on purpose: callers
//! run it inside `spawn_blocking` so the async runtime / UI thread never
//! waits on the network.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AiConfig {
    #[serde(default)]
    pub base_url: String,
    #[serde(default)]
    pub api_key: String,
    #[serde(default)]
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiMessage {
    pub role: String,
    pub content: String,
}

/// POST `{base_url}/chat/completions` and return the first choice's text.
pub fn chat_blocking(cfg: &AiConfig, messages: &[AiMessage]) -> Result<String> {
    if cfg.base_url.is_empty() || cfg.model.is_empty() {
        anyhow::bail!("AI 未配置：请在设置里填写接口地址和模型名");
    }
    let url = format!(
        "{}/chat/completions",
        cfg.base_url.trim_end_matches('/')
    );
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(120))
        .build()
        .context("构建 HTTP 客户端失败")?;
    let mut req = client
        .post(&url)
        .json(&serde_json::json!({ "model": cfg.model, "messages": messages }));
    if !cfg.api_key.is_empty() {
        req = req.bearer_auth(&cfg.api_key);
    }
    let resp = req.send().context("无法连接 AI 接口")?;
    let status = resp.status();
    let body: serde_json::Value = resp.json().context("AI 接口返回的不是 JSON")?;
    if !status.is_success() {
        let msg = body
            .pointer("/error/message")
            .and_then(|v| v.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| body.to_string());
        anyhow::bail!("AI 接口错误 HTTP {status}: {msg}");
    }
    body.pointer("/choices/0/message/content")
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .context("AI 接口响应缺少 choices[0].message.content")
}
