//! 按模型 ID 自动识别模型规格：上下文窗口、最大输出、能力和支持的思考强度。
//!
//! 识别结果只是默认值，用户在「编辑模型」里改过的字段以用户设置为准。
//! 数值取自各家官方文档；没有收录的新模型对应字段为空，界面上显示「未知」。

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::i18n::{AppLanguage, Key, tr};
use crate::model::ReasoningLevel;

/// 模型能力。目前只用于展示和筛选，附件、联网等功能接入后会按这里判断是否可用。
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Capability {
    Vision,
    Files,
    Tools,
    WebSearch,
    ImageOutput,
}

impl Capability {
    pub const ALL: [Capability; 5] = [
        Capability::Vision,
        Capability::Files,
        Capability::Tools,
        Capability::WebSearch,
        Capability::ImageOutput,
    ];

    /// 界面上的能力名。
    pub fn label(self, lang: AppLanguage) -> &'static str {
        tr(
            lang,
            match self {
                Capability::Vision => Key::CapabilityVision,
                Capability::Files => Key::CapabilityFiles,
                Capability::Tools => Key::CapabilityTools,
                Capability::WebSearch => Key::CapabilityWebSearch,
                Capability::ImageOutput => Key::CapabilityImageOutput,
            },
        )
    }

    /// 能力名下面的补充说明，用在模型信息卡片里。
    pub fn description(self, lang: AppLanguage) -> &'static str {
        tr(
            lang,
            match self {
                Capability::Vision => Key::CapabilityVisionDesc,
                Capability::Files => Key::CapabilityFilesDesc,
                Capability::Tools => Key::CapabilityToolsDesc,
                Capability::WebSearch => Key::CapabilityWebSearchDesc,
                Capability::ImageOutput => Key::CapabilityImageOutputDesc,
            },
        )
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct ModelSpec {
    pub context_window: Option<u32>,
    pub max_output: Option<u32>,
    pub capabilities: Vec<Capability>,
    /// 可以调节的思考强度，空表示不支持调节
    pub reasoning_levels: Vec<ReasoningLevel>,
    /// 总会输出思考过程、但不能调节强度的模型（如 DeepSeek R1）
    pub always_thinks: bool,
}

use Capability::{Files, ImageOutput, Tools, Vision, WebSearch};
use ReasoningLevel::{High, Low, Medium, Minimal, Off};

const LMH: &[ReasoningLevel] = &[Low, Medium, High];
const MULTIMODAL: &[Capability] = &[Vision, Files, Tools];

static SIZE_SUFFIX: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:^|[-_./])(\d+(?:\.\d+)?)([km])(?:$|[-_./])").unwrap());
static O_SERIES: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^o[134](?:-|$)").unwrap());
static CLAUDE_4: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?:opus|sonnet|haiku)-4(?:[-.]|$)|claude-4(?:[-.]|$)").unwrap());
static CLAUDE_OPUS_4_EARLY: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"opus-4(?:$|-20\d{6}|[-.]1(?:$|[-.@]))|claude-4-opus").unwrap());
static GLM_VISION: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"glm-\d(?:\.\d)?v|glm-4v").unwrap());

fn spec(context: u32, output: u32, capabilities: &[Capability], levels: &[ReasoningLevel]) -> ModelSpec {
    ModelSpec {
        context_window: (context > 0).then_some(context),
        max_output: (output > 0).then_some(output),
        capabilities: capabilities.to_vec(),
        reasoning_levels: levels.to_vec(),
        always_thinks: false,
    }
}

fn thinking(mut spec: ModelSpec) -> ModelSpec {
    spec.always_thinks = true;
    spec
}

fn apply_registry_limits(spec: &mut ModelSpec, registry: &ModelSpec) {
    spec.context_window = registry.context_window.or(spec.context_window);
    spec.max_output = registry.max_output.or(spec.max_output);
}

pub fn detect(model_id: &str, tags: &str) -> ModelSpec {
    let full = model_id.trim().to_ascii_lowercase();
    // "deepseek-ai/DeepSeek-V3"、"openai/gpt-4o:free" 这类写法只看最后一段的模型名
    let name = full.rsplit('/').next().unwrap_or(&full);
    let name = name.split(':').next().unwrap_or(name);

    // 1. 本地内置规则识别系列与支持的思考强度
    let mut spec = family_spec(name);

    // 2. models.dev 元数据库补充/校准上下文窗口、最大输出与能力（如多模态视觉）
    if let Some(dev_spec) = crate::models_dev::lookup_spec(model_id) {
        apply_registry_limits(&mut spec, &dev_spec);
        for cap in dev_spec.capabilities {
            if !spec.capabilities.contains(&cap) {
                spec.capabilities.push(cap);
            }
        }
        if spec.reasoning_levels.is_empty() && !dev_spec.reasoning_levels.is_empty() {
            // 本地规则没有覆盖的新模型，直接采用 models.dev 的推理档位。
            spec.reasoning_levels = dev_spec.reasoning_levels;
        }
        if dev_spec.always_thinks {
            spec.always_thinks = true;
        }
    }

    if spec.context_window.is_none() {
        spec.context_window = size_suffix(name);
    }
    let mut add = |capability: Capability| {
        if !spec.capabilities.contains(&capability) {
            spec.capabilities.push(capability);
        }
    };
    if [
        "vision",
        "-vl",
        "vl-",
        "qvq",
        "pixtral",
        "llava",
        "internvl",
        "minicpm-v",
        "sensenova-u",
        "-u1",
        "step-1v",
        "step-2",
        "glm-4v",
        "glm-4.5v",
        "glm-5v",
    ]
    .iter()
    .any(|word| name.contains(word))
    {
        add(Vision);
    }
    if name.contains("search") || name.contains("sonar") || full.ends_with(":online") {
        add(WebSearch);
    }
    if [
        "dall-e",
        "gpt-image",
        "imagen",
        "flux",
        "stable-diffusion",
        "sdxl",
        "sd3",
        "cogview",
        "wanx",
        "seedream",
        "kolors",
        "midjourney",
        "qwen-image",
        "hunyuan-image",
        "-image",
    ]
    .iter()
    .any(|word| name.contains(word))
    {
        add(ImageOutput);
    }
    if spec.reasoning_levels.is_empty()
        && !name.contains("non-reasoning")
        && ["reasoner", "reasoning", "thinking", "-r1", "qwq"]
            .iter()
            .any(|word| name.contains(word))
    {
        spec.always_thinks = true;
    }
    // 旧版本在标签里写「推理」来打开思考选项
    let tags = tags.to_lowercase();
    if spec.reasoning_levels.is_empty() && ["推理", "reason", "think"].iter().any(|word| tags.contains(word)) {
        spec.reasoning_levels = LMH.to_vec();
    }
    spec.capabilities.sort();
    spec
}

fn family_spec(name: &str) -> ModelSpec {
    let has = |needle: &str| name.contains(needle);
    let starts = |prefix: &str| name.starts_with(prefix);

    // ---------- OpenAI ----------
    // 新版 GPT 系列默认支持可调推理；上下文和输出上限交给接口元数据补充。
    if starts("gpt-6") {
        return if has("chat") {
            spec(0, 0, MULTIMODAL, &[])
        } else {
            spec(0, 0, MULTIMODAL, &[Minimal, Low, Medium, High])
        };
    }
    if starts("gpt-5") {
        return if has("chat") {
            spec(128_000, 16_384, MULTIMODAL, &[])
        } else {
            spec(400_000, 128_000, MULTIMODAL, &[Minimal, Low, Medium, High])
        };
    }
    if starts("gpt-4.1") {
        return spec(1_047_576, 32_768, MULTIMODAL, &[]);
    }
    if starts("gpt-4o") || starts("chatgpt-4o") {
        return spec(128_000, 16_384, MULTIMODAL, &[]);
    }
    if starts("gpt-4-turbo") || starts("gpt-4-1106") || starts("gpt-4-0125") || starts("gpt-4-vision") {
        return spec(128_000, 4_096, &[Vision, Tools], &[]);
    }
    if starts("gpt-4") {
        return spec(8_192, 8_192, &[Tools], &[]);
    }
    if starts("gpt-3.5") {
        return spec(16_385, 4_096, &[Tools], &[]);
    }
    if starts("gpt-oss") {
        return spec(131_072, 131_072, &[Tools], LMH);
    }
    if O_SERIES.is_match(name) {
        let text_only = starts("o1-mini") || starts("o1-preview") || starts("o3-mini");
        let capabilities: &[Capability] = if text_only { &[Tools] } else { MULTIMODAL };
        return spec(200_000, 100_000, capabilities, LMH);
    }

    // ---------- Anthropic ----------
    if has("claude") {
        if has("claude-3-7") || has("claude-3.7") {
            return spec(200_000, 64_000, MULTIMODAL, &[Off, Low, Medium, High]);
        }
        if has("claude-3-5") || has("claude-3.5") {
            return spec(200_000, 8_192, MULTIMODAL, &[]);
        }
        if has("claude-3") {
            return spec(200_000, 4_096, MULTIMODAL, &[]);
        }
        if has("claude-2") || has("claude-instant") {
            return spec(100_000, 4_096, &[], &[]);
        }
        // Claude 4 及之后都支持扩展思考。没收录的新版本按 32K 输出算，保证不超限
        let output = if CLAUDE_4.is_match(name) && !CLAUDE_OPUS_4_EARLY.is_match(name) {
            64_000
        } else {
            32_000
        };
        return spec(200_000, output, MULTIMODAL, &[Off, Low, Medium, High]);
    }

    // ---------- Google ----------
    if has("gemini") {
        if has("image") {
            return spec(32_768, 32_768, &[Vision, ImageOutput], &[]);
        }
        if has("gemini-1.5-pro") {
            return spec(2_097_152, 8_192, MULTIMODAL, &[]);
        }
        if has("gemini-1.5") {
            return spec(1_048_576, 8_192, MULTIMODAL, &[]);
        }
        let all = &[Vision, Files, Tools, WebSearch];
        if has("gemini-2.0") {
            return spec(1_048_576, 8_192, all, &[]);
        }
        // 2.5 及之后默认会思考；Flash 可以关闭思考，Pro 不行
        let levels: &[ReasoningLevel] = if has("flash") { &[Off, Low, Medium, High] } else { LMH };
        return spec(1_048_576, 65_536, all, levels);
    }
    if has("imagen") || starts("veo") {
        return spec(0, 0, &[ImageOutput], &[]);
    }
    if has("gemma-3") {
        return spec(131_072, 8_192, &[Vision], &[]);
    }

    // ---------- DeepSeek ----------
    if has("deepseek") {
        if has("reasoner") || has("-r1") || has("r1-") {
            return thinking(spec(128_000, 64_000, &[Tools], &[]));
        }
        if has("-vl") || has("vl2") {
            return spec(0, 0, &[Vision], &[]);
        }
        return spec(128_000, 8_192, &[Tools], &[]);
    }

    // ---------- xAI ----------
    if has("grok") {
        if has("grok-3-mini") {
            return spec(131_072, 0, &[Tools], &[Low, High]);
        }
        if has("vision") || has("grok-2-v") {
            return spec(32_768, 0, &[Vision], &[]);
        }
        if has("grok-4") {
            let base = spec(256_000, 0, &[Vision, Tools], &[]);
            return if has("non-reasoning") { base } else { thinking(base) };
        }
        return spec(131_072, 0, &[Tools], &[]);
    }

    // ---------- 通义千问 ----------
    if has("qwen") || has("qwq") || has("qvq") {
        let mut capabilities = Vec::new();
        if has("vl") || has("qvq") || has("omni") {
            capabilities.push(Vision);
        }
        if [
            "qwen-max",
            "qwen-plus",
            "qwen-turbo",
            "qwen-flash",
            "qwen2.5",
            "qwen3",
            "qwq",
        ]
        .iter()
        .any(|prefix| starts(prefix))
        {
            capabilities.push(Tools);
        }
        let base = spec(0, 0, &capabilities, &[]);
        return if has("qwq") || has("qvq") || has("thinking") {
            thinking(base)
        } else {
            base
        };
    }

    // ---------- 智谱 GLM ----------
    if has("glm") {
        let mut capabilities = vec![Tools];
        if GLM_VISION.is_match(name) {
            capabilities.push(Vision);
        }
        let base = spec(0, 0, &capabilities, &[]);
        return if has("thinking") || has("glm-z1") {
            thinking(base)
        } else {
            base
        };
    }

    // ---------- 月之暗面 Kimi ----------
    if has("kimi") || has("moonshot") {
        let mut capabilities = vec![Tools];
        if has("vision") || has("kimi-latest") {
            capabilities.push(Vision);
        }
        let context = if has("kimi-k2-0905") || has("kimi-k2-turbo") || has("kimi-k2-thinking") {
            262_144
        } else if has("kimi-k2") || has("kimi-latest") {
            131_072
        } else {
            0
        };
        let base = spec(context, 0, &capabilities, &[]);
        return if has("thinking") { thinking(base) } else { base };
    }

    // ---------- 其他常见模型 ----------
    if has("doubao") {
        let mut capabilities = vec![Tools];
        if has("vision") || has("seed-1-6") || has("seed-1.6") {
            capabilities.push(Vision);
        }
        let base = spec(0, 0, &capabilities, &[]);
        return if has("thinking") { thinking(base) } else { base };
    }
    if has("hunyuan") {
        let capabilities: &[Capability] = if has("vision") { &[Vision] } else { &[] };
        let base = spec(0, 0, capabilities, &[]);
        return if has("t1") { thinking(base) } else { base };
    }
    if has("ernie") {
        let capabilities: &[Capability] = if has("vl") { &[Vision] } else { &[] };
        let base = spec(0, 0, capabilities, &[]);
        return if has("x1") { thinking(base) } else { base };
    }
    if has("minimax") || has("abab") {
        let base = spec(0, 0, &[Tools], &[]);
        return if has("m1") { thinking(base) } else { base };
    }
    if [
        "mistral",
        "mixtral",
        "codestral",
        "pixtral",
        "magistral",
        "ministral",
        "devstral",
    ]
    .iter()
    .any(|word| has(word))
    {
        let context = if has("codestral") {
            256_000
        } else if has("large") {
            131_072
        } else {
            0
        };
        let capabilities: &[Capability] = if has("pixtral") { &[Vision, Tools] } else { &[Tools] };
        let base = spec(context, 0, capabilities, &[]);
        return if has("magistral") { thinking(base) } else { base };
    }
    if has("llama") {
        let modern = [
            "llama-3.1",
            "llama-3.2",
            "llama-3.3",
            "llama-4",
            "llama3.1",
            "llama3.2",
            "llama3.3",
            "llama4",
        ]
        .iter()
        .any(|word| has(word));
        let vision = has("vision") || has("llama-4") || has("llama4");
        let capabilities: &[Capability] = match (vision, modern) {
            (true, _) => &[Vision, Tools],
            (false, true) => &[Tools],
            _ => &[],
        };
        return spec(if modern { 131_072 } else { 0 }, 0, capabilities, &[]);
    }
    if has("sonar") || has("pplx") {
        return spec(128_000, 0, &[WebSearch], &[]);
    }
    // ---------- 商汤 SenseNova ----------
    if has("sensenova") || has("sensechat") {
        let mut capabilities = vec![Tools];
        if has("-u1") || has("u1.") || has("vision") || has("-v") {
            capabilities.push(Vision);
        }
        let base = spec(131_072, 8_192, &capabilities, &[]);
        return if has("reasoner") || has("thinking") {
            thinking(base)
        } else {
            base
        };
    }
    ModelSpec::default()
}

/// 模型名里的上下文长度，例如 moonshot-v1-128k、glm-4-9b-chat-1m
fn size_suffix(name: &str) -> Option<u32> {
    let captures = SIZE_SUFFIX.captures(name)?;
    let number: f64 = captures.get(1)?.as_str().parse().ok()?;
    let unit = if captures.get(2)?.as_str() == "m" {
        1_048_576.0
    } else {
        1_024.0
    };
    let value = number * unit;
    (1_000.0..=100_000_000.0).contains(&value).then_some(value as u32)
}

/// 展示用：128000 和 131072 都显示为 "128K"，1048576 显示为 "1M"
pub fn format_tokens(value: u32) -> String {
    let value = value as f64;
    let (unit, bases) = if value >= 999_500.0 {
        ("M", [1_000_000.0, 1_048_576.0])
    } else if value >= 1_000.0 {
        ("K", [1_000.0, 1_024.0])
    } else {
        return format!("{value}");
    };
    let best = bases
        .iter()
        .map(|base| value / base)
        .min_by(|a, b| (a - a.round()).abs().total_cmp(&(b - b.round()).abs()))
        .unwrap_or(value);
    if (best - best.round()).abs() < 0.05 {
        format!("{}{unit}", best.round())
    } else {
        format!("{best:.1}{unit}")
    }
}

/// 输入框里的写法：能整除时写成 K / M，否则写原数，保证解析回来还是同一个数
pub fn format_tokens_exact(value: u32) -> String {
    if value >= 1_000_000 && value.is_multiple_of(1_000_000) {
        format!("{}M", value / 1_000_000)
    } else if value >= 1_000 && value.is_multiple_of(1_000) {
        format!("{}K", value / 1_000)
    } else {
        value.to_string()
    }
}

/// 解析 token 数时可能出的错。
///
/// 这里只报"错在哪"，具体提示文案交给界面层翻——数据层不依赖文案表，
/// 也就不必为了报错而给每个调用点都传一个语言参数。
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TokenParseError {
    /// 不是能识别的数字
    NotANumber,
    /// 数字超出了合理范围（1 ~ 1 亿）
    OutOfRange,
}

/// 解析用户输入的 token 数："128K" → 128000，"1.5M" → 1500000，"131072" → 131072。
/// 空字符串表示自动识别。K、M 按 1000 进位：作为上限时宁可偏小，不会超出模型限制。
pub fn parse_tokens(text: &str) -> Result<Option<u32>, TokenParseError> {
    let text = text.trim().replace([',', '_', ' '], "");
    if text.is_empty() {
        return Ok(None);
    }
    let lower = text.to_ascii_lowercase();
    let (number, multiplier) = if let Some(number) = lower.strip_suffix('k') {
        (number, 1_000.0)
    } else if let Some(number) = lower.strip_suffix('m') {
        (number, 1_000_000.0)
    } else {
        (lower.as_str(), 1.0)
    };
    let value: f64 = number.parse().map_err(|_| TokenParseError::NotANumber)?;
    let value = (value * multiplier).round();
    if !(1.0..=100_000_000.0).contains(&value) {
        return Err(TokenParseError::OutOfRange);
    }
    Ok(Some(value as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_limits_override_family_fallback_without_erasing_missing_fields() {
        let mut detected = spec(200_000, 32_000, MULTIMODAL, LMH);
        let registry = ModelSpec {
            context_window: Some(1_000_000),
            max_output: Some(128_000),
            ..ModelSpec::default()
        };
        apply_registry_limits(&mut detected, &registry);
        assert_eq!(detected.context_window, Some(1_000_000));
        assert_eq!(detected.max_output, Some(128_000));
        apply_registry_limits(&mut detected, &ModelSpec::default());
        assert_eq!(detected.context_window, Some(1_000_000));
        assert_eq!(detected.max_output, Some(128_000));
    }

    #[test]
    fn well_known_models_are_detected() {
        let gpt = detect("gpt-4o-mini", "");
        assert_eq!(gpt.context_window, Some(128_000));
        assert!(gpt.capabilities.contains(&Vision));
        assert!(gpt.reasoning_levels.is_empty());

        let o3 = detect("o3-mini", "");
        assert_eq!(o3.reasoning_levels, LMH);
        assert!(!o3.capabilities.contains(&Vision));

        let sonnet = detect("claude-sonnet-4-5-20250929", "");
        assert_eq!(sonnet.max_output, Some(64_000));
        assert!(sonnet.reasoning_levels.contains(&Off));
        assert_eq!(detect("claude-opus-4-1-20250805", "").max_output, Some(32_000));
        assert_eq!(detect("anthropic/claude-opus-4", "").max_output, Some(32_000));
        assert_eq!(detect("claude-opus-4-5", "").max_output, Some(64_000));
        assert_eq!(detect("claude-3-5-sonnet-20241022", "").max_output, Some(8_192));

        let flash = detect("gemini-2.5-flash", "");
        assert!(flash.reasoning_levels.contains(&Off));
        assert!(!detect("gemini-2.5-pro", "").reasoning_levels.contains(&Off));

        let r1 = detect("deepseek-ai/DeepSeek-R1", "");
        assert!(r1.always_thinks);
        // 内置规则只标记总会思考；可调档位由可选的 models.dev 缓存补充。
        assert!(family_spec("deepseek-r1").reasoning_levels.is_empty());
        assert!(!detect("deepseek-chat", "").always_thinks);
    }

    #[test]
    fn suffixes_and_keywords_fill_the_gaps() {
        assert_eq!(detect("moonshot-v1-128k", "").context_window, Some(131_072));
        assert!(
            detect("moonshot-v1-8k-vision-preview", "")
                .capabilities
                .contains(&Vision)
        );
        assert!(detect("qwen2.5-vl-72b-instruct", "").capabilities.contains(&Vision));
        assert!(detect("qwq-32b", "").always_thinks);
        assert!(detect("dall-e-3", "").capabilities.contains(&ImageOutput));
        assert!(detect("gpt-4o-search-preview", "").capabilities.contains(&WebSearch));
        assert!(!detect("grok-4-fast-non-reasoning", "").always_thinks);
        assert_eq!(detect("my-relay-model", "推理").reasoning_levels, LMH);
        assert_eq!(detect("my-relay-model", ""), ModelSpec::default());
    }

    #[test]
    fn token_counts_format_and_parse() {
        assert_eq!(format_tokens(128_000), "128K");
        assert_eq!(format_tokens(131_072), "128K");
        assert_eq!(format_tokens(1_047_576), "1M");
        assert_eq!(format_tokens(2_097_152), "2M");
        assert_eq!(format_tokens(16_384), "16K");
        assert_eq!(format_tokens(400_000), "400K");
        assert_eq!(format_tokens(16_385), "16K");

        assert_eq!(parse_tokens("128K"), Ok(Some(128_000)));
        assert_eq!(parse_tokens(" 1.5m "), Ok(Some(1_500_000)));
        assert_eq!(parse_tokens("131,072"), Ok(Some(131_072)));
        assert_eq!(parse_tokens(""), Ok(None));
        assert!(parse_tokens("abc").is_err());
        assert!(parse_tokens("0").is_err());

        for value in [128_000, 131_072, 1_000_000, 64_000, 8_192] {
            assert_eq!(parse_tokens(&format_tokens_exact(value)), Ok(Some(value)));
        }
    }
}
