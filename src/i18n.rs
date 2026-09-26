//! 界面文案的多语言表。
//!
//! 所有文案在下面的 `i18n!` 里**一处定义**，宏会展开出 `Key` 枚举、`Key::ALL`
//! 和查表函数 `tr`。这样设计是为了让"漏 key / 漏语言"变成**编译错误**：
//!
//! - 宏要求每条译文给全四种语言，漏一种就编译不过；
//! - `tr` 的 `match key` **故意不写 `_` 兜底**，将来加了 key 却忘了处理、或把 key 名写错，
//!   都是非穷尽匹配，编译期直接报错。
//!
//! 早先的实现用 `&str` 当 key，并在末尾留了 `_ => ""`：key 一旦写错，界面会**静默变成空白**，
//! 编译期无感、测试也测不到。改成枚举就是为了堵死这条路。
//!
//! 新增文案时只需在 `i18n!` 里加一行，格式是
//! `KeyName => { "简体中文", "English", "日本語", "繁體中文" },`。

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
}

/// 同步组件库自带文案（弹窗按钮等）的语言。组件库没有日语，日语时使用英文。
pub fn apply_locale(lang: AppLanguage) {
    gpui_kit::component::set_locale(match lang {
        AppLanguage::ZhCn => "zh-CN",
        AppLanguage::ZhTw => "zh-TW",
        AppLanguage::EnUs | AppLanguage::JaJp => "en",
    });
}

macro_rules! i18n {
    ($($variant:ident => { $zh_cn:expr, $en_us:expr, $ja_jp:expr, $zh_tw:expr }),* $(,)?) => {
        /// 界面文案的 key。
        ///
        /// 由 `i18n!` 宏生成，不要手工维护——加文案请改 `i18n!` 里的表。
        ///
        /// 允许 dead_code 的原因：这张表是**文案全集**，界面文案正在分批迁进来，
        /// 暂时会有一些 key 还没有调用点。等迁移做完就把这个属性删掉。
        ///
        /// 允许 enum_variant_names 的原因：表里有些 key 天然以 "Key" 结尾（如 `ApiKey`），
        /// 它们是完整的概念名，不是"以枚举名结尾的冗余变体"，这条 lint 对文案表不适用。
        #[allow(dead_code, clippy::enum_variant_names)]
        #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
        pub enum Key { $($variant),* }

        #[cfg(test)]
        impl Key {
            /// 全部 key。测试用它遍历，确保每种语言都不缺文案。
            ///
            /// 只在测试构建里存在，免得给正式构建留一个没人用的常量。
            pub const ALL: &'static [Key] = &[$(Key::$variant),*];
        }

        /// 查一条界面文案。
        ///
        /// `match` 故意不写 `_` 兜底：往 `i18n!` 加了 key 却忘了处理、或 key 名写错，
        /// 都会在编译期报错，而不是运行期悄悄返回空字符串。
        pub fn tr(lang: AppLanguage, key: Key) -> &'static str {
            match key {
                $(Key::$variant => match lang {
                    AppLanguage::ZhCn => $zh_cn,
                    AppLanguage::EnUs => $en_us,
                    AppLanguage::JaJp => $ja_jp,
                    AppLanguage::ZhTw => $zh_tw,
                }),*
            }
        }
    };
}

i18n! {
    AppTitle => { "Perch", "Perch", "Perch", "Perch" },
    Chat => { "对话", "Chat", "チャット", "對話" },
    NewChat => { "新对话", "New Chat", "新規チャット", "新對話" },
    SearchChat => { "搜索对话", "Search chats", "チャットを検索", "搜尋對話" },
    Settings => { "设置", "Settings", "設定", "設定" },
    GeneralSettings => { "通用设置", "General", "一般設定", "一般設定" },
    ProviderSettings => { "模型渠道", "Providers", "プロバイダー管理", "模型渠道" },
    McpSettings => { "MCP 服务器", "MCP Servers", "MCPサーバー", "MCP 伺服器" },
    AboutSettings => { "关于", "About", "このアプリについて", "關於" },
    LanguageSelect => { "界面语言", "Language", "表示言語", "介面語言" },
    ThemeSelect => { "外观主题 (Theme)", "Appearance Theme", "外観テーマ設定", "外觀主題" },
    SystemPrompt => { "全局系统提示词 (System Prompt)", "Global System Prompt", "グローバルシステムプロンプト", "全局系統提示詞" },
    Temperature => { "模型采样温度 (Temperature)", "Model Temperature", "モデルサンプリング温度", "模型採樣溫度" },
    AddProvider => { "添加 AI 渠道", "Add AI Provider", "AIプロバイダー追加", "新增 AI 渠道" },
    AddModel => { "添加模型", "Add Model", "モデル追加", "新增模型" },
    FetchModels => { "从接口拉取模型", "Fetch Models from API", "APIからモデル取得", "從介面拉取模型" },
    InputPlaceholder => { "输入问题或指令，Enter 发送，Shift+Enter 换行...", "Type a message, Enter to send, Shift+Enter for new line...", "メッセージを入力。Enterで送信、Shift+Enterで改行...", "輸入訊息，Enter 發送，Shift+Enter 換行..." },
    Send => { "发送", "Send", "送信", "發送" },
    Stop => { "停止", "Stop", "停止", "停止" },
    Cancel => { "取消", "Cancel", "キャンセル", "取消" },
    Save => { "保存", "Save", "保存", "儲存" },
    Delete => { "删除", "Delete", "削除", "刪除" },
    BackToChat => { "返回对话", "Back to Chat", "チャットに戻る", "返回對話" },
    ChannelType => { "渠道类型规范", "Channel Specification", "チャンネル仕様", "渠道類型規範" },
    ChannelName => { "渠道名称", "Channel Name", "チャンネル名", "渠道名稱" },
    BaseUrl => { "接口地址 (Base URL)", "API Base URL", "ベースURL", "接口地址 (Base URL)" },
    ApiKey => { "API 密钥 (API Key)", "API Key", "APIキー", "API 金鑰 (API Key)" },
    NoProviders => { "暂无渠道，请点击上方的「+」添加您自有的 AI 渠道", "No providers configured. Click '+' above to add your AI channel.", "プロバイダーがありません。上の「+」をクリックして追加してください。", "暫無渠道，請點擊上方的「+」新增自有的 AI 渠道" },
    NoModels => { "暂无模型，可点击右上角「从接口拉取模型」或「添加模型」", "No models found. Click 'Fetch Models' or 'Add Model' above.", "モデルがありません。右上の「モデル取得」または「モデル追加」をクリックしてください。", "暫無模型，可點擊右上角「從介面拉取模型」或「新增模型」" },
    LangSwitched => { "语言已切换为：简体中文", "Language switched to: English", "言語を日本語に切り替えました", "語言已切換為：繁體中文" },
}

#[cfg(test)]
mod tests {
    use super::*;

    const LANGS: [AppLanguage; 4] = [
        AppLanguage::ZhCn,
        AppLanguage::EnUs,
        AppLanguage::JaJp,
        AppLanguage::ZhTw,
    ];

    /// 每条文案在四种语言下都不能是空串。
    ///
    /// 宏能保证"漏写一个语言"编译不过，但拦不住有人把译文写成空串——那种情况界面会空白，
    /// 所以用这个测试兜住。
    #[test]
    fn every_key_has_text_in_all_languages() {
        for &key in Key::ALL {
            for lang in LANGS {
                assert!(!tr(lang, key).is_empty(), "{key:?} 在 {lang:?} 下为空");
            }
        }
    }

    /// 语言标识字符串与枚举要能互相还原。
    ///
    /// 这两个函数是配置读写与语言切换的入口，写反了会导致"切换语言后重启又变回去"。
    #[test]
    fn language_code_round_trips() {
        for lang in LANGS {
            assert_eq!(AppLanguage::from_str(lang.as_str()), lang);
        }
        // 兼容简写与地区变体
        assert_eq!(AppLanguage::from_str("en"), AppLanguage::EnUs);
        assert_eq!(AppLanguage::from_str("ja"), AppLanguage::JaJp);
        assert_eq!(AppLanguage::from_str("zh-HK"), AppLanguage::ZhTw);
        // 认不出来的一律回落简体中文
        assert_eq!(AppLanguage::from_str("fr-FR"), AppLanguage::ZhCn);
    }
}
