use std::time::Duration;

use reqwest::{Client, Url};
use serde_json::Value;

use crate::config::{ChannelType, ProviderConfig};

pub async fn fetch_models(provider: &ProviderConfig) -> Result<Vec<(String, String)>, String> {
    let url = models_url(provider)?;
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .build()
        .map_err(|error| format!("无法创建 HTTP 客户端: {error}"))?;
    let mut request = client.get(url);
    match provider.channel_type {
        ChannelType::OpenAiChat | ChannelType::OpenAiResponses => {
            if !provider.api_key.is_empty() {
                request = request.bearer_auth(&provider.api_key);
            }
        }
        ChannelType::Claude => {
            request = request.header("anthropic-version", "2023-06-01");
            if !provider.api_key.is_empty() {
                request = request.header("x-api-key", &provider.api_key);
            }
        }
        ChannelType::Gemini => {}
    }
    let response = request
        .send()
        .await
        .map_err(|error| format!("连接失败: {}", error.without_url()))?;
    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        let excerpt: String = body.chars().take(300).collect();
        let excerpt = if provider.api_key.is_empty() {
            excerpt
        } else {
            excerpt.replace(&provider.api_key, "[redacted]")
        };
        return Err(format!("接口返回 HTTP {status}: {excerpt}"));
    }
    let body: Value = response
        .json()
        .await
        .map_err(|error| format!("模型列表不是有效 JSON: {}", error.without_url()))?;
    parse_models(provider.channel_type, &body)
}

fn models_url(provider: &ProviderConfig) -> Result<Url, String> {
    let mut url = Url::parse(provider.base_url.trim()).map_err(|error| format!("接口地址无效: {error}"))?;
    let path = url.path().trim_end_matches('/');
    if !path.ends_with("/models") {
        let suffix = match provider.channel_type {
            ChannelType::OpenAiChat | ChannelType::OpenAiResponses if !path.ends_with("/v1") => "/v1/models",
            _ => "/models",
        };
        url.set_path(&format!("{path}{suffix}"));
    }
    if provider.channel_type == ChannelType::Gemini && !provider.api_key.is_empty() {
        url.query_pairs_mut().append_pair("key", &provider.api_key);
    }
    Ok(url)
}

fn parse_models(channel_type: ChannelType, body: &Value) -> Result<Vec<(String, String)>, String> {
    let field = if channel_type == ChannelType::Gemini {
        "models"
    } else {
        "data"
    };
    let items = body
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| "接口响应缺少模型列表".to_string())?;
    Ok(items
        .iter()
        .filter_map(|item| {
            let id = match channel_type {
                ChannelType::Gemini => item.get("name")?.as_str()?.strip_prefix("models/")?,
                _ => item.get("id")?.as_str()?,
            };
            let name = match channel_type {
                ChannelType::Claude => item.get("display_name").and_then(Value::as_str).unwrap_or(id),
                ChannelType::Gemini => item.get("displayName").and_then(Value::as_str).unwrap_or(id),
                _ => item.get("name").and_then(Value::as_str).unwrap_or(id),
            };
            Some((id.to_string(), name.to_string()))
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn provider(channel_type: ChannelType, base_url: &str) -> ProviderConfig {
        ProviderConfig {
            id: "test".into(),
            name: "Test".into(),
            channel_type,
            base_url: base_url.into(),
            api_path: String::new(),
            api_key: "secret".into(),
            api_key_ref: "provider/test".into(),
            enabled: true,
            models: Vec::new(),
            timeout_secs: 90,
            retries: 0,
            proxy: String::new(),
            extra_headers: Vec::new(),
        }
    }

    #[test]
    fn builds_provider_specific_model_urls() {
        let openai = models_url(&provider(ChannelType::OpenAiChat, "https://example.com/v1")).unwrap();
        assert_eq!(openai.as_str(), "https://example.com/v1/models");
        let claude = models_url(&provider(ChannelType::Claude, "https://api.anthropic.com/v1")).unwrap();
        assert_eq!(claude.as_str(), "https://api.anthropic.com/v1/models");
        let gemini = models_url(&provider(ChannelType::Gemini, "https://example.com/v1beta")).unwrap();
        assert_eq!(gemini.path(), "/v1beta/models");
        assert_eq!(
            gemini.query_pairs().find(|(name, _)| name == "key").unwrap().1,
            "secret"
        );
    }

    #[test]
    fn parses_all_provider_response_shapes() {
        assert_eq!(
            parse_models(ChannelType::OpenAiChat, &json!({"data": [{"id": "gpt-test"}]})).unwrap(),
            vec![("gpt-test".into(), "gpt-test".into())]
        );
        assert_eq!(
            parse_models(
                ChannelType::Claude,
                &json!({"data": [{"id": "claude-test", "display_name": "Claude Test"}]})
            )
            .unwrap(),
            vec![("claude-test".into(), "Claude Test".into())]
        );
        assert_eq!(
            parse_models(
                ChannelType::Gemini,
                &json!({"models": [{"name": "models/gemini-test", "displayName": "Gemini Test"}]})
            )
            .unwrap(),
            vec![("gemini-test".into(), "Gemini Test".into())]
        );
    }
}
