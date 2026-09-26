//! 模型与渠道的品牌识别，用来给模型列表、消息头像配上厂商图标。
//!
//! 模型关键词规则与头像配色参考 LobeHub Icons（MIT 协议，https://icons.lobehub.com）
//! 的 modelMappings 与各图标的 Avatar 样式。图标文件来自 @lobehub/icons-static-svg，
//! 放在 assets/brand 目录，构建时嵌入程序；缺少图标文件时显示品牌色加首字母。

use std::borrow::Cow;
use std::sync::LazyLock;

use gpui_kit::{AssetSource, SharedString};
use regex::Regex;

use crate::config::{ModelConfig, ProviderConfig};

include!(concat!(env!("OUT_DIR"), "/brand_icons.rs"));

pub struct Brand {
    /// 规则里引用的名字，也是用户手动选择图标时保存的值
    pub key: &'static str,
    pub title: &'static str,
    /// assets/brand 下的文件名（不含扩展名和 -color 后缀）
    icon: &'static str,
    pub background: u32,
    pub foreground: u32,
    /// 头像使用彩色图标（背景通常是白色），否则用单色图标按前景色着色
    color_icon: bool,
    /// 图标占头像边长的比例
    pub scale: f32,
}

/// 头像里实际要画的图标
pub enum BrandGlyph {
    /// 单色 SVG，按前景色着色
    Mono(SharedString),
    /// 彩色 SVG，原样绘制
    Color(SharedString),
    /// 没有图标文件时显示的字母
    Letter(char),
}

impl Brand {
    pub fn glyph(&self) -> BrandGlyph {
        if self.color_icon {
            let file = format!("{}-color.svg", self.icon);
            if embedded_icon(&file).is_some() {
                return BrandGlyph::Color(format!("brand/{file}").into());
            }
        }
        let file = format!("{}.svg", self.icon);
        if embedded_icon(&file).is_some() {
            return BrandGlyph::Mono(format!("brand/{file}").into());
        }
        BrandGlyph::Letter(self.key.chars().next().unwrap_or('?').to_ascii_uppercase())
    }

    /// 浅色背景的头像需要描边，否则在白色界面上看不出边界
    pub fn is_light(&self) -> bool {
        let [_, r, g, b] = self.background.to_be_bytes();
        (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) > 200.
    }

    /// 深色背景的头像在深色主题下也需要描边
    pub fn is_dark(&self) -> bool {
        let [_, r, g, b] = self.background.to_be_bytes();
        (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) < 48.
    }

    /// 没有图标文件时字母的颜色。彩色图标的品牌前景色没有意义，浅色背景上改用深色字
    pub fn letter_color(&self) -> u32 {
        if self.is_light() && (self.color_icon || self.foreground == self.background) {
            0x1F2937
        } else {
            self.foreground
        }
    }
}

macro_rules! brands {
    ($(($key:literal, $title:literal, $icon:literal, $bg:literal, $fg:literal, $color:literal, $scale:literal)),* $(,)?) => {
        pub static BRANDS: &[Brand] = &[
            $(Brand { key: $key, title: $title, icon: $icon, background: $bg, foreground: $fg, color_icon: $color, scale: $scale },)*
        ];
    };
}

brands![
    // 模型厂商
    ("openai", "OpenAI", "openai", 0x000000, 0xFFFFFF, false, 0.75),
    ("openai-gpt3", "GPT-3.5", "openai", 0x19C37D, 0xFFFFFF, false, 0.75),
    ("openai-gpt4", "GPT-4", "openai", 0xAB68FF, 0xFFFFFF, false, 0.75),
    ("openai-gpt5", "GPT-5", "openai", 0xF86AA4, 0xFFFFFF, false, 0.75),
    ("openai-o", "OpenAI o 系列", "openai", 0xF9C322, 0xFFFFFF, false, 0.75),
    ("openai-oss", "gpt-oss", "openai", 0x0099FF, 0xFFFFFF, false, 0.75),
    ("openai-platform", "OpenAI 平台", "openai", 0x0000FE, 0xFFFFFF, false, 0.75),
    ("sora", "Sora", "sora", 0x0968DA, 0xFFFFFF, false, 0.7),
    ("dalle", "DALL·E", "dalle", 0x000000, 0xFFFFFF, true, 0.6),
    ("claude", "Claude", "claude", 0xD97757, 0xFFFFFF, false, 0.75),
    ("anthropic", "Anthropic", "anthropic", 0xF1F0E8, 0x141413, false, 0.75),
    ("gemini", "Gemini", "gemini", 0xFFFFFF, 0xFFFFFF, true, 0.8),
    ("google", "Google", "google", 0xFFFFFF, 0xFFFFFF, true, 0.75),
    ("vertexai", "Vertex AI", "vertexai", 0x4285F4, 0xFFFFFF, false, 0.6),
    ("deepmind", "DeepMind", "deepmind", 0x1A73E8, 0xFFFFFF, false, 0.7),
    ("gemma", "Gemma", "gemma", 0x2E96FF, 0xFFFFFF, false, 0.9),
    ("nanobanana", "Nano Banana", "nanobanana", 0xFCD53F, 0xFFFFFF, false, 0.8),
    ("deepseek", "DeepSeek", "deepseek", 0x4D6BFE, 0xFFFFFF, false, 0.75),
    ("qwen", "通义千问", "qwen", 0x615CED, 0xFFFFFF, false, 0.75),
    ("chatglm", "ChatGLM", "chatglm", 0x4268FA, 0xFFFFFF, false, 0.75),
    ("zai", "Z.ai", "zai", 0x000000, 0xFFFFFF, false, 0.6),
    ("glmv", "GLM-V", "glmv", 0x4268FA, 0xFFFFFF, false, 0.7),
    ("zhipu", "智谱", "zhipu", 0x3859FF, 0xFFFFFF, false, 0.75),
    ("codegeex", "CodeGeeX", "codegeex", 0x00E7E7, 0x000000, false, 0.75),
    ("cogview", "CogView", "cogview", 0x000000, 0xFFFFFF, true, 0.6),
    ("moonshot", "月之暗面", "moonshot", 0x16191E, 0xFFFFFF, false, 0.75),
    ("kimi", "Kimi", "kimi", 0x000000, 0xFFFFFF, true, 0.6),
    ("doubao", "豆包", "doubao", 0xFFFFFF, 0xFFFFFF, true, 0.75),
    ("bytedance", "字节跳动", "bytedance", 0x325AB4, 0xFFFFFF, false, 0.6),
    ("jimeng", "即梦", "jimeng", 0x000000, 0xFFFFFF, true, 0.6),
    ("hunyuan", "腾讯混元", "hunyuan", 0x0053E0, 0xFFFFFF, false, 0.75),
    ("wenxin", "文心", "wenxin", 0x167ADF, 0xFFFFFF, false, 0.75),
    ("spark", "讯飞星火", "spark", 0x0070F0, 0xFFFFFF, false, 0.75),
    ("minimax", "MiniMax", "minimax", 0xF23F5D, 0xFFFFFF, false, 0.75),
    ("yi", "零一万物", "yi", 0x003425, 0xFFFFFF, true, 0.6),
    ("baichuan", "百川", "baichuan", 0xFF6933, 0xFFFFFF, false, 0.6),
    ("stepfun", "阶跃星辰", "stepfun", 0xFFFFFF, 0x000000, false, 0.65),
    ("sensenova", "商汤日日新", "sensenova", 0x5B2AD8, 0xFFFFFF, false, 0.7),
    ("internlm", "书生", "internlm", 0x1B3882, 0xFFFFFF, false, 0.75),
    ("ai360", "360 智脑", "ai360", 0x006FFB, 0xFFFFFF, false, 0.75),
    ("xiaomimimo", "小米 MiMo", "xiaomimimo", 0x000000, 0xFFFFFF, false, 0.7),
    ("longcat", "LongCat", "longcat", 0xFFFFFF, 0x000000, true, 0.7),
    ("skywork", "天工", "skywork", 0xFFFFFF, 0x000000, true, 0.75),
    ("kolors", "可图", "kolors", 0x83FF63, 0x000000, false, 0.75),
    ("grok", "Grok", "grok", 0x000000, 0xFFFFFF, false, 0.75),
    ("xai", "xAI", "xai", 0xFFFFFF, 0x000000, false, 0.65),
    ("meta", "Meta", "meta", 0x1D65C1, 0xFFFFFF, false, 0.75),
    ("mistral", "Mistral", "mistral", 0xFA520F, 0xFFFFFF, false, 0.75),
    ("cohere", "Cohere", "cohere", 0x39594D, 0xFFFFFF, false, 0.6),
    ("aya", "Aya", "aya", 0x416FDC, 0xFFFFFF, false, 0.6),
    ("perplexity", "Perplexity", "perplexity", 0x22B8CD, 0x000000, false, 0.75),
    ("microsoft", "Microsoft", "microsoft", 0x00A4EF, 0xFFFFFF, false, 0.6),
    ("nvidia", "NVIDIA", "nvidia", 0x76B900, 0xFFFFFF, false, 0.7),
    ("ibm", "IBM", "ibm", 0x0F62FE, 0xFFFFFF, false, 0.75),
    ("ai21", "AI21", "ai21", 0xE91E63, 0xFFFFFF, false, 0.7),
    ("upstage", "Upstage", "upstage", 0x908AF9, 0xFFFFFF, false, 0.6),
    ("nousresearch", "Nous Research", "nousresearch", 0x000000, 0xFFFFFF, false, 0.7),
    ("llava", "LLaVA", "llava", 0xCB2D30, 0xFFFFFF, false, 0.6),
    ("rwkv", "RWKV", "rwkv", 0x000000, 0xFFFFFF, false, 0.7),
    ("dbrx", "DBRX", "dbrx", 0xEE3D2C, 0xFFFFFF, false, 0.6),
    ("jina", "Jina", "jina", 0x000000, 0xFFFFFF, false, 0.6),
    ("voyage", "Voyage", "voyage", 0x012E33, 0xFFFFFF, false, 0.6),
    ("liquid", "Liquid", "liquid", 0xFFFFFF, 0x000000, false, 0.75),
    ("baai", "BAAI", "baai", 0x000000, 0xFFFFFF, false, 0.6),
    ("flux", "FLUX", "flux", 0x000000, 0xFFFFFF, false, 0.7),
    ("stability", "Stability AI", "stability", 0x330066, 0xFFFFFF, false, 0.7),
    ("midjourney", "Midjourney", "midjourney", 0xFFFFFF, 0x000000, false, 0.75),
    ("suno", "Suno", "suno", 0x000000, 0xFFFFFF, false, 0.6),
    ("cursor", "Cursor", "cursor", 0x000000, 0xFFFFFF, false, 0.6),
    // 云平台与中转服务
    ("openrouter", "OpenRouter", "openrouter", 0x000000, 0xC8FF00, false, 0.75),
    ("siliconcloud", "硅基流动", "siliconcloud", 0x6E29F6, 0xFFFFFF, false, 0.7),
    ("bailian", "阿里云百炼", "bailian", 0xFFFFFF, 0xFFFFFF, true, 0.75),
    ("alibabacloud", "阿里云", "alibabacloud", 0xFF6A00, 0xFFFFFF, false, 0.7),
    ("volcengine", "火山引擎", "volcengine", 0xFFFFFF, 0xFFFFFF, true, 0.75),
    ("tencentcloud", "腾讯云", "tencentcloud", 0x2151D1, 0xFFFFFF, false, 0.75),
    ("baiducloud", "百度智能云", "baiducloud", 0x2468F2, 0xFFFFFF, false, 0.75),
    ("iflytekcloud", "讯飞开放平台", "iflytekcloud", 0x2A80E2, 0xFFFFFF, false, 0.75),
    ("zeroone", "零一万物", "zeroone", 0x003425, 0xFFFFFF, true, 0.6),
    ("modelscope", "魔搭", "modelscope", 0x624AFF, 0xFFFFFF, false, 0.75),
    ("qiniu", "七牛云", "qiniu", 0x06AEEF, 0xFFFFFF, false, 0.75),
    ("giteeai", "Gitee AI", "giteeai", 0x000000, 0xFFFFFF, false, 0.75),
    ("infinigence", "无问芯穹", "infinigence", 0x7952EA, 0xFFFFFF, false, 0.6),
    ("ppio", "PPIO", "ppio", 0x2874FF, 0xFFFFFF, false, 0.75),
    ("aihubmix", "AiHubMix", "aihubmix", 0x006FFB, 0xFFFFFF, false, 0.75),
    ("ai302", "302.AI", "ai302", 0x8E47FF, 0xFFFFFF, false, 0.8),
    ("newapi", "New API", "newapi", 0xFFFFFF, 0xDD2E57, true, 0.7),
    ("azure", "Azure", "azure", 0xFFFFFF, 0xFFFFFF, true, 0.7),
    ("aws", "AWS", "aws", 0x222F3E, 0xFFFFFF, true, 0.75),
    ("bedrock", "Bedrock", "bedrock", 0x222F3E, 0xFFFFFF, false, 0.75),
    ("github", "GitHub", "github", 0x000000, 0xFFFFFF, false, 0.75),
    ("huggingface", "Hugging Face", "huggingface", 0xFFFFFF, 0xFFFFFF, true, 0.75),
    ("groq", "Groq", "groq", 0xF55036, 0xFFFFFF, false, 0.75),
    ("together", "Together AI", "together", 0xFFFFFF, 0x000000, true, 0.75),
    ("fireworks", "Fireworks", "fireworks", 0x5019C5, 0xFFFFFF, false, 0.75),
    ("cloudflare", "Cloudflare", "cloudflare", 0xF38020, 0xFFFFFF, false, 0.75),
    ("novita", "Novita", "novita", 0x23D57C, 0x000000, false, 0.75),
    ("deepinfra", "DeepInfra", "deepinfra", 0xFFFFFF, 0xFFFFFF, true, 0.75),
    ("cerebras", "Cerebras", "cerebras", 0xF15A29, 0xFFFFFF, false, 0.8),
    ("sambanova", "SambaNova", "sambanova", 0xEE7624, 0xFFFFFF, false, 0.6),
    ("hyperbolic", "Hyperbolic", "hyperbolic", 0x594CE9, 0xFFFFFF, false, 0.6),
    ("nebius", "Nebius", "nebius", 0xDAFF33, 0x052B42, false, 0.6),
    ("lambda", "Lambda", "lambda", 0x000000, 0xFFFFFF, false, 0.6),
    ("poe", "Poe", "poe", 0x000000, 0xFFFFFF, true, 0.75),
    ("ollama", "Ollama", "ollama", 0xFFFFFF, 0x000000, false, 0.75),
    ("lmstudio", "LM Studio", "lmstudio", 0x4338CA, 0xFFFFFF, false, 0.7),
    ("xinference", "Xinference", "xinference", 0x781FF5, 0xFFFFFF, false, 0.7),
    ("vllm", "vLLM", "vllm", 0xFFFFFF, 0xFFFFFF, true, 0.6),
];

/// 模型 ID 的匹配规则，按顺序第一条命中的生效（与 LobeHub 的 modelMappings 顺序一致）
const MODEL_RULES: &[(&str, &[&str])] = &[
    ("openai-gpt3", &["gpt-3"]),
    ("openai-gpt4", &["gpt-4"]),
    ("openai-gpt5", &["gpt-5"]),
    ("sora", &["sora"]),
    ("openai-oss", &["gpt-oss"]),
    ("openai-o", &["o1-", "^o1", "/o1", "o3-", "^o3", "/o3", "o4-", "^o4", "/o4"]),
    ("dalle", &["dalle", "dall-e"]),
    (
        "openai-platform",
        &[
            "text-embedding-", "tts-", "whisper-", "codex", "davinci", "babbage", "omni-moderation", "text-moderation",
            "computer-use",
        ],
    ),
    ("openai", &["^gpt-", "/gpt-", "openai"]),
    ("glmv", &["^glm-(.*)v", "/glm-(.*)v", "-glm-(.*)v"]),
    ("zai", &["^glm-5", "/glm-5", "/glm5", "-glm-4", "^glm-4", "/glm-4", "/glm4", "-glm-5"]),
    ("chatglm", &["^glm-", "/glm-", "chatglm", "-glm-"]),
    ("codegeex", &["^codegeex", "/codegeex"]),
    ("claude", &["claude"]),
    ("anthropic", &["anthropic"]),
    ("internlm", &["internlm", "internvl"]),
    ("nousresearch", &["deephermes", "hermes", "genstruct", "minos"]),
    ("nvidia", &["nemotron", "openreasoning", "nemoretriever", "neva-", "nv-"]),
    ("meta", &["llama", "/l3"]),
    ("llava", &["llava"]),
    (
        "nanobanana",
        &[
            r"gemini-\d+(?:\.\d+)?-(?:flash(?:-lite)?|pro)-image(?:-preview)?(?::|$)",
            "nanobanana",
            "nano-banana",
        ],
    ),
    ("gemini", &["gemini"]),
    ("deepmind", &["^imagen-", "/imagen-", r"^imagen\d/", r"/imagen\d"]),
    ("gemma", &["gemma"]),
    ("moonshot", &["kimi", "moonshot"]),
    ("qiniu", &["qiniu"]),
    ("qwen", &["qwen", "qwq", "qvq", "wanx", r"wan\d/", r"wan\d\.\d-", "tongyi", "gte-rerank"]),
    ("minimax", &["minimax", "abab", "^image-"]),
    (
        "mistral",
        &[
            "mistral", "mixtral", "codestral", "mathstral", "/mn-", "pixtral", "ministral", "magistral", "devstral",
            "voxtral",
        ],
    ),
    ("perplexity", &["pplx", "sonar"]),
    ("yi", &["^yi-", "/yi-", "-yi-"]),
    ("openrouter", &["^openrouter"]),
    ("aya", &["aya"]),
    ("cohere", &["command"]),
    ("dbrx", &["dbrx"]),
    ("stepfun", &["step"]),
    ("ai360", &["360gpt", "360zhinao"]),
    ("baichuan", &["baichuan"]),
    ("rwkv", &["rwkv", "/eagle-"]),
    ("wenxin", &["ernie", "irag"]),
    ("jina", &["^jina", "/jina"]),
    ("jimeng", &["^jimeng-", "/jimeng-", "seedream", "seededit", "seedance-"]),
    ("doubao", &["^ep-", "doubao-"]),
    ("hunyuan", &["hunyuan", "hy3"]),
    ("bytedance", &["skylark", "seed-", "bytedance"]),
    (
        "stability",
        &["stable-diffusion", "stable-video", "stable-cascade", "sdxl", "stablelm", "^stable-", "^sd3", "^sd2", "^sd1"],
    ),
    ("flux", &["flux"]),
    ("suno", &["suno"]),
    ("microsoft", &["wizardlm", "/phi-", "^phi-", "-phi-", "mai-", "microsoft"]),
    ("ai21", &["jamba", "^j2-", "ai21"]),
    ("upstage", &["^solar-", "/solar"]),
    ("sensenova", &["sensechat", "sensenova"]),
    ("grok", &["^grok-", "/grok-"]),
    ("meta", &["(^|/)muse-spark($|-)"]),
    (
        "spark",
        &["spark", "general$", "generalv3$", r"generalv3\.5$", r"4\.0ultra$", "pro-128k$", "^max-32k$", "^lite$", "^x1$"],
    ),
    ("deepseek", &["deepseek"]),
    ("voyage", &["voyage"]),
    ("liquid", &["liquid", "lfm"]),
    ("aihubmix", &["aihubmix"]),
    ("vertexai", &["^veo-", "/veo-", "^veo3"]),
    ("google", &["google", "learnlm", "nano-banana"]),
    ("cogview", &["cogview"]),
    ("kolors", &["kolors"]),
    ("baiducloud", &["baidu", "qianfan"]),
    ("ibm", &["ibm", "granite"]),
    ("skywork", &["skywork"]),
    ("longcat", &["longcat"]),
    ("xiaomimimo", &["^mimo-", "/mimo-"]),
    ("baai", &["^baai", "^bge-", "/beg-", "touchd", "robobrain"]),
    ("cursor", &["^composer", "/composer", "-composer"]),
];

/// 渠道按接口地址匹配
const PROVIDER_RULES: &[(&str, &[&str])] = &[
    ("openai", &["api.openai.com"]),
    ("anthropic", &["api.anthropic.com"]),
    ("vertexai", &["aiplatform.googleapis.com"]),
    ("gemini", &["generativelanguage.googleapis.com"]),
    ("deepseek", &["deepseek.com"]),
    ("openrouter", &["openrouter.ai"]),
    ("siliconcloud", &["siliconflow"]),
    ("bailian", &["dashscope", "bailian"]),
    ("volcengine", &["volces.com", "volcengine"]),
    ("moonshot", &["moonshot.cn", "moonshot.ai"]),
    ("zhipu", &["bigmodel.cn"]),
    ("zai", &["api.z.ai"]),
    ("minimax", &["minimax"]),
    ("stepfun", &["stepfun"]),
    ("baichuan", &["baichuan-ai"]),
    ("zeroone", &["lingyiwanwu", "01.ai"]),
    ("hunyuan", &["hunyuan"]),
    ("tencentcloud", &["tencentcloudapi", "cloud.tencent"]),
    ("baiducloud", &["baidubce", "qianfan"]),
    ("iflytekcloud", &["xf-yun.com", "xfyun"]),
    ("xai", &["api.x.ai"]),
    ("mistral", &["mistral.ai"]),
    ("groq", &["groq.com"]),
    ("together", &["together.xyz", "together.ai"]),
    ("fireworks", &["fireworks.ai"]),
    ("perplexity", &["perplexity.ai"]),
    ("cohere", &["cohere.com", "cohere.ai"]),
    ("github", &["models.inference.ai.azure.com", "models.github.ai"]),
    ("azure", &["azure.com"]),
    ("bedrock", &["bedrock", "amazonaws"]),
    ("aihubmix", &["aihubmix"]),
    ("ai302", &["302.ai"]),
    ("cloudflare", &["cloudflare"]),
    ("novita", &["novita.ai"]),
    ("deepinfra", &["deepinfra"]),
    ("cerebras", &["cerebras"]),
    ("sambanova", &["sambanova"]),
    ("ppio", &["ppinfra", "ppio"]),
    ("modelscope", &["modelscope"]),
    ("qiniu", &["qiniu"]),
    ("giteeai", &["gitee"]),
    ("infinigence", &["infini-ai"]),
    ("huggingface", &["huggingface", "hf.space"]),
    ("poe", &["poe.com"]),
    ("nvidia", &["nvidia.com"]),
    ("sensenova", &["sensenova"]),
    ("hyperbolic", &["hyperbolic"]),
    ("nebius", &["nebius"]),
    ("lambda", &["lambdalabs", "lambda.ai"]),
    ("ollama", &[":11434", "ollama"]),
    ("lmstudio", &[":1234"]),
    ("xinference", &[":9997"]),
];

static MODEL_PATTERNS: LazyLock<Vec<(&'static str, Vec<Regex>)>> = LazyLock::new(|| {
    MODEL_RULES
        .iter()
        .map(|(key, patterns)| {
            let regexes = patterns
                .iter()
                .filter_map(|pattern| Regex::new(&format!("(?i){pattern}")).ok())
                .collect();
            (*key, regexes)
        })
        .collect()
});

pub fn find(key: &str) -> Option<&'static Brand> {
    BRANDS.iter().find(|brand| brand.key == key)
}

pub fn brand_for_model_id(model_id: &str) -> Option<&'static Brand> {
    let normalized: String = model_id.trim().to_lowercase().split_whitespace().collect();
    if normalized.is_empty() {
        return None;
    }
    MODEL_PATTERNS
        .iter()
        .find(|(_, regexes)| regexes.iter().any(|regex| regex.is_match(&normalized)))
        .and_then(|(key, _)| find(key))
}

/// 用户手动选的图标优先，其次按模型 ID 匹配
pub fn brand_for_model(model: &ModelConfig) -> Option<&'static Brand> {
    model
        .icon
        .as_deref()
        .and_then(find)
        .or_else(|| brand_for_model_id(&model.id))
}

pub fn brand_for_provider(provider: &ProviderConfig) -> Option<&'static Brand> {
    let url = provider.base_url.trim().to_lowercase();
    PROVIDER_RULES
        .iter()
        .find(|(_, needles)| needles.iter().any(|needle| url.contains(needle)))
        .and_then(|(key, _)| find(key))
}

pub fn embedded_icon(file: &str) -> Option<&'static [u8]> {
    BRAND_ICONS.iter().find(|(name, _)| *name == file).map(|(_, bytes)| *bytes)
}

/// 应用的资源：Lucide 图标之外再加上品牌图标
pub struct AppAssets;

impl AssetSource for AppAssets {
    fn load(&self, path: &str) -> gpui_kit::Result<Option<Cow<'static, [u8]>>> {
        if let Some(file) = path.strip_prefix("brand/") {
            return Ok(embedded_icon(file).map(Cow::Borrowed));
        }
        gpui_kit_assets::AllAssets.load(path)
    }

    fn list(&self, path: &str) -> gpui_kit::Result<Vec<SharedString>> {
        let mut paths = gpui_kit_assets::AllAssets.list(path)?;
        paths.extend(
            BRAND_ICONS
                .iter()
                .map(|(name, _)| format!("brand/{name}"))
                .filter(|name| name.starts_with(path))
                .map(SharedString::from),
        );
        Ok(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(model_id: &str) -> Option<&'static str> {
        brand_for_model_id(model_id).map(|brand| brand.key)
    }

    #[test]
    fn model_ids_match_brands() {
        assert_eq!(key("gpt-4o-mini"), Some("openai-gpt4"));
        assert_eq!(key("gpt-5-mini"), Some("openai-gpt5"));
        assert_eq!(key("o3-mini"), Some("openai-o"));
        assert_eq!(key("openai/o4-mini"), Some("openai-o"));
        assert_eq!(key("claude-sonnet-4-5-20250929"), Some("claude"));
        assert_eq!(key("anthropic/claude-3.5-sonnet"), Some("claude"));
        assert_eq!(key("gemini-2.5-pro"), Some("gemini"));
        assert_eq!(key("gemini-2.5-flash-image-preview"), Some("nanobanana"));
        assert_eq!(key("deepseek-chat"), Some("deepseek"));
        assert_eq!(key("deepseek-ai/DeepSeek-V3"), Some("deepseek"));
        assert_eq!(key("glm-4.5"), Some("zai"));
        assert_eq!(key("glm-4.5v"), Some("glmv"));
        assert_eq!(key("kimi-k2-0905-preview"), Some("moonshot"));
        assert_eq!(key("qwen-max"), Some("qwen"));
        assert_eq!(key("SenseChat-5"), Some("sensenova"));
        assert_eq!(key("doubao-seed-1-6"), Some("doubao"));
        assert_eq!(key("ERNIE-4.0-8K"), Some("wenxin"));
        assert_eq!(key("grok-4"), Some("grok"));
        assert_eq!(key("meta-llama/Llama-3.3-70B-Instruct"), Some("meta"));
        assert_eq!(key("my-own-model"), None);
    }

    #[test]
    fn every_rule_points_at_a_known_brand_and_compiles() {
        for (brand_key, patterns) in MODEL_RULES.iter().chain(PROVIDER_RULES.iter()) {
            assert!(find(brand_key).is_some(), "unknown brand {brand_key}");
            for pattern in patterns.iter() {
                assert!(Regex::new(pattern).is_ok(), "bad pattern {pattern}");
            }
        }
    }

    #[test]
    fn missing_icon_files_fall_back_to_a_letter() {
        let brand = find("claude").unwrap();
        if embedded_icon("claude.svg").is_none() {
            assert!(matches!(brand.glyph(), BrandGlyph::Letter('C')));
        }
        assert!(find("gemini").unwrap().is_light());
        assert!(!brand.is_light());
        // 白底彩色图标的品牌，字母不能也是白色
        assert_ne!(find("gemini").unwrap().letter_color(), 0xFFFFFF);
        assert_eq!(find("stepfun").unwrap().letter_color(), 0x000000);
    }
}
