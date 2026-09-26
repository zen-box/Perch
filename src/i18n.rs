use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppLanguage {
    ZhCn,
    EnUs,
    JaJp,
    ZhTw,
}

impl AppLanguage {
    pub fn from_str(s: &str) -> Self {
        match s {
            "en-US" | "en" => AppLanguage::EnUs,
            "ja-JP" | "ja" => AppLanguage::JaJp,
            "zh-TW" | "zh-HK" | "zh-Hant" => AppLanguage::ZhTw,
            _ => AppLanguage::ZhCn,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            AppLanguage::ZhCn => "zh-CN",
            AppLanguage::EnUs => "en-US",
            AppLanguage::JaJp => "ja-JP",
            AppLanguage::ZhTw => "zh-TW",
        }
    }

    #[allow(dead_code)]
    pub fn display_name(&self) -> &'static str {
        match self {
            AppLanguage::ZhCn => "简体中文 (Simplified Chinese)",
            AppLanguage::EnUs => "English (US)",
            AppLanguage::JaJp => "日本語 (Japanese)",
            AppLanguage::ZhTw => "繁體中文 (Traditional Chinese)",
        }
    }

    #[allow(dead_code)]
    pub fn code_tag(&self) -> &'static str {
        match self {
            AppLanguage::ZhCn => "CN",
            AppLanguage::EnUs => "EN",
            AppLanguage::JaJp => "JA",
            AppLanguage::ZhTw => "TW",
        }
    }
}

/// 同步组件库自带文案（弹窗按钮等）的语言。组件库没有日语，日语时使用英文。
pub fn apply_locale(lang: AppLanguage) {
    gpui_kit::component::set_locale(match lang {
        AppLanguage::ZhCn => "zh-CN",
        AppLanguage::ZhTw => "zh-TW",
        AppLanguage::EnUs | AppLanguage::JaJp => "en",
    });
}

pub fn tr(lang: AppLanguage, key: &str) -> &'static str {
    match key {
        "app_title" => "Perch",
        "chat" => match lang {
            AppLanguage::ZhCn => "对话",
            AppLanguage::EnUs => "Chat",
            AppLanguage::JaJp => "チャット",
            AppLanguage::ZhTw => "對話",
        },
        "new_chat" => match lang {
            AppLanguage::ZhCn => "新对话",
            AppLanguage::EnUs => "New Chat",
            AppLanguage::JaJp => "新規チャット",
            AppLanguage::ZhTw => "新對話",
        },
        "search_chat" => match lang {
            AppLanguage::ZhCn => "搜索对话",
            AppLanguage::EnUs => "Search chats",
            AppLanguage::JaJp => "チャットを検索",
            AppLanguage::ZhTw => "搜尋對話",
        },
        "settings" => match lang {
            AppLanguage::ZhCn => "设置",
            AppLanguage::EnUs => "Settings",
            AppLanguage::JaJp => "設定",
            AppLanguage::ZhTw => "設定",
        },
        "general_settings" => match lang {
            AppLanguage::ZhCn => "通用设置",
            AppLanguage::EnUs => "General",
            AppLanguage::JaJp => "一般設定",
            AppLanguage::ZhTw => "一般設定",
        },
        "provider_settings" => match lang {
            AppLanguage::ZhCn => "模型渠道",
            AppLanguage::EnUs => "Providers",
            AppLanguage::JaJp => "プロバイダー管理",
            AppLanguage::ZhTw => "模型渠道",
        },
        "mcp_settings" => match lang {
            AppLanguage::ZhCn => "MCP 服务器",
            AppLanguage::EnUs => "MCP Servers",
            AppLanguage::JaJp => "MCPサーバー",
            AppLanguage::ZhTw => "MCP 伺服器",
        },
        "about_settings" => match lang {
            AppLanguage::ZhCn => "关于",
            AppLanguage::EnUs => "About",
            AppLanguage::JaJp => "このアプリについて",
            AppLanguage::ZhTw => "關於",
        },
        "language_select" => match lang {
            AppLanguage::ZhCn => "界面语言",
            AppLanguage::EnUs => "Language",
            AppLanguage::JaJp => "表示言語",
            AppLanguage::ZhTw => "介面語言",
        },
        "theme_select" => match lang {
            AppLanguage::ZhCn => "外观主题 (Theme)",
            AppLanguage::EnUs => "Appearance Theme",
            AppLanguage::JaJp => "外観テーマ設定",
            AppLanguage::ZhTw => "外觀主題",
        },
        "system_prompt" => match lang {
            AppLanguage::ZhCn => "全局系统提示词 (System Prompt)",
            AppLanguage::EnUs => "Global System Prompt",
            AppLanguage::JaJp => "グローバルシステムプロンプト",
            AppLanguage::ZhTw => "全局系統提示詞",
        },
        "temperature" => match lang {
            AppLanguage::ZhCn => "模型采样温度 (Temperature)",
            AppLanguage::EnUs => "Model Temperature",
            AppLanguage::JaJp => "モデルサンプリング温度",
            AppLanguage::ZhTw => "模型採樣溫度",
        },
        "add_provider" => match lang {
            AppLanguage::ZhCn => "添加 AI 渠道",
            AppLanguage::EnUs => "Add AI Provider",
            AppLanguage::JaJp => "AIプロバイダー追加",
            AppLanguage::ZhTw => "新增 AI 渠道",
        },
        "add_model" => match lang {
            AppLanguage::ZhCn => "添加模型",
            AppLanguage::EnUs => "Add Model",
            AppLanguage::JaJp => "モデル追加",
            AppLanguage::ZhTw => "新增模型",
        },
        "fetch_models" => match lang {
            AppLanguage::ZhCn => "从接口拉取模型",
            AppLanguage::EnUs => "Fetch Models from API",
            AppLanguage::JaJp => "APIからモデル取得",
            AppLanguage::ZhTw => "從介面拉取模型",
        },
        "input_placeholder" => match lang {
            AppLanguage::ZhCn => "输入问题或指令，Enter 发送，Shift+Enter 换行...",
            AppLanguage::EnUs => "Type a message, Enter to send, Shift+Enter for new line...",
            AppLanguage::JaJp => "メッセージを入力。Enterで送信、Shift+Enterで改行...",
            AppLanguage::ZhTw => "輸入訊息，Enter 發送，Shift+Enter 換行...",
        },
        "send" => match lang {
            AppLanguage::ZhCn => "发送",
            AppLanguage::EnUs => "Send",
            AppLanguage::JaJp => "送信",
            AppLanguage::ZhTw => "發送",
        },
        "stop" => match lang {
            AppLanguage::ZhCn => "停止",
            AppLanguage::EnUs => "Stop",
            AppLanguage::JaJp => "停止",
            AppLanguage::ZhTw => "停止",
        },
        "cancel" => match lang {
            AppLanguage::ZhCn => "取消",
            AppLanguage::EnUs => "Cancel",
            AppLanguage::JaJp => "キャンセル",
            AppLanguage::ZhTw => "取消",
        },
        "save" => match lang {
            AppLanguage::ZhCn => "保存",
            AppLanguage::EnUs => "Save",
            AppLanguage::JaJp => "保存",
            AppLanguage::ZhTw => "儲存",
        },
        "delete" => match lang {
            AppLanguage::ZhCn => "删除",
            AppLanguage::EnUs => "Delete",
            AppLanguage::JaJp => "削除",
            AppLanguage::ZhTw => "刪除",
        },
        "back_to_chat" => match lang {
            AppLanguage::ZhCn => "返回对话",
            AppLanguage::EnUs => "Back to Chat",
            AppLanguage::JaJp => "チャットに戻る",
            AppLanguage::ZhTw => "返回對話",
        },
        "channel_type" => match lang {
            AppLanguage::ZhCn => "渠道类型规范",
            AppLanguage::EnUs => "Channel Specification",
            AppLanguage::JaJp => "チャンネル仕様",
            AppLanguage::ZhTw => "渠道類型規範",
        },
        "channel_name" => match lang {
            AppLanguage::ZhCn => "渠道名称",
            AppLanguage::EnUs => "Channel Name",
            AppLanguage::JaJp => "チャンネル名",
            AppLanguage::ZhTw => "渠道名稱",
        },
        "base_url" => match lang {
            AppLanguage::ZhCn => "接口地址 (Base URL)",
            AppLanguage::EnUs => "API Base URL",
            AppLanguage::JaJp => "ベースURL",
            AppLanguage::ZhTw => "接口地址 (Base URL)",
        },
        "api_key" => match lang {
            AppLanguage::ZhCn => "API 密钥 (API Key)",
            AppLanguage::EnUs => "API Key",
            AppLanguage::JaJp => "APIキー",
            AppLanguage::ZhTw => "API 金鑰 (API Key)",
        },
        "no_providers" => match lang {
            AppLanguage::ZhCn => "暂无渠道，请点击上方的「+」添加您自有的 AI 渠道",
            AppLanguage::EnUs => "No providers configured. Click '+' above to add your AI channel.",
            AppLanguage::JaJp => "プロバイダーがありません。上の「+」をクリックして追加してください。",
            AppLanguage::ZhTw => "暫無渠道，請點擊上方的「+」新增自有的 AI 渠道",
        },
        "no_models" => match lang {
            AppLanguage::ZhCn => "暂无模型，可点击右上角「从接口拉取模型」或「添加模型」",
            AppLanguage::EnUs => "No models found. Click 'Fetch Models' or 'Add Model' above.",
            AppLanguage::JaJp => "モデルがありません。右上の「モデル取得」または「モデル追加」をクリックしてください。",
            AppLanguage::ZhTw => "暫無模型，可點擊右上角「從介面拉取模型」或「新增模型」",
        },
        "lang_switched" => match lang {
            AppLanguage::ZhCn => "语言已切换为：简体中文",
            AppLanguage::EnUs => "Language switched to: English",
            AppLanguage::JaJp => "言語を日本語に切り替えました",
            AppLanguage::ZhTw => "語言已切換為：繁體中文",
        },
        _ => "",
    }
}
