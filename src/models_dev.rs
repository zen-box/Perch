use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::{LazyLock, RwLock};
use std::time::{Duration, SystemTime};

use reqwest::Client;
use serde::{Deserialize, Serialize};

use crate::model_info::{Capability, ModelSpec};
use crate::paths::{MODELS_DEV_CACHE_FILE, data_dir, write_atomic};

const MODELS_DEV_URL: &str = "https://models.dev/models.json";
const CACHE_TTL_SECS: u64 = 86400; // 24 小时更新一次

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RawModelEntry {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub family: Option<String>,
    #[serde(default)]
    pub reasoning: Option<bool>,
    #[serde(default)]
    pub tool_call: Option<bool>,
    #[serde(default)]
    pub modalities: Option<Modalities>,
    #[serde(default)]
    pub limit: Option<Limits>,
    #[serde(default)]
    pub cost: Option<RawCost>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RawCost {
    #[serde(default)]
    pub input: Option<f64>,
    #[serde(default)]
    pub output: Option<f64>,
    #[serde(default)]
    pub cache_read: Option<f64>,
    #[serde(default)]
    pub cache_write: Option<f64>,
}

#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize)]
pub struct ModelCost {
    pub input: f64,       // $ / 1M tokens
    pub output: f64,      // $ / 1M tokens
    pub cache_read: f64,  // $ / 1M tokens
    pub cache_write: f64, // $ / 1M tokens
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Modalities {
    #[serde(default)]
    pub input: Vec<String>,
    #[serde(default)]
    pub output: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Limits {
    #[serde(default)]
    pub context: Option<u64>,
    #[serde(default)]
    pub output: Option<u64>,
}

pub struct ModelsDevRegistry {
    /// 完整 ID 映射（例如 "openai/gpt-4o" -> RawModelEntry）
    pub exact: HashMap<String, RawModelEntry>,
    /// 短 ID 映射（例如 "gpt-4o" -> RawModelEntry）
    pub short: HashMap<String, RawModelEntry>,
}

impl ModelsDevRegistry {
    pub fn new() -> Self {
        Self {
            exact: HashMap::new(),
            short: HashMap::new(),
        }
    }

    pub fn insert_entry(&mut self, full_id: String, entry: RawModelEntry) {
        let full_key = full_id.to_lowercase();
        // 提取短名字：比如 "anthropic/claude-3-5-sonnet-20241022" -> "claude-3-5-sonnet-20241022"
        let short_key = full_key.rsplit('/').next().unwrap_or(&full_key).to_string();

        self.exact.insert(full_key, entry.clone());
        if !self.short.contains_key(&short_key) {
            self.short.insert(short_key.clone(), entry.clone());
        }

        // 如果包含版本后缀，也可以建一个基础别名（如 "claude-3-5-sonnet-20241022" -> "claude-3-5-sonnet"）
        if let Some(stripped) = strip_version_suffix(&short_key)
            && !self.short.contains_key(stripped)
        {
            self.short.insert(stripped.to_string(), entry);
        }
    }

    pub fn lookup(&self, model_id: &str) -> Option<RawModelEntry> {
        let key = model_id.trim().to_lowercase();
        if let Some(entry) = self.exact.get(&key) {
            return Some(entry.clone());
        }
        let short_key = key.rsplit('/').next().unwrap_or(&key);
        if let Some(entry) = self.short.get(short_key) {
            return Some(entry.clone());
        }
        if let Some(stripped) = strip_version_suffix(short_key)
            && let Some(entry) = self.short.get(stripped)
        {
            return Some(entry.clone());
        }
        None
    }
}

static REGISTRY: LazyLock<RwLock<ModelsDevRegistry>> = LazyLock::new(|| {
    let registry = RwLock::new(ModelsDevRegistry::new());
    load_from_cache(&registry);
    registry
});

fn cache_file_path() -> PathBuf {
    data_dir().join(MODELS_DEV_CACHE_FILE)
}

/// 尝试从本地缓存文件恢复
fn load_from_cache(lock: &RwLock<ModelsDevRegistry>) {
    let path = cache_file_path();
    if !path.exists() {
        return;
    }
    let Ok(content) = fs::read_to_string(&path) else {
        return;
    };
    let Ok(map) = serde_json::from_str::<HashMap<String, RawModelEntry>>(&content) else {
        return;
    };

    let mut reg = lock.write().unwrap_or_else(|poison| poison.into_inner());
    for (id, mut entry) in map {
        if entry.id.is_empty() {
            entry.id = id.clone();
        }
        reg.insert_entry(id, entry);
    }
}

/// 后台异步更新 models.dev 缓存
pub fn sync_cache_background(force: bool) {
    std::thread::spawn(move || {
        let path = cache_file_path();
        if !force
            && path.exists()
            && let Ok(meta) = fs::metadata(&path)
            && let Ok(modified) = meta.modified()
            && let Ok(elapsed) = SystemTime::now().duration_since(modified)
            && elapsed.as_secs() < CACHE_TTL_SECS
        {
            return; // 还在有效期内
        }

        let runtime = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(_) => return,
        };

        runtime.block_on(async {
            let client = match Client::builder().timeout(Duration::from_secs(20)).build() {
                Ok(c) => c,
                Err(_) => return,
            };

            let resp = match client.get(MODELS_DEV_URL).send().await {
                Ok(r) if r.status().is_success() => r,
                _ => return,
            };

            let text = match resp.text().await {
                Ok(t) => t,
                Err(_) => return,
            };

            let map = match serde_json::from_str::<HashMap<String, RawModelEntry>>(&text) {
                Ok(m) => m,
                Err(_) => return,
            };

            // 写入本地缓存
            let _ = write_atomic(&path, &text);

            // 更新内存
            let mut reg = REGISTRY.write().unwrap_or_else(|poison| poison.into_inner());
            for (id, mut entry) in map {
                if entry.id.is_empty() {
                    entry.id = id.clone();
                }
                reg.insert_entry(id, entry);
            }
        });
    });
}

/// 匹配 models.dev 中的模型并转换为 ModelSpec
pub fn lookup_spec(model_id: &str) -> Option<ModelSpec> {
    let reg = REGISTRY.read().unwrap_or_else(|poison| poison.into_inner());
    let entry = reg.lookup(model_id)?;

    let mut capabilities = Vec::new();
    if let Some(modalities) = &entry.modalities {
        for input in &modalities.input {
            let lower = input.to_lowercase();
            if lower == "image" || lower == "vision" {
                if !capabilities.contains(&Capability::Vision) {
                    capabilities.push(Capability::Vision);
                }
            } else if (lower == "pdf" || lower == "file" || lower == "document")
                && !capabilities.contains(&Capability::Files)
            {
                capabilities.push(Capability::Files);
            }
        }
    }

    if entry.tool_call == Some(true) && !capabilities.contains(&Capability::Tools) {
        capabilities.push(Capability::Tools);
    }

    capabilities.sort();

    let context = entry.limit.as_ref().and_then(|l| l.context).map(|c| c as u32);
    let output = entry.limit.as_ref().and_then(|l| l.output).map(|o| o as u32);
    let always_thinks = entry.reasoning == Some(true);

    Some(ModelSpec {
        context_window: context,
        max_output: output,
        capabilities,
        reasoning_levels: Vec::new(),
        always_thinks,
    })
}

pub const USD_TO_CNY_RATE: f64 = 7.2;

/// 查询模型的每百万 Token 价格（$ / 1M tokens）
pub fn lookup_cost(model_id: &str) -> Option<ModelCost> {
    // 1. 先查 models.dev 缓存中记录的官方价格
    {
        let reg = REGISTRY.read().unwrap_or_else(|poison| poison.into_inner());
        if let Some(entry) = reg.lookup(model_id)
            && let Some(cost) = entry.cost
            && (cost.input.is_some() || cost.output.is_some())
        {
            return Some(ModelCost {
                input: cost.input.unwrap_or(0.0),
                output: cost.output.unwrap_or(0.0),
                cache_read: cost.cache_read.unwrap_or(0.0),
                cache_write: cost.cache_write.unwrap_or(0.0),
            });
        }
    }

    // 2. 启发式内置官方最新标准价格库（保证常用模型即使缓存无价格也能精准计费）
    let full = model_id.trim().to_ascii_lowercase();
    let name = full.rsplit('/').next().unwrap_or(&full);

    // OpenAI 系列
    if name.starts_with("gpt-4o-mini") {
        return Some(ModelCost {
            input: 0.15,
            output: 0.60,
            cache_read: 0.075,
            cache_write: 0.0,
        });
    }
    if name.starts_with("gpt-4o") || name.starts_with("chatgpt-4o") {
        return Some(ModelCost {
            input: 2.50,
            output: 10.00,
            cache_read: 1.25,
            cache_write: 0.0,
        });
    }
    if name.starts_with("o1-mini") || name.starts_with("o3-mini") {
        return Some(ModelCost {
            input: 1.10,
            output: 4.40,
            cache_read: 0.55,
            cache_write: 0.0,
        });
    }
    if name.starts_with("o1") || name.starts_with("o3") {
        return Some(ModelCost {
            input: 15.00,
            output: 60.00,
            cache_read: 7.50,
            cache_write: 0.0,
        });
    }
    if name.starts_with("gpt-4-turbo") {
        return Some(ModelCost {
            input: 10.00,
            output: 30.00,
            cache_read: 5.00,
            cache_write: 0.0,
        });
    }

    // Anthropic Claude 系列
    if name.contains("claude-3-5-sonnet") || name.contains("claude-3.5-sonnet") || name.contains("claude-3-7-sonnet") {
        return Some(ModelCost {
            input: 3.00,
            output: 15.00,
            cache_read: 0.30,
            cache_write: 3.75,
        });
    }
    if name.contains("claude-3-5-haiku") || name.contains("claude-3-haiku") {
        return Some(ModelCost {
            input: 0.80,
            output: 4.00,
            cache_read: 0.08,
            cache_write: 1.00,
        });
    }
    if name.contains("opus") {
        return Some(ModelCost {
            input: 15.00,
            output: 75.00,
            cache_read: 1.50,
            cache_write: 18.75,
        });
    }

    // DeepSeek 系列
    if name.contains("deepseek-r1") || name.contains("reasoner") {
        return Some(ModelCost {
            input: 0.55,
            output: 2.19,
            cache_read: 0.14,
            cache_write: 0.0,
        });
    }
    if name.contains("deepseek-chat") || name.contains("deepseek-v3") || name.contains("deepseek") {
        return Some(ModelCost {
            input: 0.14,
            output: 0.28,
            cache_read: 0.014,
            cache_write: 0.0,
        });
    }

    // Google Gemini 系列
    if name.contains("gemini-1.5-flash") || name.contains("gemini-2.0-flash") {
        return Some(ModelCost {
            input: 0.075,
            output: 0.30,
            cache_read: 0.018,
            cache_write: 0.0,
        });
    }
    if name.contains("gemini-1.5-pro") || name.contains("gemini-pro") {
        return Some(ModelCost {
            input: 1.25,
            output: 5.00,
            cache_read: 0.3125,
            cache_write: 0.0,
        });
    }

    // 国产主流 (SenseNova, Qwen, GLM, Kimi)
    if name.contains("sensenova") || name.contains("sensechat") {
        return Some(ModelCost {
            input: 0.20,
            output: 0.60,
            cache_read: 0.05,
            cache_write: 0.0,
        });
    }
    if name.contains("qwen-turbo") {
        return Some(ModelCost {
            input: 0.05,
            output: 0.20,
            cache_read: 0.01,
            cache_write: 0.0,
        });
    }
    if name.contains("qwen-plus") || name.contains("qwen2.5-72b") {
        return Some(ModelCost {
            input: 0.40,
            output: 1.20,
            cache_read: 0.10,
            cache_write: 0.0,
        });
    }
    if name.contains("qwen-max") {
        return Some(ModelCost {
            input: 2.40,
            output: 9.60,
            cache_read: 0.60,
            cache_write: 0.0,
        });
    }
    if name.contains("glm-4") || name.contains("glm-5") {
        return Some(ModelCost {
            input: 0.70,
            output: 0.70,
            cache_read: 0.15,
            cache_write: 0.0,
        });
    }
    if name.contains("moonshot") || name.contains("kimi") {
        return Some(ModelCost {
            input: 1.20,
            output: 1.20,
            cache_read: 0.30,
            cache_write: 0.0,
        });
    }

    None
}

/// 计算指定用量下的预估费用：返回 (美元 $, 人民币 ¥)
pub fn calculate_cost(
    model_id: &str,
    prompt_tokens: usize,
    completion_tokens: usize,
    cached_tokens: usize,
) -> (f64, f64) {
    let Some(cost) = lookup_cost(model_id) else {
        return (0.0, 0.0);
    };

    let actual_prompt = prompt_tokens.saturating_sub(cached_tokens);
    let prompt_cost = (actual_prompt as f64 * cost.input) / 1_000_000.0;
    let cache_cost = (cached_tokens as f64 * cost.cache_read) / 1_000_000.0;
    let completion_cost = (completion_tokens as f64 * cost.output) / 1_000_000.0;

    let total_usd = prompt_cost + cache_cost + completion_cost;
    let total_cny = total_usd * USD_TO_CNY_RATE;

    (total_usd, total_cny)
}

fn strip_version_suffix(name: &str) -> Option<&str> {
    // 匹配日期后缀如 -20241022, -20250219, -0125 等
    if let Some(idx) = name.rfind('-') {
        let suffix = &name[idx + 1..];
        if suffix.chars().all(|c| c.is_ascii_digit()) && suffix.len() >= 4 {
            return Some(&name[..idx]);
        }
        if suffix == "latest" || suffix == "preview" || suffix == "online" {
            return Some(&name[..idx]);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_registry_lookup() {
        let mut reg = ModelsDevRegistry::new();
        let entry = RawModelEntry {
            id: "openai/gpt-4o".into(),
            name: Some("GPT-4o".into()),
            family: Some("gpt".into()),
            reasoning: Some(false),
            tool_call: Some(true),
            modalities: Some(Modalities {
                input: vec!["text".into(), "image".into()],
                output: vec!["text".into()],
            }),
            limit: Some(Limits {
                context: Some(128000),
                output: Some(16384),
            }),
            cost: None,
        };

        reg.insert_entry("openai/gpt-4o".into(), entry);

        assert!(reg.lookup("openai/gpt-4o").is_some());
        assert!(reg.lookup("gpt-4o").is_some());
        assert!(reg.lookup("GPT-4O").is_some());
    }

    #[test]
    fn test_cost_calculation() {
        let (cost_usd, cost_cny) = calculate_cost("gpt-4o", 1_000_000, 1_000_000, 0);
        // gpt-4o builtin: input $2.5 / M, output $10.0 / M
        assert!((cost_usd - 12.5).abs() < 0.001);
        assert!((cost_cny - 12.5 * 7.2).abs() < 0.01);

        let (cost_usd_mini, _) = calculate_cost("gpt-4o-mini", 1_000_000, 1_000_000, 0);
        // gpt-4o-mini builtin: input $0.15 / M, output $0.60 / M
        assert!((cost_usd_mini - 0.75).abs() < 0.001);
    }
}
