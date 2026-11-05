//! LLM 适配器 — OpenAI 兼容 HTTP 面（/chat/completions + /embeddings），
//! reqwest 直调；provider 行携带 base_url（云）/local_host+port（本地）与
//! 解密后的 api_key。与上游 go-openai SDK 调用同语义：非 200 读 body 前
//! 512 字节入错误、超时按 provider.timeout_seconds（默认 60）、
//! usage 的 prompt/completion tokens 回传给调用方挂账。

use crate::data::sys_ai_providers::Model as ProviderRow;

pub struct ChatMessage {
    pub role: &'static str, // system | user | assistant
    pub content: String,
}

pub struct ChatResult {
    pub content: String,
    pub model: String,
    pub prompt_tokens: u32,
    pub completion_tokens: u32,
    pub duration_ms: u32,
}

fn base_of(row: &ProviderRow) -> String {
    if let Some(url) = row
        .base_url
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
    {
        return url.trim_end_matches('/').to_string();
    }
    // 本地模型（Ollama 等）：http://host:port
    format!(
        "http://{}:{}",
        row.local_host.as_deref().unwrap_or("127.0.0.1"),
        row.local_port.unwrap_or(11434)
    )
}

fn api_key_of(row: &ProviderRow) -> String {
    row.api_key
        .as_deref()
        .map(crate::crypto::decrypt_if_needed)
        .transpose()
        .unwrap_or(None)
        .unwrap_or_default()
}

fn client(row: &ProviderRow) -> reqwest::Client {
    let timeout =
        std::time::Duration::from_secs(row.timeout_seconds.unwrap_or(60).clamp(1, 600) as u64);
    reqwest::Client::builder()
        .timeout(timeout)
        .build()
        .unwrap_or_default()
}

/// OpenAI 兼容 /chat/completions（非流式；本服务面无 SSE 需求）。
pub async fn chat_completion(
    row: &ProviderRow,
    messages: Vec<ChatMessage>,
) -> Result<ChatResult, String> {
    let url = format!("{}/chat/completions", base_of(row));
    let body = serde_json::json!({
        "model": row.model_name.clone().unwrap_or_default(),
        "messages": messages
            .iter()
            .map(|m| serde_json::json!({"role": m.role, "content": m.content}))
            .collect::<Vec<_>>(),
    });
    let started = std::time::Instant::now();
    let mut req = client(row).post(&url).json(&body);
    let key = api_key_of(row);
    if !key.is_empty() {
        req = req.bearer_auth(&key);
    }
    if let Some(org) = row.organization.as_deref().filter(|s| !s.is_empty()) {
        req = req.header("OpenAI-Organization", org);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("llm chat request: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let head = resp.text().await.unwrap_or_default();
        let head: String = head.chars().take(512).collect();
        return Err(format!("llm chat returned {status}: {head}"));
    }
    #[derive(serde::Deserialize)]
    struct Choice {
        message: ChoiceMessage,
    }
    #[derive(serde::Deserialize)]
    struct ChoiceMessage {
        content: Option<String>,
    }
    #[derive(serde::Deserialize)]
    struct Usage {
        #[serde(default)]
        prompt_tokens: u32,
        #[serde(default)]
        completion_tokens: u32,
    }
    #[derive(serde::Deserialize)]
    struct ChatResp {
        model: Option<String>,
        choices: Vec<Choice>,
        usage: Option<Usage>,
    }
    let parsed: ChatResp = resp
        .json()
        .await
        .map_err(|e| format!("llm chat decode: {e}"))?;
    let content = parsed
        .choices
        .first()
        .and_then(|c| c.message.content.clone())
        .unwrap_or_default();
    let (pt, ct) = parsed
        .usage
        .map(|u| (u.prompt_tokens, u.completion_tokens))
        .unwrap_or((0, 0));
    Ok(ChatResult {
        content,
        model: parsed
            .model
            .unwrap_or_else(|| row.model_name.clone().unwrap_or_default()),
        prompt_tokens: pt,
        completion_tokens: ct,
        duration_ms: started.elapsed().as_millis() as u32,
    })
}

/// OpenAI 兼容 /embeddings：批量向量化（知识库入库与查询检索共用；
/// RAG 通道随 3.10c 接 pgvector，本件先落适配面）。
#[allow(dead_code)]
pub async fn embeddings(
    row: &ProviderRow,
    model: &str,
    inputs: Vec<String>,
) -> Result<Vec<Vec<f32>>, String> {
    let url = format!("{}/embeddings", base_of(row));
    let body = serde_json::json!({ "model": model, "input": inputs });
    let mut req = client(row).post(&url).json(&body);
    let key = api_key_of(row);
    if !key.is_empty() {
        req = req.bearer_auth(&key);
    }
    let resp = req
        .send()
        .await
        .map_err(|e| format!("llm embeddings request: {e}"))?;
    let status = resp.status();
    if !status.is_success() {
        let head = resp.text().await.unwrap_or_default();
        let head: String = head.chars().take(512).collect();
        return Err(format!("llm embeddings returned {status}: {head}"));
    }
    #[derive(serde::Deserialize)]
    struct Item {
        embedding: Vec<f32>,
    }
    #[derive(serde::Deserialize)]
    struct EmbedResp {
        data: Vec<Item>,
    }
    let parsed: EmbedResp = resp
        .json()
        .await
        .map_err(|e| format!("llm embeddings decode: {e}"))?;
    Ok(parsed.data.into_iter().map(|d| d.embedding).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn provider(base: Option<&str>, host: Option<&str>, port: Option<i32>) -> ProviderRow {
        ProviderRow {
            id: 1,
            tenant_id: Some(0),
            name: Some("t".into()),
            model_type: Some("OPENAI_COMPATIBLE".into()),
            model_name: Some("gpt-x".into()),
            base_url: base.map(|s| s.into()),
            organization: None,
            api_key: None,
            api_key_hint: None,
            local_host: host.map(|s| s.into()),
            local_port: port,
            timeout_seconds: None,
            system_prompt: None,
            is_default: Some(false),
            remark: None,
            is_enabled: Some(true),
            created_by: None,
            updated_by: None,
            deleted_by: None,
            created_at: None,
            updated_at: None,
            deleted_at: None,
        }
    }

    #[test]
    fn base_url_prefers_cloud_then_local_fallback() {
        assert_eq!(
            base_of(&provider(Some("https://api.x.com/v1/"), None, None)),
            "https://api.x.com/v1"
        );
        assert_eq!(
            base_of(&provider(None, Some("10.0.0.8"), Some(11434))),
            "http://10.0.0.8:11434"
        );
        assert_eq!(
            base_of(&provider(None, None, None)),
            "http://127.0.0.1:11434"
        );
    }
}
