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

use gpui_kit::{App, Global};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum AppLanguage {
    #[default]
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
        /// 这里**故意不加 `#[allow(dead_code)]`**：迁移已经做完，每个 key 都该有调用点，
        /// 加了 key 却没人用会被编译器当成死代码报出来（4.5 收尾时就靠它清掉了 7 个
        /// 改 key 名之后剩下的遗留）。旧版留过这个属性，理由是"迁移期间会有 key 暂时没调用点"，
        /// 那个理由已经不成立了。
        ///
        /// 允许 enum_variant_names 的原因：表里有些 key 天然以 "Key" 结尾（如 `ApiKey`），
        /// 它们是完整的概念名，不是"以枚举名结尾的冗余变体"，这条 lint 对文案表不适用。
        #[allow(clippy::enum_variant_names)]
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
    Chat => { "对话", "Chat", "チャット", "對話" },
    NewChat => { "新对话", "New Chat", "新規チャット", "新對話" },
    SearchChat => { "搜索对话", "Search chats", "チャットを検索", "搜尋對話" },
    Settings => { "设置", "Settings", "設定", "設定" },
    GeneralSettings => { "通用设置", "General", "一般設定", "一般設定" },
    ProviderSettings => { "模型渠道", "Providers", "プロバイダー管理", "模型渠道" },
    McpSettings => { "MCP 服务器", "MCP Servers", "MCPサーバー", "MCP 伺服器" },
    AboutSettings => { "关于", "About", "このアプリについて", "關於" },
    LanguageSelect => { "界面语言", "Language", "表示言語", "介面語言" },
    SystemPrompt => { "全局系统提示词 (System Prompt)", "Global System Prompt", "グローバルシステムプロンプト", "全局系統提示詞" },
    Temperature => { "模型采样温度 (Temperature)", "Model Temperature", "モデルサンプリング温度", "模型採樣溫度" },
    AddModel => { "添加模型", "Add Model", "モデル追加", "新增模型" },
    FetchModels => { "从接口拉取模型", "Fetch Models from API", "APIからモデル取得", "從介面拉取模型" },
    InputPlaceholder => { "输入问题或指令，Enter 发送，Shift+Enter 换行...", "Type a message, Enter to send, Shift+Enter for new line...", "メッセージを入力。Enterで送信、Shift+Enterで改行...", "輸入訊息，Enter 發送，Shift+Enter 換行..." },
    Cancel => { "取消", "Cancel", "キャンセル", "取消" },
    Save => { "保存", "Save", "保存", "儲存" },
    Delete => { "删除", "Delete", "削除", "刪除" },
    BackToChat => { "返回对话", "Back to Chat", "チャットに戻る", "返回對話" },
    ChannelName => { "渠道名称", "Channel Name", "チャンネル名", "渠道名稱" },
    BaseUrl => { "接口地址 (Base URL)", "API Base URL", "ベースURL", "接口地址 (Base URL)" },
    ApiKey => { "API 密钥 (API Key)", "API Key", "APIキー", "API 金鑰 (API Key)" },
    NoModels => { "暂无模型，可点击右上角「从接口拉取模型」或「添加模型」", "No models found. Click 'Fetch Models' or 'Add Model' above.", "モデルがありません。右上の「モデル取得」または「モデル追加」をクリックしてください。", "暫無模型，可點擊右上角「從介面拉取模型」或「新增模型」" },
    LangSwitched => { "语言已切换为：简体中文", "Language switched to: English", "言語を日本語に切り替えました", "語言已切換為：繁體中文" },

    // 模型能力（模型信息卡片上的标签与说明）
    CapabilityVision => { "图片理解", "Vision", "画像理解", "圖片理解" },
    CapabilityFiles => { "PDF 与文档", "PDF & Docs", "PDF・文書", "PDF 與文件" },
    CapabilityTools => { "工具调用", "Tool Use", "ツール呼び出し", "工具呼叫" },
    CapabilityWebSearch => { "联网搜索", "Web Search", "ウェブ検索", "聯網搜尋" },
    CapabilityImageOutput => { "图片生成", "Image Generation", "画像生成", "圖片生成" },
    CapabilityVisionDesc => { "能看懂图片和截图", "Understands images and screenshots", "画像やスクリーンショットを理解できます", "能看懂圖片和截圖" },
    CapabilityFilesDesc => { "能直接读取 PDF 等文档", "Reads PDFs and other documents directly", "PDF などの文書を直接読み取れます", "能直接讀取 PDF 等文件" },
    CapabilityToolsDesc => { "支持函数调用，MCP 要用", "Supports function calling, required for MCP", "関数呼び出しに対応（MCP に必要）", "支援函式呼叫，MCP 需要" },
    CapabilityWebSearchDesc => { "模型自带联网搜索", "Model has built-in web search", "モデルがウェブ検索を内蔵", "模型內建聯網搜尋" },
    CapabilityImageOutputDesc => { "可以生成图片", "Can generate images", "画像を生成できます", "可以生成圖片" },

    // 思考强度档位
    ReasoningOff => { "关闭", "Off", "オフ", "關閉" },
    ReasoningMinimal => { "最小", "Minimal", "最小", "最小" },
    ReasoningLow => { "低", "Low", "低", "低" },
    ReasoningMedium => { "中", "Medium", "中", "中" },
    ReasoningHigh => { "高", "High", "高", "高" },
    ReasoningXHigh => { "超高", "Extra High", "最高", "超高" },
    ReasoningMax => { "最大", "Max", "最大", "最大" },

    // 数据层算出来、直接显示给用户的值
    NoProviderSelected => { "未选择渠道", "No provider selected", "プロバイダー未選択", "未選擇渠道" },

    // 上下文窗口 / 最大输出输入框的校验提示
    TokenNotANumber => { "「{}」不是有效的数字，可以写 128000、128K 或 1M", "「{}」is not a valid number. Try 128000, 128K or 1M", "「{}」は有効な数字ではありません（128000、128K、1M など）", "「{}」不是有效的數字，可以寫 128000、128K 或 1M" },
    TokenOutOfRange => { "「{}」超出了合理范围", "「{}」is out of the reasonable range", "「{}」は妥当な範囲を超えています", "「{}」超出了合理範圍" },

    // ---- 4.3-a：空状态 / 输入框 / 消息操作 / 模型选择器 ----
    EmptyNoProviderHint => { "还没有配置模型渠道，先添加一个吧", "No provider configured yet. Add one to get started.", "プロバイダーが未設定です。まず追加してください。", "還沒有設定模型渠道，先新增一個吧" },
    EmptyGreeting => { "今天想聊点什么？", "What would you like to talk about?", "今日は何を話しましょうか？", "今天想聊點什麼？" },
    PresetTranslate => { "翻译", "Translate", "翻訳", "翻譯" },
    PresetTranslateDesc => { "中英互译，保留原文格式", "Translate between Chinese and English, keeping the original formatting", "中国語と英語の相互翻訳（元の書式は保持）", "中英互譯，保留原文格式" },
    PresetTranslatePrompt => { "请把下面的内容翻译成英文（如果原文是英文则翻译成中文），保留原有格式：\n", "Translate the following into English (or into Chinese if it is already English), keeping the original formatting:\n", "以下の内容を英語に翻訳してください（原文が英語の場合は中国語に）。書式はそのまま保持してください：\n", "請把下面的內容翻譯成英文（如果原文是英文則翻譯成中文），保留原有格式：\n" },
    PresetPolish => { "润色文字", "Polish", "文章の推敲", "潤飾文字" },
    PresetPolishDesc => { "让表达更通顺、更专业", "Make the wording smoother and more professional", "より読みやすく、プロらしい表現に", "讓表達更通順、更專業" },
    PresetPolishPrompt => { "请帮我润色下面这段文字，使表达更通顺专业，并说明主要改动：\n", "Please polish the text below to make it smoother and more professional, and explain the main changes:\n", "以下の文章を読みやすくプロらしい表現に推敲し、主な変更点も説明してください：\n", "請幫我潤飾下面這段文字，讓表達更通順專業，並說明主要改動：\n" },
    PresetSummary => { "总结要点", "Summarize", "要点をまとめる", "總結要點" },
    PresetSummaryDesc => { "提炼长文的核心内容", "Extract the key points from a long text", "長文の核心を抽出", "提煉長文的核心內容" },
    PresetSummaryPrompt => { "请用要点的形式总结下面的内容：\n", "Please summarize the following content as bullet points:\n", "以下の内容を箇条書きで要約してください：\n", "請用要點的形式總結下面的內容：\n" },
    PresetExplainCode => { "解释一段代码", "Explain code", "コードを解説", "解釋一段程式碼" },
    PresetExplainCodeDesc => { "粘贴代码，让 AI 逐段讲解", "Paste code and let the AI walk through it", "コードを貼り付けて、AI に順に解説させます", "貼上程式碼，讓 AI 逐段講解" },
    PresetExplainCodePrompt => { "请逐段解释下面这段代码：\n", "Please explain the following code section by section:\n", "以下のコードを順を追って解説してください：\n", "請逐段解釋下面這段程式碼：\n" },
    AddModelChannel => { "添加模型渠道", "Add a provider", "プロバイダーを追加", "新增模型渠道" },
    Remove => { "移除", "Remove", "削除", "移除" },
    AttachmentUnsupported => { "「{}」不支持「{}」，发送前请换模型或先移除对应附件", "\"{}\" does not support \"{}\". Switch models or remove the attachment before sending.", "「{}」は「{}」に対応していません。送信前にモデルを切り替えるか、該当の添付を外してください。", "「{}」不支援「{}」，發送前請換模型或先移除對應附件" },
    // 附件按钮的悬停提示：受限时只列「能加什么」，不解释「为什么少了那类」——
    // 菜单本身已经只摆收得下的项，这里再念一遍限制只会把提示拉得很长
    AddAttachment => { "添加附件", "Add attachment", "添付を追加", "新增附件" },
    AddAttachmentNoFiles => { "添加附件（图片、文本与代码）", "Add attachment (images, text and code)", "添付を追加（画像・テキスト・コード）", "新增附件（圖片、文字與程式碼）" },
    AddAttachmentNoVision => { "添加附件（PDF、文本与代码）", "Add attachment (PDFs, text and code)", "添付を追加（PDF・テキスト・コード）", "新增附件（PDF、文字與程式碼）" },
    AddAttachmentTextOnly => { "添加附件（文本与代码）", "Add attachment (text and code)", "添付を追加（テキストとコード）", "新增附件（文字與程式碼）" },
    StopGenerating => { "停止生成", "Stop generating", "生成を停止", "停止生成" },
    SendEnter => { "发送 (Enter)", "Send (Enter)", "送信（Enter）", "發送 (Enter)" },
    SendCompareEnter => { "对比发送 (Enter)", "Send for comparison (Enter)", "比較送信（Enter）", "對比發送 (Enter)" },
    ClearQuote => { "取消引用", "Clear quote", "引用を解除", "取消引用" },
    PromptTemplateHint => { "提示词模板 · 回车或点击插入", "Prompt templates · press Enter or click to insert", "プロンプトテンプレート・Enter またはクリックで挿入", "提示詞模板 · Enter 或點擊插入" },
    Copy => { "复制", "Copy", "コピー", "複製" },
    EditAndResend => { "编辑并重发", "Edit and resend", "編集して再送信", "編輯並重送" },
    Quote => { "引用", "Quote", "引用", "引用" },
    Adopt => { "采用", "Use this", "採用", "採用" },
    Thinking => { "正在思考…", "Thinking…", "考えています…", "正在思考…" },
    ThinkingProcess => { "思考过程", "Reasoning", "思考プロセス", "思考過程" },
    Generating => { "正在生成…", "Generating…", "生成中…", "正在生成…" },
    ComparePickHint => { "模型对比输出中，采用一条后继续对话：", "Comparing model outputs. Pick one to continue the conversation:", "モデル比較の出力中です。1 つ採用すると会話を続けられます：", "模型對比輸出中，採用一條後繼續對話：" },
    NoModelConfigured => { "未配置模型", "No model configured", "モデル未設定", "未設定模型" },
    NoAvailableModel => { "还没有可用的模型", "No models available yet", "利用できるモデルがありません", "還沒有可用的模型" },
    NoMatchingModel => { "没有匹配的模型", "No matching model", "一致するモデルがありません", "沒有符合的模型" },
    ManageModelChannel => { "管理模型渠道", "Manage providers", "プロバイダーを管理", "管理模型渠道" },
    PromptTemplates => { "提示词", "Prompts", "プロンプト", "提示詞" },

    // ---- 4.3-b：侧边栏 / 图片渲染 / 参数面板 ----
    DefaultValue => { "默认", "Default", "デフォルト", "預設" },
    Pin => { "置顶", "Pinned", "ピン留め", "置頂" },
    Unpin => { "取消置顶", "Unpin", "ピン留めを解除", "取消置頂" },
    Favorite => { "收藏", "Favorite", "お気に入り", "收藏" },
    Unfavorite => { "取消收藏", "Remove from favorites", "お気に入りを解除", "取消收藏" },
    SidebarNoMatch => { "没有匹配的对话", "No matching chats", "一致するチャットがありません", "沒有符合的對話" },
    All => { "全部", "All", "すべて", "全部" },
    Rename => { "重命名", "Rename", "名前を変更", "重新命名" },
    RemoveFromFolder => { "移出文件夹", "Remove from folder", "フォルダから外す", "移出資料夾" },
    NewFolder => { "新建文件夹…", "New folder…", "新しいフォルダ…", "新增資料夾…" },
    MoveToFolder => { "移到「{}」", "Move to \"{}\"", "「{}」へ移動", "移到「{}」" },
    DateToday => { "今天", "Today", "今日", "今天" },
    DateYesterday => { "昨天", "Yesterday", "昨日", "昨天" },
    DateLast7Days => { "近 7 天", "Last 7 days", "過去 7 日", "近 7 天" },
    DateLast30Days => { "近 30 天", "Last 30 days", "過去 30 日", "近 30 天" },
    DateEarlier => { "更早", "Earlier", "それ以前", "更早" },
    Image => { "图片", "Image", "画像", "圖片" },
    ImageBadBase64 => { "Base64 图片数据无效或格式不支持", "The Base64 image data is invalid or in an unsupported format", "Base64 画像データが無効か、対応していない形式です", "Base64 圖片資料無效或格式不支援" },
    ImageUnsupportedUrl => { "不支持的图片地址", "Unsupported image URL", "対応していない画像 URL です", "不支援的圖片網址" },
    ImageLoading => { "图片加载中…", "Loading image…", "画像を読み込み中…", "圖片載入中…" },
    ImageLoadFailed => { "图片加载失败：{}", "Failed to load image: {}", "画像の読み込みに失敗しました：{}", "圖片載入失敗：{}" },
    ImageHttpStatus => { "服务器返回 HTTP {}", "The server returned HTTP {}", "サーバーが HTTP {} を返しました", "伺服器回傳 HTTP {}" },
    Retry => { "重试", "Retry", "再試行", "重試" },
    OpenInBrowser => { "在浏览器中打开", "Open in browser", "ブラウザで開く", "在瀏覽器中開啟" },
    ImageBadFormat => { "不是能识别的图片格式", "Not a recognizable image format", "判別できない画像形式です", "不是能辨識的圖片格式" },
    ImageRemote => { "远程图片", "Remote image", "リモート画像", "遠端圖片" },
    CopyLink => { "复制链接", "Copy link", "リンクをコピー", "複製連結" },
    ImageLinkCopied => { "图片链接已复制", "Image link copied", "画像リンクをコピーしました", "圖片連結已複製" },
    Close => { "关闭", "Close", "閉じる", "關閉" },
    ImageInlineBase64 => { "Base64 内联图片", "Inline Base64 image", "インライン Base64 画像", "Base64 內嵌圖片" },
    CopyBase64 => { "复制 Base64", "Copy Base64", "Base64 をコピー", "複製 Base64" },
    Base64Copied => { "Base64 数据已复制", "Base64 data copied", "Base64 データをコピーしました", "Base64 資料已複製" },

    // ---- 4.3-c：参数面板 / 会话视图 / 会话操作 ----
    Params => { "参数", "Parameters", "パラメータ", "參數" },
    ChatParams => { "对话参数", "Chat parameters", "会話パラメータ", "對話參數" },
    ParamsDefaultHint => { "留空或选择“默认”时使用全局设置", "Leave empty or choose “Default” to use the global setting", "空欄または「デフォルト」を選ぶと全体設定が使われます", "留空或選擇「預設」時使用全域設定" },
    TemperatureLabel => { "温度", "Temperature", "温度", "溫度" },
    MaxTokens => { "最大 tokens", "Max tokens", "最大 tokens", "最大 tokens" },
    ContextMessages => { "上下文条数", "Context messages", "コンテキスト件数", "上下文則數" },
    DefaultWithArg => { "默认（{}）", "Default ({})", "デフォルト（{}）", "預設（{}）" },
    ReasoningEffort => { "思考强度", "Reasoning effort", "思考の強度", "思考強度" },
    StreamingOutput => { "流式输出", "Streaming", "ストリーミング出力", "串流輸出" },
    RestoreDefaults => { "恢复默认", "Restore defaults", "デフォルトに戻す", "恢復預設" },
    Compare => { "对比", "Compare", "比較", "對比" },
    CompareCount => { "对比 {}", "Compare {}", "比較 {}", "對比 {}" },
    ModelCompare => { "模型对比", "Model comparison", "モデル比較", "模型對比" },
    Clear => { "清空", "Clear", "クリア", "清除" },
    CurrentModelBaseline => { "当前模型（基准）", "Current model (baseline)", "現在のモデル（基準）", "目前模型（基準）" },
    Current => { "当前", "Current", "現在", "目前" },
    ComparePickModels => { "选择 1 到 2 个模型与当前模型对比：", "Pick 1–2 models to compare against the current one:", "現在のモデルと比較するモデルを 1〜2 個選んでください：", "選擇 1 到 2 個模型與目前模型對比：" },
    NoOtherEnabledModel => { "没有其他已启用的模型", "No other enabled models", "他に有効なモデルがありません", "沒有其他已啟用的模型" },
    StartCompareWithCount => { "开始对比 ({} 个模型)", "Start comparison ({} models)", "比較を開始（{} モデル）", "開始對比（{} 個模型）" },
    PleasePickCompareModel => { "请选择对比模型", "Select models to compare", "比較するモデルを選んでください", "請選擇對比模型" },
    JumpToLatest => { "回到最新", "Jump to latest", "最新へ戻る", "回到最新" },
    MessageCount => { "{} 条消息", "{} messages", "{} 件のメッセージ", "{} 則訊息" },
    NewChatShortcut => { "新建对话 (Ctrl+N)", "New chat (Ctrl+N)", "新規チャット (Ctrl+N)", "新增對話 (Ctrl+N)" },
    ExportMarkdown => { "导出为 Markdown", "Export as Markdown", "Markdown として書き出す", "匯出為 Markdown" },
    ExportJson => { "导出 JSON 备份", "Export JSON backup", "JSON バックアップを書き出す", "匯出 JSON 備份" },
    ImportJson => { "从 JSON 备份恢复", "Restore from JSON backup", "JSON バックアップから復元", "從 JSON 備份還原" },
    ClearCurrentChat => { "清空当前对话", "Clear current chat", "現在の会話をクリア", "清空目前對話" },
    ImportConfirmHint => { "已读取 JSON 备份。恢复会覆盖本机会话、提示词和渠道配置，不会写入 API Key。", "The JSON backup has been read. Restoring overwrites local chats, prompts and provider settings; API keys are not included.", "JSON バックアップを読み込みました。復元すると本機のチャット・プロンプト・プロバイダー設定が上書きされます（API キーは含まれません）。", "已讀取 JSON 備份。還原會覆蓋本機會話、提示詞和渠道設定，不會寫入 API Key。" },
    Restore => { "恢复", "Restore", "復元", "還原" },
    AdoptCompareFirst => { "请先采用一条对比回答，再继续对话", "Adopt one of the comparison replies before continuing", "先に比較回答を 1 つ採用してから続けてください", "請先採用一則對比回答，再繼續對話" },
    FileTypeText => { "文本/代码", "Text / code", "テキスト・コード", "文字／程式碼" },
    FileTypeSheet => { "表格", "Spreadsheet", "表計算", "試算表" },
    FileTypeFile => { "文件", "File", "ファイル", "檔案" },
    ToolAuthRequired => { "需要你的授权", "Permission required", "許可が必要です", "需要你的授權" },
    ToolAuthHint => { "即将在本机执行下面的命令，请确认内容安全：", "The command below is about to run on this machine. Make sure it is safe:", "次のコマンドを本機で実行します。内容が安全か確認してください：", "即將在本機執行下面的指令，請確認內容安全：" },
    ToolAuthHintMcp => { "即将通过 MCP 服务器调用下面的工具，请确认参数安全：", "The MCP server below is about to be called. Make sure the arguments are safe:", "次のツールを MCP サーバー経由で呼び出します。引数が安全か確認してください：", "即將透過 MCP 伺服器呼叫下面的工具，請確認參數安全：" },
    Deny => { "拒绝", "Deny", "拒否", "拒絕" },
    AllowOnce => { "允许执行一次", "Allow once", "一度だけ許可", "允許執行一次" },
    CompareNeedInput => { "请在输入框输入问题或添加图片后再开始对比", "Type a question or add an image before starting a comparison", "比較を始める前に、質問を入力するか画像を追加してください", "請在輸入框輸入問題或新增圖片後再開始對比" },
    CompareNeedModels => { "请至少勾选 1 个要对比的模型", "Select at least 1 model to compare", "比較するモデルを 1 つ以上選んでください", "請至少勾選 1 個要對比的模型" },
    CompareMaxTwo => { "最多选择 2 个对比模型（共 3 个模型 PK）", "At most 2 comparison models (3 models in total)", "比較モデルは最大 2 つ（合計 3 モデル）", "最多選擇 2 個對比模型（共 3 個模型 PK）" },
    NoUserMessageToRegenerate => { "没有可重答的用户消息", "No user message to answer again", "再回答できるユーザーメッセージがありません", "沒有可重答的使用者訊息" },
    CanOnlyContinueAfterDone => { "只能在已完成的回答后继续生成", "You can only continue after a finished reply", "完了した回答の後でのみ続きを生成できます", "只能在已完成的回答後繼續生成" },
    ContinuePrompt => { "请从上次中断的地方继续，不要重复已有内容。", "Continue from where you left off, and do not repeat what you already wrote.", "前回中断したところから続けてください。既に書いた内容は繰り返さないでください。", "請從上次中斷的地方繼續，不要重複已有內容。" },
    CurrentModelUnavailable => { "当前模型不可用", "The current model is unavailable", "現在のモデルは利用できません", "目前模型無法使用" },
    CannotDeleteWhileGenerating => { "生成过程中不能删除消息", "Cannot delete a message while generating", "生成中はメッセージを削除できません", "生成過程中不能刪除訊息" },
    SessionDeleted => { "对话已删除", "Chat deleted", "チャットを削除しました", "對話已刪除" },
    ExportedTo => { "已导出至 {}", "Exported to {}", "{} に書き出しました", "已匯出至 {}" },
    ExportFailed => { "导出失败: {}", "Export failed: {}", "書き出しに失敗しました: {}", "匯出失敗: {}" },
    ChatCleared => { "当前对话已清空", "Current chat cleared", "現在の会話をクリアしました", "目前對話已清空" },

    // ---- 4.3-d：对话框 / 模型编辑 / 拉取模型 / 顶部工具条 ----
    RenameSession => { "重命名对话", "Rename chat", "チャット名を変更", "重新命名對話" },
    SessionName => { "对话名称", "Chat name", "チャット名", "對話名稱" },
    ApiStandard => { "接口规范", "API format", "API 仕様", "介面規格" },
    BaseUrlAutoHint => { "已按接口规范填入官方地址，使用代理或中转时请修改", "The official URL is pre-filled for this API format; change it if you use a proxy or relay", "API 仕様に応じた公式 URL を入力済みです。プロキシや中継を使う場合は変更してください", "已按介面規格填入官方網址，使用代理或中轉時請修改" },
    ApiKeyLabel => { "API 密钥", "API key", "API キー", "API 金鑰" },
    AddChannel => { "添加渠道", "Add provider", "プロバイダーを追加", "新增渠道" },
    RegenerateDesc => { "这条回答之后的 {} 条消息会被删除，然后重新生成这条回答。", "The {} messages after this reply will be deleted, then this reply is regenerated.", "この回答より後の {} 件のメッセージが削除され、この回答を再生成します。", "這則回答之後的 {} 則訊息會被刪除，然後重新生成這則回答。" },
    RegenerateConfirmTitle => { "重新生成这条回答？", "Regenerate this reply?", "この回答を再生成しますか？", "重新生成這則回答？" },
    Regenerate => { "重新生成", "Regenerate", "再生成", "重新生成" },
    DeleteSessionDesc => { "「{}」中的全部消息都会被删除，且无法恢复。", "All messages in “{}” will be deleted and cannot be recovered.", "「{}」のすべてのメッセージが削除され、復元できません。", "「{}」中的全部訊息都會被刪除，且無法復原。" },
    DeleteSessionTitle => { "删除这个对话？", "Delete this chat?", "このチャットを削除しますか？", "刪除這個對話？" },
    ClearChatTitle => { "清空当前对话？", "Clear the current chat?", "現在の会話をクリアしますか？", "清空目前對話？" },
    ClearChatDesc => { "对话中的全部消息都会被清空，且无法恢复。", "All messages in this chat will be cleared and cannot be recovered.", "会話内のすべてのメッセージが消去され、復元できません。", "對話中的全部訊息都會被清空，且無法復原。" },
    DeleteChannelDesc => { "「{}」及其下的全部模型配置都会被删除。", "“{}” and all of its model configurations will be deleted.", "「{}」とそのすべてのモデル設定が削除されます。", "「{}」及其下的全部模型設定都會被刪除。" },
    DeleteChannelTitle => { "删除这个渠道？", "Delete this provider?", "このプロバイダーを削除しますか？", "刪除這個渠道？" },
    EditResend => { "编辑并重新发送", "Edit and resend", "編集して再送信", "編輯並重新傳送" },
    MessageContent => { "消息内容", "Message content", "メッセージ内容", "訊息內容" },
    EditResendHint => { "保存后会删除这条消息之后的回复，并重新生成", "Saving deletes the replies after this message and regenerates them", "保存すると、このメッセージより後の回答が削除され、再生成されます", "儲存後會刪除這則訊息之後的回覆，並重新生成" },
    Resend => { "重新发送", "Resend", "再送信", "重新傳送" },
    MoveToFolderDialog => { "移动到文件夹", "Move to folder", "フォルダへ移動", "移動到資料夾" },
    FolderName => { "文件夹名称", "Folder name", "フォルダ名", "資料夾名稱" },
    FolderNameHint => { "留空则移回「默认」", "Leave empty to move back to “Default”", "空欄にすると「デフォルト」へ戻します", "留空則移回「預設」" },
    Move => { "移动", "Move", "移動", "移動" },
    EditModel => { "编辑模型", "Edit model", "モデルを編集", "編輯模型" },
    ModelId => { "模型 ID", "Model ID", "モデル ID", "模型 ID" },
    ModelIdHint => { "调用接口时使用的名字，填好后会自动识别下面的规格", "The name used when calling the API; the specs below are auto-detected once it is filled in", "API 呼び出しに使う名前です。入力すると以下の仕様が自動判定されます", "呼叫介面時使用的名稱，填好後會自動識別下面的規格" },
    DisplayName => { "显示名称", "Display name", "表示名", "顯示名稱" },
    Specs => { "规格", "Specs", "仕様", "規格" },
    ContextWindow => { "上下文窗口", "Context window", "コンテキストウィンドウ", "上下文視窗" },
    ContextWindowHint => { "一次对话最多能带上多少 token", "The most tokens a single conversation can carry", "1 回の会話で持てる最大トークン数", "一次對話最多能帶上多少 token" },
    MaxOutput => { "最大输出", "Max output", "最大出力", "最大輸出" },
    MaxOutputHint => { "单次回复的上限。Claude 必须指定，没设置对话参数时就用它", "Upper bound for a single reply. Required for Claude; used when the chat has no parameter override", "1 回の返信の上限。Claude では必須で、会話パラメータ未設定時に使われます", "單次回覆的上限。Claude 必須指定，沒設定對話參數時就用它" },
    Reasoning => { "思考", "Reasoning", "思考", "思考" },
    ManualSet => { "已手动设置", "Set manually", "手動設定済み", "已手動設定" },
    AutoDetect => { "自动识别", "Auto-detected", "自動判定", "自動識別" },
    SupportedEfforts => { "支持的强度", "Supported levels", "対応する強度", "支援的強度" },
    SupportedEffortsHint => { "对话参数里只会列出这里选中的档位，不支持调节就都不选", "Only the levels selected here appear in chat parameters; leave all unchecked if the level cannot be adjusted", "会話パラメータにはここで選んだ段階だけが表示されます。調整できない場合は何も選ばないでください", "對話參數裡只會列出這裡選中的檔位，不支援調節就都不選" },
    AlwaysThinkingHint => { "这个模型总会先思考再回答，但接口不支持调节强度", "This model always thinks before answering, but the API does not allow adjusting the level", "このモデルは常に思考してから回答しますが、API では強度を調整できません", "這個模型總會先思考再回答，但介面不支援調節強度" },
    DefaultEffort => { "默认强度", "Default level", "デフォルト強度", "預設強度" },
    DefaultEffortHint => { "对话没有单独设置时使用；「不指定」表示不发送，由接口决定", "Used when the chat has no override; “Unspecified” sends nothing and lets the API decide", "会話側で個別に設定していない場合に使われます。「指定なし」は送信せず API に任せます", "對話沒有單獨設定時使用；「不指定」表示不傳送，由介面決定" },
    PickEffortFirst => { "先在上面选择支持的强度", "Select the supported levels above first", "まず上で対応する強度を選んでください", "先在上面選擇支援的強度" },
    Unspecified => { "不指定", "Unspecified", "指定なし", "不指定" },
    Capabilities => { "能力", "Capabilities", "能力", "能力" },
    CapabilitiesHint => { "添加图片、PDF 附件和使用工具时，按这里判断模型能不能用；自动识别不准时可以手动勾选", "Decides whether the model can take image and PDF attachments and use tools; tick them by hand if auto-detection gets it wrong", "画像・PDF の添付やツールが使えるかどうかは、ここで判断します。自動判定が違う場合は手動で切り替えてください", "新增圖片、PDF 附件和使用工具時，按這裡判斷模型能不能用；自動識別不準時可以手動勾選" },
    RestoreAutoDetect => { "恢复自动识别", "Restore auto-detect", "自動判定に戻す", "恢復自動識別" },
    RestoreAutoDetectHint => { "清除手动设置的规格、能力、思考和图标", "Clear the manually set specs, capabilities, reasoning and icon", "手動設定した仕様・能力・思考・アイコンを消去します", "清除手動設定的規格、能力、思考和圖示" },
    AutoPrefix => { "自动 · {}", "Auto · {}", "自動 · {}", "自動 · {}" },
    Unknown => { "未知", "Unknown", "不明", "未知" },
    ChangeIcon => { "更换图标", "Change icon", "アイコンを変更", "更換圖示" },
    PickIcon => { "选择图标", "Choose an icon", "アイコンを選択", "選擇圖示" },
    PickIconHint => { "默认按模型 ID 自动匹配，匹配不对时可以手动指定", "Matched automatically by model ID; pick one manually if it is wrong", "既定ではモデル ID で自動照合されます。合わない場合は手動で指定できます", "預設按模型 ID 自動匹配，匹配不對時可以手動指定" },
    AutoMatch => { "自动匹配", "Auto", "自動", "自動" },
    PickModelsToAdd => { "选择要添加的模型", "Choose models to add", "追加するモデルを選択", "選擇要新增的模型" },
    FetchModelsSummary => { "可添加 {} 个，已存在 {} 个。已选 {} 个", "{} available, {} already added, {} selected", "追加可能 {} 件、既存 {} 件、選択中 {} 件", "可新增 {} 個，已存在 {} 個。已選 {} 個" },
    NoNewModelMatch => { "没有匹配的新模型", "No matching new models", "一致する新規モデルがありません", "沒有符合的新模型" },
    SelectAllCurrent => { "全选当前", "Select all", "すべて選択", "全選目前" },
    ClearCurrent => { "清空当前", "Clear", "クリア", "清除目前" },
    AddSelected => { "添加所选", "Add selected", "選択したものを追加", "新增所選" },
    ToggleSidebar => { "显示/隐藏侧边栏 (Ctrl+B)", "Show / hide sidebar (Ctrl+B)", "サイドバーの表示/非表示 (Ctrl+B)", "顯示/隱藏側邊欄 (Ctrl+B)" },
    SwitchToLight => { "切换到浅色模式", "Switch to light mode", "ライトモードに切り替え", "切換到淺色模式" },
    SwitchToDark => { "切换到深色模式", "Switch to dark mode", "ダークモードに切り替え", "切換到深色模式" },
    UsageDashboard => { "用量与费用统计看板", "Usage & cost dashboard", "使用量と費用のダッシュボード", "用量與費用統計看板" },

    // ---- 4.3-e：设置页（渠道 / 通用 / 提示词 / 关于） ----
    ModelCount => { "{} 个模型", "{} models", "{} モデル", "{} 個模型" },
    NoProviderYet => { "还没有模型渠道", "No providers yet", "プロバイダーがまだありません", "還沒有模型渠道" },
    PickProviderHint => { "选择一个渠道查看配置", "Select a provider to view its settings", "設定を見るプロバイダーを選択してください", "選擇一個渠道查看設定" },
    ProviderIntro => { "支持 OpenAI、Gemini、Claude 等接口规范，也可以接入兼容 OpenAI 的中转服务", "Supports OpenAI, Gemini and Claude API formats, plus OpenAI-compatible relays", "OpenAI・Gemini・Claude などの API 仕様に対応。OpenAI 互換の中継サービスも利用できます", "支援 OpenAI、Gemini、Claude 等介面規格，也可以接入相容 OpenAI 的中轉服務" },
    Enabled => { "已启用", "Enabled", "有効", "已啟用" },
    Disabled => { "已停用", "Disabled", "無効", "已停用" },
    DeleteProvider => { "删除渠道", "Delete provider", "プロバイダーを削除", "刪除渠道" },
    ConnectionConfig => { "连接配置", "Connection", "接続設定", "連線設定" },
    BaseUrlHint => { "一般以 /v1 结尾，例如 https://api.openai.com/v1", "Usually ends with /v1, e.g. https://api.openai.com/v1", "通常は /v1 で終わります（例：https://api.openai.com/v1）", "一般以 /v1 結尾，例如 https://api.openai.com/v1" },
    Proxy => { "代理", "Proxy", "プロキシ", "代理" },
    TimeoutSeconds => { "超时（秒）", "Timeout (s)", "タイムアウト（秒）", "逾時（秒）" },
    Retries => { "失败重试", "Retries", "再試行", "失敗重試" },
    CustomHeaders => { "自定义请求头", "Custom headers", "カスタムヘッダー", "自訂請求標頭" },
    CustomHeadersHint => { "每行一个 Name: Value", "One Name: Value per line", "1 行に 1 つ、Name: Value 形式", "每行一個 Name: Value" },
    TestConnection => { "测试连接", "Test connection", "接続をテスト", "測試連線" },
    SaveConfig => { "保存配置", "Save settings", "設定を保存", "儲存設定" },
    PinToTop => { "置顶到模型列表顶部", "Pin to the top of the model list", "モデル一覧の先頭に固定", "置頂到模型列表頂部" },
    DeleteModel => { "删除模型", "Delete model", "モデルを削除", "刪除模型" },
    ModelList => { "模型", "Models", "モデル", "模型" },
    Appearance => { "外观", "Appearance", "外観", "外觀" },
    Theme => { "主题", "Theme", "テーマ", "主題" },
    ThemeHint => { "选择浅色或深色界面", "Choose a light or dark interface", "ライトまたはダークの外観を選択", "選擇淺色或深色介面" },
    Light => { "浅色", "Light", "ライト", "淺色" },
    Dark => { "深色", "Dark", "ダーク", "深色" },
    DefaultModelForNewChat => { "新对话默认模型", "Default model for new chats", "新規チャットの既定モデル", "新對話預設模型" },
    DefaultModelHint => { "只影响之后创建的对话", "Only affects chats created afterwards", "これ以降に作成するチャットにのみ影響します", "只影響之後建立的對話" },
    SelectModel => { "选择模型", "Select a model", "モデルを選択", "選擇模型" },
    TemperatureHint => { "当前 {}，数值越低回答越稳定，越高越有创意", "Currently {}; lower is more consistent, higher is more creative", "現在 {}。低いほど安定し、高いほど創造的になります", "目前 {}，數值越低回答越穩定，越高越有創意" },
    Precise => { "精准", "Precise", "正確", "精準" },
    Balanced => { "平衡", "Balanced", "バランス", "平衡" },
    Creative => { "创意", "Creative", "創造的", "創意" },
    SystemPromptHint => { "每次对话都会作为第一条 system 消息发送给模型", "Sent to the model as the first system message of every chat", "毎回の会話で最初の system メッセージとして送信されます", "每次對話都會作為第一條 system 訊息傳送給模型" },
    LocalTools => { "本地工具", "Local tools", "ローカルツール", "本機工具" },
    EnableLocalTools => { "启用本地工具", "Enable local tools", "ローカルツールを有効化", "啟用本機工具" },
    LocalToolsHint => { "允许在对话中使用 /ls、/read、/git、/bash 指令", "Allow /ls, /read, /git and /bash commands in chats", "会話で /ls、/read、/git、/bash コマンドを使えるようにします", "允許在對話中使用 /ls、/read、/git、/bash 指令" },
    ToolTimeout => { "命令超时", "Command timeout", "コマンドのタイムアウト", "指令逾時" },
    ToolTimeoutHint => { "单条本地命令最多跑多久，到点结束进程树", "How long a single local command may run before its process tree is ended", "ローカルコマンド 1 件の最大実行時間。超えるとプロセスツリーを終了します", "單條本機指令最多執行多久，逾時會結束行程樹" },
    ToolTimeoutMinutes => { "{} 分钟", "{} min", "{} 分", "{} 分鐘" },
    GeneralSectionDesc => { "外观与默认的对话参数", "Appearance and default chat parameters", "外観と既定の会話パラメータ", "外觀與預設的對話參數" },
    McpSettingsDesc => { "通过 Model Context Protocol 为 Agent 接入外部工具", "Connect external tools to the agent via Model Context Protocol", "Model Context Protocol でエージェントに外部ツールを接続", "透過 Model Context Protocol 為 Agent 接入外部工具" },
    McpAddServer => { "添加服务器", "Add server", "サーバーを追加", "新增伺服器" },
    McpEditServer => { "编辑", "Edit", "編集", "編輯" },
    McpEmpty => { "还没有服务器。加一个之后，它提供的工具会和本机工具一起交给模型。", "No servers yet. Once you add one, its tools are offered to the model alongside the built-in ones.", "サーバーがまだありません。追加すると、そのツールが組み込みツールと一緒にモデルへ渡されます。", "還沒有伺服器。新增之後，它提供的工具會和本機工具一起交給模型。" },
    McpStatusIdle => { "未连接", "Not connected", "未接続", "未連線" },
    McpStatusConnecting => { "连接中…", "Connecting…", "接続中…", "連線中…" },
    McpStatusReady => { "已连接", "Connected", "接続済み", "已連線" },
    McpStatusFailed => { "连接失败：{}", "Connection failed: {}", "接続に失敗：{}", "連線失敗：{}" },
    McpLastError => { "上次失败：{}", "Last error: {}", "前回の失敗：{}", "上次失敗：{}" },
    McpToolCount => { "{} 个工具", "{} tools", "ツール {} 個", "{} 個工具" },
    McpConnect => { "连接", "Connect", "接続", "連線" },
    McpDisconnect => { "断开", "Disconnect", "切断", "中斷連線" },
    McpReconnect => { "重连", "Reconnect", "再接続", "重新連線" },
    McpName => { "名称", "Name", "名前", "名稱" },
    McpCommand => { "启动命令", "Command", "起動コマンド", "啟動指令" },
    McpCommandHint => { "可执行文件，例如 npx、uvx、node。Windows 上会自动补 .cmd / .exe 后缀。", "The executable, e.g. npx, uvx, node. On Windows the .cmd / .exe suffix is added automatically.", "実行ファイル（例：npx、uvx、node）。Windows では .cmd / .exe を自動補完します。", "可執行檔，例如 npx、uvx、node。Windows 上會自動補 .cmd / .exe 副檔名。" },
    McpArgs => { "启动参数", "Arguments", "起動引数", "啟動參數" },
    McpArgsHint => { "一行一个，按顺序传给命令。", "One per line, passed to the command in order.", "1 行に 1 つ、順番にコマンドへ渡します。", "一行一個，依序傳給指令。" },
    McpCwd => { "工作目录", "Working directory", "作業ディレクトリ", "工作目錄" },
    McpCwdHint => { "留空表示继承 Perch 自己的目录。", "Leave empty to inherit Perch's own directory.", "空欄の場合、Perch 自身のディレクトリを引き継ぎます。", "留空表示沿用 Perch 自己的目錄。" },
    McpEnv => { "环境变量", "Environment variables", "環境変数", "環境變數" },
    McpEnvHint => { "一行一条，格式 NAME: VALUE。值不会写进配置文件，存在系统凭据管理器里。", "One per line as NAME: VALUE. Values are never written to the config file — they live in the system credential manager.", "1 行に 1 つ、NAME: VALUE の形式。値は設定ファイルには書かれず、システムの資格情報マネージャーに保存されます。", "一行一條，格式 NAME: VALUE。值不會寫進設定檔，存放在系統認證管理員裡。" },
    McpToolsSection => { "工具", "Tools", "ツール", "工具" },
    McpToolsHint => { "停用某个工具后，它不会再出现在交给模型的清单里。", "A disabled tool is no longer offered to the model.", "無効にしたツールはモデルへ渡されません。", "停用某個工具後，它不會再出現在交給模型的清單裡。" },
    McpNoTools => { "这台服务器没有提供工具。", "This server exposes no tools.", "このサーバーはツールを提供していません。", "這台伺服器沒有提供工具。" },
    McpLogsSection => { "运行日志", "Logs", "実行ログ", "執行日誌" },
    McpLogsEmpty => { "还没有输出。", "No output yet.", "まだ出力はありません。", "還沒有輸出。" },
    McpDisabledByMaster => { "「本地工具」总开关关着，MCP 工具也不会交给模型。", "The local-tools switch is off, so MCP tools are not offered to the model either.", "「ローカルツール」が無効のため、MCP ツールもモデルへ渡されません。", "「本機工具」總開關關著，MCP 工具也不會交給模型。" },
    McpEnableMaster => { "去打开", "Turn it on", "有効にする", "去開啟" },
    McpNameRequired => { "名称和启动命令不能为空。", "Name and command are both required.", "名前と起動コマンドは必須です。", "名稱和啟動指令不能為空。" },
    McpSaveFailed => { "MCP 服务器配置保存失败: {}", "Failed to save the MCP server settings: {}", "MCPサーバー設定の保存に失敗しました: {}", "MCP 伺服器設定儲存失敗: {}" },
    McpSecretFailed => { "MCP 环境变量保存失败: {}", "Failed to save the MCP environment variables: {}", "MCP 環境変数の保存に失敗しました: {}", "MCP 環境變數儲存失敗: {}" },
    McpDeleteTitle => { "删除 MCP 服务器", "Delete MCP server", "MCPサーバーを削除", "刪除 MCP 伺服器" },
    McpDeleteDesc => { "会一并清掉「{}」存在凭据管理器里的环境变量。确定删除吗？", "This also removes the environment variables stored for \"{}\". Delete it?", "「{}」の環境変数（資格情報マネージャー内）も削除されます。よろしいですか？", "會一併清掉「{}」存在認證管理員裡的環境變數。確定刪除嗎？" },
    FeatureLocalOnly => { "数据只保存在本地，没有任何云端遥测", "Data stays on your machine; no cloud telemetry", "データはローカルのみ。クラウドへの送信はありません", "資料只保存在本機，沒有任何雲端遙測" },
    FeatureFourApis => { "支持 OpenAI Chat、OpenAI Responses、Gemini、Claude 四种接口规范", "Supports four API formats: OpenAI Chat, OpenAI Responses, Gemini and Claude", "OpenAI Chat・OpenAI Responses・Gemini・Claude の 4 つの API 仕様に対応", "支援 OpenAI Chat、OpenAI Responses、Gemini、Claude 四種介面規格" },
    FeatureStreaming => { "原生 SSE 流式解析，Markdown 实时渲染", "Native SSE streaming with live Markdown rendering", "ネイティブな SSE ストリーミング解析と Markdown のリアルタイム描画", "原生 SSE 串流解析，Markdown 即時渲染" },
    FeatureLocalTools => { "内置本地工具：/ls、/read、/git、/bash（执行前需授权）", "Built-in local tools: /ls, /read, /git, /bash (authorisation required)", "ローカルツール内蔵：/ls、/read、/git、/bash（実行前に許可が必要）", "內建本機工具：/ls、/read、/git、/bash（執行前需授權）" },
    FeatureI18n => { "界面支持简体中文、繁體中文、English、日本語", "Interface available in 简体中文, 繁體中文, English and 日本語", "UI は 简体中文・繁體中文・English・日本語 に対応", "介面支援简体中文、繁體中文、English、日本語" },
    AboutTagline => { "纯 Rust + GPUI 构建的桌面 AI 工作台", "A desktop AI workbench built in pure Rust + GPUI", "純粋な Rust + GPUI で作られたデスクトップ AI ワークベンチ", "純 Rust + GPUI 打造的桌面 AI 工作台" },
    Version => { "版本 {}", "Version {}", "バージョン {}", "版本 {}" },
    Features => { "特性", "Features", "特徴", "特色" },
    Use => { "使用", "Use", "使用", "使用" },
    PromptsIntro => { "助手预设用于新建对话，模板可在输入框输入 /名称 后回车插入", "Assistant presets are used when creating a chat; type /name in the composer and press Enter to insert a template", "アシスタントのプリセットは新規チャットで使われます。テンプレートは入力欄で /名前 と入力して Enter で挿入できます", "助手預設用於新增對話，模板可在輸入框輸入 /名稱 後按 Enter 插入" },
    AssistantPresets => { "助手预设", "Assistant presets", "アシスタントのプリセット", "助手預設" },
    NoPresetYet => { "还没有预设", "No presets yet", "プリセットがまだありません", "還沒有預設" },
    PromptTemplateList => { "提示词模板", "Prompt templates", "プロンプトテンプレート", "提示詞模板" },
    NoTemplateYet => { "还没有模板", "No templates yet", "テンプレートがまだありません", "還沒有模板" },
    NewItem => { "新建", "New", "新規作成", "新增" },
    SaveAsPreset => { "保存为预设", "Save as preset", "プリセットとして保存", "儲存為預設" },
    SaveAsTemplate => { "保存为模板", "Save as template", "テンプレートとして保存", "儲存為模板" },

    // ---- 4.3-g：助手消息操作 / 用量看板 ----
    PrevModelReply => { "上一个模型回答", "Previous model reply", "前のモデルの回答", "上一個模型回答" },
    NextModelReply => { "下一个模型回答", "Next model reply", "次のモデルの回答", "下一個模型回答" },
    CopyReply => { "复制回答", "Copy reply", "回答をコピー", "複製回答" },
    RetryWithModel => { "换模型重答", "Answer again with another model", "別のモデルで再回答", "換模型重答" },
    ContinueGenerating => { "继续生成", "Continue generating", "生成を続ける", "繼續生成" },
    PickRegenerateModel => { "选择重新生成的模型", "Choose the model to regenerate with", "再生成に使うモデルを選択", "選擇重新生成的模型" },
    NoProviderAvailable => { "没有可用的模型渠道", "No providers available", "利用できるプロバイダーがありません", "沒有可用的模型渠道" },
    CopyCode => { "复制代码", "Copy code", "コードをコピー", "複製程式碼" },
    CodeCopied => { "代码已复制", "Code copied", "コードをコピーしました", "程式碼已複製" },
    TokenBreakdown => { "Token 与费用明细", "Token & cost breakdown", "トークンと費用の内訳", "Token 與費用明細" },
    TokenInput => { "• 输入 (Input): {} tokens", "• Input: {} tokens", "• 入力 (Input): {} tokens", "• 輸入 (Input): {} tokens" },
    TokenOutput => { "• 输出 (Output): {} tokens", "• Output: {} tokens", "• 出力 (Output): {} tokens", "• 輸出 (Output): {} tokens" },
    TokenReasoning => { "  └ 思考生成: ≈{} tokens", "  └ Reasoning: ≈{} tokens", "  └ 思考生成: ≈{} tokens", "  └ 思考生成: ≈{} tokens" },
    TokenTotal => { "• 总计 (Total): {} tokens", "• Total: {} tokens", "• 合計 (Total): {} tokens", "• 總計 (Total): {} tokens" },
    TokenCost => { "• 预估费用: ${} (≈ ¥{})", "• Estimated cost: ${} (≈ ¥{})", "• 推定費用: ${} (≈ ¥{})", "• 預估費用: ${} (≈ ¥{})" },
    TokenSpeed => { "• 速率与耗时: {} tok/s · {}s", "• Speed: {} tok/s · {}s", "• 速度: {} tok/s · {}s", "• 速率與耗時: {} tok/s · {}s" },
    OverviewTab => { "Overview 概览", "Overview", "Overview 概要", "Overview 概覽" },
    ModelsTab => { "Models 模型排行", "Models", "Models モデル順位", "Models 模型排行" },
    TotalTokens => { "总 Token 消耗", "Total tokens", "総トークン消費", "總 Token 消耗" },
    TokenInOut => { "入 {} · 出 {}", "in {} · out {}", "入力 {} · 出力 {}", "入 {} · 出 {}" },
    EstimatedCost => { "预估费用", "Estimated cost", "推定費用", "預估費用" },
    CostCny => { "约合 ¥{}", "≈ ¥{}", "≈ ¥{}", "約合 ¥{}" },
    ReplyCount => { "回答条数", "Replies", "回答数", "回答則數" },
    AssistantReplies => { "条助手回复", "assistant replies", "件のアシスタント回答", "則助手回覆" },
    DailyTokenTrend => { "每日 Token 消耗走势", "Daily token usage", "日別トークン消費の推移", "每日 Token 消耗走勢" },
    PeakPerDay => { "峰值: {} / 天", "Peak: {} / day", "ピーク: {} / 日", "峰值: {} / 天" },
    DayTooltip => { "{}\n• 消耗: {} tokens (入 {} · 出 {})\n• 预估: ${} (¥{})", "{}\n• Usage: {} tokens (in {} · out {})\n• Estimated: ${} (¥{})", "{}\n• 消費: {} tokens (入力 {} · 出力 {})\n• 推定: ${} (¥{})", "{}\n• 消耗: {} tokens (入 {} · 出 {})\n• 預估: ${} (¥{})" },
    TopModelsTitle => { "模型用量排行 (Top Models)", "Top models", "モデル使用量ランキング (Top Models)", "模型用量排行 (Top Models)" },
    AllModelsTitle => { "全部模型用量与明细 (All Models)", "All models", "全モデルの使用量と明細 (All Models)", "全部模型用量與明細 (All Models)" },
    Share => { "占比", "Share", "割合", "佔比" },
    NoUsageInRange => { "暂无该时间范围内的模型用量记录", "No model usage in this period", "この期間のモデル使用量はありません", "暫無該時間範圍內的模型用量記錄" },
    ModelUsageDetail => { "{} (入 {} · 出 {})", "{} (in {} · out {})", "{} (入力 {} · 出力 {})", "{} (入 {} · 出 {})" },
    PricingNote => { "计费单价同步自 models.dev，汇率按 1 USD = 7.2 CNY 换算", "Prices synced from models.dev; converted at 1 USD = 7.2 CNY", "料金は models.dev より同期。1 USD = 7.2 CNY で換算", "計費單價同步自 models.dev，匯率按 1 USD = 7.2 CNY 換算" },
    Done => { "完成", "Done", "完了", "完成" },

    // ---- 4.3-g：助手消息操作 / 用量看板 ----
    ApiKeySaveFailed => { "API Key 保存失败: {}", "Failed to save the API key: {}", "API キーの保存に失敗しました: {}", "API Key 儲存失敗: {}" },
    ProviderSaved => { "渠道配置已保存", "Provider settings saved", "プロバイダー設定を保存しました", "渠道設定已儲存" },
    ProviderSaveFailed => { "渠道配置保存失败: {}", "Failed to save provider settings: {}", "プロバイダー設定の保存に失敗しました: {}", "渠道設定儲存失敗: {}" },
    ProviderNameUrlRequired => { "渠道名称与接口地址不能为空", "Provider name and base URL cannot be empty", "プロバイダー名とベース URL は必須です", "渠道名稱與介面網址不能為空" },
    ProviderAddFailed => { "渠道添加失败: {}", "Failed to add the provider: {}", "プロバイダーの追加に失敗しました: {}", "渠道新增失敗: {}" },
    ProviderAdded => { "渠道已添加，可以从接口拉取模型或手动添加模型", "Provider added. You can fetch models from the API or add them manually", "プロバイダーを追加しました。API からモデルを取得するか、手動で追加できます", "渠道已新增，可以從介面拉取模型或手動新增模型" },
    ProviderDeleteFailed => { "渠道删除失败: {}", "Failed to delete the provider: {}", "プロバイダーの削除に失敗しました: {}", "渠道刪除失敗: {}" },
    ProviderKeyCleanupFailed => { "渠道已删除，但凭据清理失败: {}", "Provider deleted, but clearing its credentials failed: {}", "プロバイダーは削除しましたが、認証情報の削除に失敗しました: {}", "渠道已刪除，但憑證清理失敗: {}" },
    ProviderDeleted => { "渠道已删除", "Provider deleted", "プロバイダーを削除しました", "渠道已刪除" },
    ModelDeleted => { "模型已删除", "Model deleted", "モデルを削除しました", "模型已刪除" },
    ConfigSaveFailed => { "配置保存失败: {}", "Failed to save settings: {}", "設定の保存に失敗しました: {}", "設定儲存失敗: {}" },
    FetchingModels => { "正在从接口拉取模型列表…", "Fetching the model list from the API…", "API からモデル一覧を取得しています…", "正在從介面拉取模型列表…" },
    FetchModelsFailed => { "拉取失败: {}", "Fetch failed: {}", "取得に失敗しました: {}", "拉取失敗: {}" },
    NoModelsReturned => { "接口没有返回模型", "The API returned no models", "API がモデルを返しませんでした", "介面沒有回傳模型" },
    FetchedModels => { "拉取到 {} 个模型，请选择要添加的", "Fetched {} models; choose the ones to add", "{} 個のモデルを取得しました。追加するものを選んでください", "拉取到 {} 個模型，請選擇要新增的" },
    PickAtLeastOneModel => { "请至少选择一个模型", "Select at least one model", "モデルを 1 つ以上選んでください", "請至少選擇一個模型" },
    ModelsAlreadyAdded => { "所选模型都已经添加过了", "All selected models have already been added", "選択したモデルはすでに追加済みです", "所選模型都已經新增過了" },
    ModelsAdded => { "已添加 {} 个模型", "Added {} models", "{} 個のモデルを追加しました", "已新增 {} 個模型" },
    ModelListSaveFailed => { "模型列表保存失败: {}", "Failed to save the model list: {}", "モデル一覧の保存に失敗しました: {}", "模型列表儲存失敗: {}" },
    TestingConnection => { "正在测试渠道连接…", "Testing the provider connection…", "プロバイダー接続をテストしています…", "正在測試渠道連線…" },
    ConnectionTestFailed => { "连接测试失败: {}", "Connection test failed: {}", "接続テストに失敗しました: {}", "連線測試失敗: {}" },
    ConnectionOk => { "连接成功，发现 {} 个模型", "Connected; {} models found", "接続に成功しました。{} 個のモデルが見つかりました", "連線成功，發現 {} 個模型" },
    PhSessionName => { "输入新的对话名称", "Enter a new chat name", "新しいチャット名を入力", "輸入新的對話名稱" },
    PhSearchProvider => { "搜索渠道", "Search providers", "プロバイダーを検索", "搜尋渠道" },
    PhSearchModelOrProvider => { "搜索模型或渠道", "Search models or providers", "モデルまたはプロバイダーを検索", "搜尋模型或渠道" },
    PhSystemPrompt => { "例如：你是一名资深的 Rust 工程师，回答简洁并给出可运行的示例。", "e.g. You are a senior Rust engineer. Answer concisely and include runnable examples.", "例：あなたは熟練した Rust エンジニアです。簡潔に答え、動作する例を示してください。", "例如：你是一名資深的 Rust 工程師，回答簡潔並給出可執行的範例。" },
    PhProviderName => { "例如：OpenAI 官方 / DeepSeek / 公司代理", "e.g. OpenAI official / DeepSeek / company relay", "例：OpenAI 公式 / DeepSeek / 社内プロキシ", "例如：OpenAI 官方 / DeepSeek / 公司代理" },
    PhModelId => { "调用接口时使用的名字，例如 claude-sonnet-4-5", "The name used when calling the API, e.g. claude-sonnet-4-5", "API 呼び出しに使う名前（例：claude-sonnet-4-5）", "呼叫介面時使用的名稱，例如 claude-sonnet-4-5" },
    PhModelDisplayName => { "留空则显示模型 ID", "Leave empty to show the model ID", "空欄の場合はモデル ID を表示", "留空則顯示模型 ID" },
    PhCustomContextLimit => { "自定义，如 128K", "Custom, e.g. 128K", "カスタム（例：128K）", "自訂，如 128K" },
    PhCustomOutputLimit => { "自定义，如 64K", "Custom, e.g. 64K", "カスタム（例：64K）", "自訂，如 64K" },
    PhSessionSystemPrompt => { "留空则使用全局系统提示词", "Leave empty to use the global system prompt", "空欄の場合は全体のシステムプロンプトを使用", "留空則使用全域系統提示詞" },
    PhName => { "名称", "Name", "名前", "名稱" },
    PhIcon => { "例如：✨", "e.g. ✨", "例：✨", "例如：✨" },
    PhTemplateVars => { "支持 {{date}} {{clipboard}} {{selection}}", "Supports {{date}} {{clipboard}} {{selection}}", "{{date}} {{clipboard}} {{selection}} が使えます", "支援 {{date}} {{clipboard}} {{selection}}" },
    PhModelSearch => { "搜索模型 ID 或名称", "Search model ID or name", "モデル ID または名前を検索", "搜尋模型 ID 或名稱" },
    MigrationFailed => { "旧数据迁移失败，可能读不到历史数据：{}", "Migrating old data failed; past data may be unreadable: {}", "旧データの移行に失敗しました。過去のデータを読み込めない可能性があります：{}", "舊資料移轉失敗，可能讀不到歷史資料：{}" },
    ChatSaveFailed => { "对话保存失败: {}", "Failed to save the chat: {}", "チャットの保存に失敗しました: {}", "對話儲存失敗: {}" },
    LocalToolSettingsSaveFailed => { "本地工具设置保存失败: {}", "Failed to save local tool settings: {}", "ローカルツール設定の保存に失敗しました: {}", "本機工具設定儲存失敗: {}" },
    LocalToolsDisabled => { "本地工具未启用", "Local tools are disabled", "ローカルツールが無効です", "本機工具未啟用" },
    AgentRoundLimit => { "这一轮工具调用太多了，已经停下。可以让模型继续，或者换个更明确的问法。", "Too many tool calls in one turn; stopping here. You can ask the model to continue, or be more specific.", "1 ターン内のツール呼び出しが多すぎるため停止しました。続きを依頼するか、より具体的に指示してください。", "這一輪工具呼叫太多，已經停下。可以請模型繼續，或換個更明確的問法。" },
    ToolResultTitle => { "工具结果", "Tool result", "ツールの結果", "工具結果" },
    ToolRunning => { "正在执行…", "Running…", "実行中…", "正在執行…" },
    ToolExitCode => { "退出码 {}", "Exit code {}", "終了コード {}", "退出碼 {}" },
    LocalOnlyNote => { "仅在本机显示，不会发给模型", "Shown only on this computer — never sent to the model", "このパソコンでのみ表示され、モデルには送信されません", "僅在本機顯示，不會傳給模型" },
    ApprovalPendingFirst => { "请先允许或拒绝上面的工具调用", "Allow or deny the pending tool call first", "先に保留中のツール呼び出しを許可または拒否してください", "請先允許或拒絕上面的工具呼叫" },
    ApprovalPendingElsewhere => { "对话「{}」正在等你授权工具调用", "“{}” is waiting for you to approve a tool call", "「{}」でツール呼び出しの承認を待っています", "對話「{}」正在等你授權工具呼叫" },
    GoToChat => { "前往", "Open", "開く", "前往" },
    ToolPreviewLines => { "内容预览：前 {} 行，共 {} 行", "Preview: first {} of {} lines", "プレビュー：先頭 {} 行（全 {} 行）", "內容預覽：前 {} 行，共 {} 行" },
    ShowAllLines => { "展开全部（共 {} 行）", "Show all ({} lines)", "すべて表示（{} 行）", "展開全部（共 {} 行）" },
    CollapseResult => { "收起", "Collapse", "折りたたむ", "收起" },
    CopiedToClipboard => { "已复制到剪贴板", "Copied to clipboard", "クリップボードにコピーしました", "已複製到剪貼簿" },
    SystemPromptSaved => { "系统提示词已保存", "System prompt saved", "システムプロンプトを保存しました", "系統提示詞已儲存" },
    AddAttachmentFailed => { "添加附件失败：{}", "Failed to add attachments: {}", "添付ファイルの追加に失敗しました：{}", "新增附件失敗：{}" },
    AttachmentsAdded => { "已添加 {} 个附件", "Added {} attachments", "添付ファイルを {} 件追加しました", "已新增 {} 個附件" },
    PickAttachmentTitle => { "选择附件", "Choose attachments", "添付ファイルを選択", "選擇附件" },
    FilterImages => { "图片文件", "Image files", "画像ファイル", "圖片檔案" },
    FilterDocuments => { "PDF 文档", "PDF documents", "PDF 文書", "PDF 文件" },
    FilterTextCode => { "文本与代码", "Text & code", "テキストとコード", "文字與程式碼" },
    CannotPasteFolder => { "不能粘贴文件夹，请选择文件夹里的文件", "Folders cannot be pasted; pick files inside the folder instead", "フォルダは貼り付けできません。フォルダ内のファイルを選んでください", "不能貼上資料夾，請選擇資料夾裡的檔案" },
    PasteImageFailed => { "粘贴图片失败：{}", "Failed to paste the image: {}", "画像の貼り付けに失敗しました：{}", "貼上圖片失敗：{}" },
    ImagePasted => { "已从剪贴板粘贴图片", "Image pasted from the clipboard", "クリップボードから画像を貼り付けました", "已從剪貼簿貼上圖片" },
    OpenFileFailed => { "打开文件失败: {}", "Failed to open the file: {}", "ファイルを開けませんでした: {}", "開啟檔案失敗: {}" },
    AttachmentNotAdded => { "「{}」没有添加：{}", "“{}” was not added: {}", "「{}」は追加できませんでした：{}", "「{}」沒有新增：{}" },
    AttachmentNeedsCapability => { "「{}」不支持「{}」", "“{}” does not support “{}”", "「{}」は「{}」に対応していません", "「{}」不支援「{}」" },
    AttachmentCapabilityHint => { "图片和 PDF 要模型支持才能发：换个模型试试；如果是识别错了，可以在编辑模型时手动勾上对应能力", "Images and PDFs need a model that supports them: try another model, or if the detection is wrong, tick the capability by hand when editing the model", "画像と PDF は対応しているモデルでないと送れません。別のモデルを使うか、判定が間違っている場合はモデルの編集で手動でオンにしてください", "圖片和 PDF 要模型支援才能發：換個模型試試；如果是識別錯了，可以在編輯模型時手動勾上對應能力" },
    AttachmentOfficeUnsupported => { "模型读不了 Word / Excel / PPT 文件", "Models cannot read Word / Excel / PowerPoint files", "Word / Excel / PowerPoint ファイルはモデルが読めません", "模型讀不了 Word / Excel / PPT 檔案" },
    AttachmentOfficeHint => { "Word / Excel / PPT 可以换个格式：表格另存为 CSV，文档另存为 PDF，或者直接把内容粘贴进来", "Convert Word / Excel / PowerPoint files first: save spreadsheets as CSV and documents as PDF, or paste the content in directly", "Word / Excel / PowerPoint は形式を変えてください。表は CSV、文書は PDF で保存し直すか、内容を直接貼り付けてください", "Word / Excel / PPT 可以換個格式：表格另存為 CSV，文件另存為 PDF，或者直接把內容貼上進來" },
    AttachmentNotUtf8 => { "不是 UTF-8 编码的文本（可能是 GBK），模型收不到", "The text is not UTF-8 encoded, so the model would not receive it", "テキストが UTF-8 ではないため、モデルに届きません", "不是 UTF-8 編碼的文字（可能是 Big5），模型收不到" },
    AttachmentNotUtf8Hint => { "文本文件请先另存为 UTF-8 编码（记事本「另存为」里就能选编码），再重新添加", "Save text files as UTF-8 first (Notepad's Save As lets you pick the encoding), then add them again", "テキストファイルは UTF-8 で保存し直してから追加してください（メモ帳の「名前を付けて保存」で文字コードを選べます）", "文字檔請先另存為 UTF-8 編碼（記事本「另存新檔」裡就能選編碼），再重新新增" },
    AttachmentBinaryUnsupported => { "不是图片、PDF 或文本文件，模型读不了", "Not an image, PDF or text file, so models cannot read it", "画像・PDF・テキストのどれでもないため、モデルは読めません", "不是圖片、PDF 或文字檔，模型讀不了" },
    AttachmentImageUnreadable => { "图片读不出来：{}", "The image could not be read: {}", "画像を読み込めませんでした：{}", "圖片讀不出來：{}" },
    SaveClipboardImageFailed => { "保存剪贴板图片失败：{}", "Failed to save the clipboard image: {}", "クリップボードの画像を保存できませんでした：{}", "儲存剪貼簿圖片失敗：{}" },
    BackupExported => { "已导出备份 {}", "Backup exported to {}", "バックアップを書き出しました {}", "已匯出備份 {}" },
    PickBackupTitle => { "选择 JSON 备份", "Choose a JSON backup", "JSON バックアップを選択", "選擇 JSON 備份" },
    FilterJsonBackup => { "JSON 备份文件", "JSON backup files", "JSON バックアップファイル", "JSON 備份檔案" },
    BackupLoaded => { "已读取备份，请确认是否恢复", "Backup loaded; confirm whether to restore", "バックアップを読み込みました。復元するか確認してください", "已讀取備份，請確認是否還原" },
    PromptsSaveFailed => { "提示词保存失败: {}", "Failed to save prompts: {}", "プロンプトの保存に失敗しました: {}", "提示詞儲存失敗: {}" },
    BackupRestored => { "备份已恢复。API Key 需在本机凭据中存在，否则请重新填写", "Backup restored. API keys must exist in the local credential store; fill them in again otherwise", "バックアップを復元しました。API キーは本機の資格情報に存在する必要があります。ない場合は再入力してください", "備份已還原。API Key 需在本機憑證中存在，否則請重新填寫" },

    // ---- 4.4-a：渠道操作 / 应用层提示 / 附件 / 备份 ----
    ErrNoBaseUrl => { "未配置接口基础地址 (Base URL)，请在设置中配置渠道。", "No API base URL configured. Set one up under Settings → Providers.", "API のベース URL が未設定です。設定 → プロバイダーで設定してください。", "未設定介面基礎位址 (Base URL)，請在設定中設定管道。" },
    ErrNoApiKey => { "未配置 API 密钥 (API Key)。\n请进入设置 -> 渠道与服务商 填入该渠道的有效 API Key。", "No API key configured.\nOpen Settings → Providers and fill in a valid key for this provider.", "API キーが未設定です。\n設定 → プロバイダーで、このプロバイダーの有効な API キーを入力してください。", "未設定 API 金鑰 (API Key)。\n請進入設定 -> 管道與服務商 填入該管道的有效 API Key。" },
    ErrBadProxy => { "代理地址无效: {}", "Invalid proxy address: {}", "プロキシアドレスが無効です: {}", "代理位址無效: {}" },
    ErrHttpStatus => { "{}返回 HTTP {}\n响应: {}", "{} returned HTTP {}\nResponse: {}", "{} が HTTP {} を返しました\nレスポンス: {}", "{}回傳 HTTP {}\n回應: {}" },
    ErrConnectFailed => { "连接{}失败: {}", "Failed to connect to {}: {}", "{} への接続に失敗しました: {}", "連線{}失敗: {}" },
    ErrRequestUrl => { "{}\n请求地址: {}", "{}\nRequest URL: {}", "{}\nリクエスト URL: {}", "{}\n請求位址: {}" },
    ErrBadJson => { "响应不是有效 JSON: {}", "The response is not valid JSON: {}", "レスポンスが有効な JSON ではありません: {}", "回應不是有效 JSON: {}" },
    ErrTransferInterrupted => { "传输中断: {}", "The transfer was interrupted: {}", "転送が中断されました: {}", "傳輸中斷: {}" },
    ErrTimeout => { "{}超过 {} 秒没有返回数据，连接已超时。可以在渠道设置里调大「超时」。", "{} returned no data for over {} seconds and the connection timed out. You can raise “Timeout” in the provider settings.", "{} が {} 秒以上データを返さなかったため、接続がタイムアウトしました。プロバイダー設定で「タイムアウト」を大きくできます。", "{}超過 {} 秒沒有回傳資料，連線已逾時。可以在管道設定裡調大「逾時」。" },
    ErrBadHeaderName => { "自定义请求头名称无效: {}", "Invalid custom header name: {}", "カスタムヘッダー名が無効です: {}", "自訂請求標頭名稱無效: {}" },
    ErrBadHeaderValue => { "自定义请求头的值包含非法字符: {}", "A custom header value contains illegal characters: {}", "カスタムヘッダーの値に不正な文字が含まれています: {}", "自訂請求標頭的值包含非法字元: {}" },
    LabelClaudeChannel => { "Claude 渠道", "Claude provider", "Claude プロバイダー", "Claude 管道" },
    LabelGeminiChannel => { "Gemini 渠道", "Gemini provider", "Gemini プロバイダー", "Gemini 管道" },
    LabelOpenAiChannel => { "OpenAI 渠道", "OpenAI provider", "OpenAI プロバイダー", "OpenAI 管道" },
    ErrNoChannelCreated => { "尚未创建任何 AI 渠道。\n\n点击右上角的设置图标，进入「模型渠道」添加你的第一个渠道。", "No AI provider yet.\n\nClick the settings icon in the top right and open “Providers” to add your first one.", "AI プロバイダーがまだありません。\n\n右上の設定アイコンから「プロバイダー」を開き、最初のプロバイダーを追加してください。", "尚未建立任何 AI 管道。\n\n點擊右上角的設定圖示，進入「模型管道」新增你的第一個管道。" },
    ErrNoModelInChannel => { "当前渠道没有可用模型，请先添加或启用模型。", "This provider has no usable model. Add or enable one first.", "このプロバイダーには使えるモデルがありません。先にモデルを追加するか有効にしてください。", "目前管道沒有可用模型，請先新增或啟用模型。" },
    ErrNotEnoughModels => { "选中的模型里没有足够的可用模型", "Not enough usable models among the selected ones", "選択したモデルの中に、使えるモデルが足りません", "選中的模型裡沒有足夠的可用模型" },
    ErrCreateHttpClient => { "无法创建 HTTP 客户端: {}", "Could not create the HTTP client: {}", "HTTP クライアントを作成できません: {}", "無法建立 HTTP 客戶端: {}" },
    ErrConnectFailedShort => { "连接失败: {}", "Connection failed: {}", "接続に失敗しました: {}", "連線失敗: {}" },
    ErrApiHttpStatus => { "接口返回 HTTP {}: {}", "The API returned HTTP {}: {}", "API が HTTP {} を返しました: {}", "介面回傳 HTTP {}: {}" },
    ErrModelsNotJson => { "模型列表不是有效 JSON: {}", "The model list is not valid JSON: {}", "モデル一覧が有効な JSON ではありません: {}", "模型清單不是有效 JSON: {}" },
    ErrInvalidBaseUrl => { "接口地址无效: {}", "Invalid API address: {}", "API アドレスが無効です: {}", "介面位址無效: {}" },
    ErrNoModelList => { "接口响应缺少模型列表", "The API response has no model list", "API レスポンスにモデル一覧がありません", "介面回應缺少模型清單" },

    // ---- 4.4-b：请求链路的错误信息 ----
    ImageBlockedHost => { "{} 指向本机或内网地址，已阻止加载", "{} points to a local or private address; loading was blocked", "{} は本機またはプライベートアドレスを指しているため、読み込みをブロックしました", "{} 指向本機或內網位址，已阻止載入" },
    ImageTooManyRedirects => { "图片重定向次数过多", "Too many image redirects", "画像のリダイレクトが多すぎます", "圖片重新導向次數過多" },
    ImageRedirectBlocked => { "图片重定向到了本机或内网地址，已阻止", "The image redirected to a local or private address; blocked", "画像が本機またはプライベートアドレスへリダイレクトされたため、ブロックしました", "圖片重新導向到了本機或內網位址，已阻止" },
    ImageOnlyHttp => { "只支持 http/https 图片: {}", "Only http/https images are supported: {}", "http/https の画像のみ対応しています: {}", "只支援 http/https 圖片: {}" },
    ImageBlockedAddress => { "不加载本机或内网地址的图片", "Images from local or private addresses are not loaded", "本機またはプライベートアドレスの画像は読み込みません", "不載入本機或內網位址的圖片" },
    ImageRequestCancelled => { "图片请求已取消", "The image request was cancelled", "画像リクエストがキャンセルされました", "圖片請求已取消" },
    ImageDownloadFailed => { "无法下载图片 {}: {}", "Could not download the image {}: {}", "画像をダウンロードできません {}: {}", "無法下載圖片 {}: {}" },
    ImageTooLarge => { "图片过大（超过 16MB）: {}", "The image is too large (over 16 MB): {}", "画像が大きすぎます（16MB 超）: {}", "圖片過大（超過 16MB）: {}" },
    ImageReadFailed => { "读取图片失败 {}: {}", "Failed to read the image {}: {}", "画像の読み込みに失敗しました {}: {}", "讀取圖片失敗 {}: {}" },
    ClipboardSvgUnsupported => { "暂不支持粘贴 SVG 图片", "Pasting SVG images is not supported yet", "SVG 画像の貼り付けにはまだ対応していません", "尚未支援貼上 SVG 圖片" },
    ClipboardImageUnrecognized => { "剪贴板里的图片无法识别：{}", "The image in the clipboard is not recognised: {}", "クリップボードの画像を判別できません：{}", "剪貼簿裡的圖片無法辨識：{}" },
    ClipboardImageConvertFailed => { "剪贴板图片转换失败：{}", "Failed to convert the clipboard image: {}", "クリップボード画像の変換に失敗しました：{}", "剪貼簿圖片轉換失敗：{}" },
    AttachmentTooLarge => { "文件有 {} MB，超过了 {} MB 的上限", "The file is {} MB, over the {} MB limit", "ファイルは {} MB で、上限の {} MB を超えています", "檔案有 {} MB，超過了 {} MB 的上限" },
    BackupParseFailed => { "备份文件无法解析: {}", "The backup file could not be parsed: {}", "バックアップファイルを解析できません: {}", "備份檔案無法解析: {}" },
    BackupVersionUnsupported => { "不支持的备份版本: {}", "Unsupported backup version: {}", "対応していないバックアップバージョンです: {}", "不支援的備份版本: {}" },
    MigrationCopyDir => { "{} 复制到 {} 失败，继续使用旧目录", "Failed to copy {} to {}; continuing with the old directory", "{} を {} へコピーできませんでした。旧ディレクトリを引き続き使用します", "{} 複製到 {} 失敗，繼續使用舊目錄" },
    MigrationCopyFile => { "{} 复制为 {} 失败: {}", "Failed to copy {} to {}: {}", "{} を {} へコピーできませんでした: {}", "{} 複製為 {} 失敗: {}" },

    // ---- 启动失败页（TECH_DEBT #3）----
    StartupFailedTitle => { "启动失败", "Failed to start", "起動できませんでした", "啟動失敗" },
    StartupFailedHint => { "配置或数据文件读不出来。修好之后重新启动，或者先打开数据目录看看。", "The configuration or data files could not be read. Fix them and restart, or open the data folder first.", "設定またはデータファイルを読み込めませんでした。修正して再起動するか、先にデータフォルダーを開いて確認してください。", "設定或資料檔案讀不出來。修好之後重新啟動，或先開啟資料目錄看看。" },
    StartupRuntimeFailed => { "无法启动后台运行时：{}", "Could not start the background runtime: {}", "バックグラウンド実行環境を起動できません：{}", "無法啟動背景執行環境：{}" },
    StartupConfigFailed => { "读取配置失败：{}", "Could not read the configuration: {}", "設定を読み込めませんでした：{}", "讀取設定失敗：{}" },
    StartupStorageFailed => { "读写会话数据失败：{}", "Could not read or write the session data: {}", "セッションデータの読み書きに失敗しました：{}", "讀寫工作階段資料失敗：{}" },
    StartupDataDir => { "数据目录：{}", "Data folder: {}", "データフォルダー：{}", "資料目錄：{}" },
    StartupOpenDataDir => { "打开数据目录", "Open data folder", "データフォルダーを開く", "開啟資料目錄" },
    StartupOpenDataDirFailed => { "打开数据目录失败：{}", "Could not open the data folder: {}", "データフォルダーを開けませんでした：{}", "開啟資料目錄失敗：{}" },
    StartupQuit => { "退出", "Quit", "終了", "結束" },

    // ---- 4.4-c：图片与文件链路 ----
    ModelIdRequired => { "模型 ID 不能为空", "The model ID cannot be empty", "モデル ID は必須です", "模型 ID 不能為空" },
    ModelAlreadyExists => { "这个渠道已经有模型「{}」了", "This provider already has a model named “{}”", "このプロバイダーには既にモデル「{}」があります", "這個管道已經有模型「{}」了" },
    ModelAdded => { "模型已添加", "Model added", "モデルを追加しました", "模型已新增" },
    ModelSaved => { "模型设置已保存", "Model settings saved", "モデル設定を保存しました", "模型設定已儲存" },
    ModelSaveFailed => { "模型保存失败: {}", "Failed to save the model: {}", "モデルの保存に失敗しました: {}", "模型儲存失敗: {}" },
    NameAndBodyRequired => { "名称和内容不能为空", "The name and the content cannot be empty", "名前と内容は必須です", "名稱和內容不能為空" },
    SaveFailed => { "保存失败: {}", "Failed to save: {}", "保存に失敗しました: {}", "儲存失敗: {}" },
    Saved => { "已保存", "Saved", "保存しました", "已儲存" },
    DeleteFailed => { "删除失败: {}", "Failed to delete: {}", "削除に失敗しました: {}", "刪除失敗: {}" },
    DefaultPresetName => { "通用助手", "General assistant", "汎用アシスタント", "通用助手" },
    PresetExplainCodeTemplate => { "请逐段解释下面这段代码：\n{{selection}}", "Please explain the following code section by section:\n{{selection}}", "以下のコードを順を追って解説してください：\n{{selection}}", "請逐段解釋下面這段程式碼：\n{{selection}}" },

    // ---- 会话级工具选择（composer 上的模式开关与工具选择器）----
    AgentMode => { "智能体", "Agent", "エージェント", "智能體" },
    SessionTools => { "本次对话的工具", "Tools for this chat", "この会話のツール", "本次對話的工具" },
    SelectAllTools => { "全选", "Select all", "すべて選択", "全選" },
    ClearAllTools => { "全不选", "Select none", "すべて解除", "全不選" },
    SessionToolsChatHint => { "当前是对话模式，工具不会启用。切到「智能体」后，下面勾选的工具才会生效。", "This chat is in Chat mode, so no tools are offered. Switch to Agent and the tools checked below take effect.", "現在はチャットモードのためツールは使われません。「エージェント」に切り替えると、以下で選択したツールが有効になります。", "目前是對話模式，工具不會啟用。切到「智能體」後，下面勾選的工具才會生效。" },
    SessionToolsEmpty => { "还没有可用的工具。到设置里打开「本地工具」，或者添加一台 MCP 服务器。", "No tools available yet. Turn on “Local tools” in Settings, or add an MCP server.", "利用できるツールがまだありません。設定で「ローカルツール」を有効にするか、MCP サーバーを追加してください。", "還沒有可用的工具。到設定裡開啟「本機工具」，或新增一台 MCP 伺服器。" },
    SessionToolsModelHint => { "当前模型不支持「工具调用」，勾了也不会生效。", "The current model does not support tool use, so nothing here will take effect.", "現在のモデルはツール呼び出しに対応していないため、選択しても効果がありません。", "目前模型不支援「工具呼叫」，勾了也不會生效。" },
}

/// 按顺序替换文案里的 `{}` 占位符。
///
/// 为什么不写成 `format!(tr(...), arg)`：`format!` 的格式串必须是编译期字面量，
/// 把 `tr(...)` 的返回值当格式串会报 `format argument must be a string literal`。
/// 语序差异（英语常把数值放句首、日语放句中）靠译文里 `{}` 的位置解决，不靠代码。
pub fn tr_args(lang: AppLanguage, key: Key, args: &[&str]) -> String {
    let mut text = tr(lang, key).to_string();
    for arg in args {
        text = text.replacen("{}", arg, 1);
    }
    text
}

/// 当前界面语言的一份全局镜像。
///
/// **为什么需要镜像**：绝大多数界面函数能从 `&AppState` 拿到语言，直接
/// `tr(lang, key)` 最清楚；但有些回调的签名是 GPUI 定死的——markdown 自定义元素的
/// `render`、对话框的内容闭包——手里只有 `&App`，拿不到 `AppState`。
/// 这类地方用 [`current`] 取一次语言，再照常 `tr(lang, key)`，其余地方一律显式传 `lang`。
///
/// 全局值在 `AppState` 构造时和每次 [`crate::app::AppState::switch_language`] 之后同步，
/// 没设过时回落 [`AppLanguage::default`]（简体中文），所以读它不会 panic。
#[derive(Clone, Copy, Default)]
pub struct CurrentLanguage(pub AppLanguage);

impl Global for CurrentLanguage {}

/// 把语言同步进全局。启动时与每次切换语言后都要调用，否则全局值会落后于配置。
pub fn set_current(cx: &mut App, lang: AppLanguage) {
    cx.set_global(CurrentLanguage(lang));
}

/// 读当前界面语言。给拿不到 `AppState` 的回调用。
///
/// 拿到的语言一般会先存进局部变量（`let lang = current(cx);`）再用多次——同一个函数里
/// 反复读全局既没必要，也容易让人误以为中途会变。
pub fn current(cx: &App) -> AppLanguage {
    cx.try_global::<CurrentLanguage>().copied().unwrap_or_default().0
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

    /// 源码扫描器：找出 `src/**/*.rs` 里白名单之外的硬编码中文。
    ///
    /// 这是 4.4 那一轮迁移的**防回归网**：迁移本身是一次性动作，但"以后新写的文案要记得
    /// 走 `tr`"是长期要求，靠人自觉迟早会漏，所以把"没有漏翻的中文"变成测试断言。
    ///
    /// 判断标准和 `i18n_skip.txt` 的注释一致：是界面文案就翻译；不是（品牌名 / 写进数据的值 /
    /// 写给模型的提示词）就往白名单里加，并写明理由。
    mod scan {
        use std::fs;
        use std::path::{Path, PathBuf};

        /// 界面文案表本身，里面通篇是中文，跳过。
        pub const SKIP_FILE: &str = "i18n.rs";

        pub fn is_cjk(c: char) -> bool {
            matches!(c as u32, 0x3400..=0x4dbf | 0x4e00..=0x9fff | 0xf900..=0xfaff)
        }

        pub fn has_cjk(s: &str) -> bool {
            s.chars().any(is_cjk)
        }

        /// 按 `#[cfg(test)]` 首次出现处切掉后面的测试区（约定测试写在文件末尾）。
        ///
        /// 测试里全是中文断言数据，那是在**测中文**，不是漏翻。
        pub fn strip_test_region(src: &str) -> &str {
            match src.find("#[cfg(test)]") {
                Some(idx) => {
                    let start = src[..idx].rfind('\n').map(|p| p + 1).unwrap_or(0);
                    &src[..start]
                }
                None => src,
            }
        }

        /// 读白名单。`\n` 还原成换行、`\\` 还原成反斜杠，其余原样。
        ///
        /// 转义约定必须跟 `i18n_skip.txt` 头部写的一致——Rust 源码里的字面量是**解码后**的
        /// 实际字符，白名单里却没法直接写换行，两边靠这套约定对齐。
        pub fn load_skip(root: &Path) -> Vec<String> {
            let path = root.join("i18n_skip.txt");
            let raw = fs::read_to_string(&path).unwrap_or_else(|e| panic!("读不到 {}: {e}", path.display()));
            let mut out = Vec::new();
            for line in raw.lines() {
                let line = line.trim();
                if line.is_empty() || line.starts_with('#') {
                    continue;
                }
                let mut buf = String::new();
                let mut chars = line.chars();
                while let Some(c) = chars.next() {
                    if c != '\\' {
                        buf.push(c);
                        continue;
                    }
                    match chars.next() {
                        Some('n') => buf.push('\n'),
                        Some(other) => buf.push(other),
                        None => buf.push('\\'),
                    }
                }
                out.push(buf);
            }
            out
        }

        /// 在字符数组里找子串，返回起始下标。
        fn find(hay: &[char], from: usize, needle: &[char]) -> Option<usize> {
            if needle.is_empty() || needle.len() > hay.len() {
                return None;
            }
            (from..=hay.len() - needle.len()).find(|&i| hay[i..i + needle.len()] == *needle)
        }

        /// 把 `\n` / `\t` / `\r` / `\"` / `\\` 还原成实际字符。
        ///
        /// 单个字符单个字符地走，而不是连着做几次 `replace`：后者在遇到 `\\n`
        /// （字面反斜杠 + n）时会先匹配到后两个字符，把它错当成换行。
        fn decode_escapes(raw: &str) -> String {
            let mut out = String::new();
            let mut chars = raw.chars();
            while let Some(c) = chars.next() {
                if c != '\\' {
                    out.push(c);
                    continue;
                }
                match chars.next() {
                    Some('n') => out.push('\n'),
                    Some('t') => out.push('\t'),
                    Some('r') => out.push('\r'),
                    Some('"') => out.push('"'),
                    Some('\\') => out.push('\\'),
                    Some(other) => {
                        out.push('\\');
                        out.push(other);
                    }
                    None => out.push('\\'),
                }
            }
            out
        }

        /// 逐字符状态机，挑出所有字符串字面量并**解码转义**。
        ///
        /// 不用正则的原因：正则分不清"注释里的中文""`r#"..."#` 原始字符串""字符字面量
        /// `'"'` 里的引号"，会把它们一并算成待翻译的文案。
        pub fn string_literals(src: &str) -> Vec<String> {
            let b: Vec<char> = src.chars().collect();
            let n = b.len();
            let mut out = Vec::new();
            let mut i = 0usize;
            while i < n {
                let c = b[i];
                // 行注释
                if c == '/' && i + 1 < n && b[i + 1] == '/' {
                    while i < n && b[i] != '\n' {
                        i += 1;
                    }
                    continue;
                }
                // 块注释（Rust 允许嵌套）
                if c == '/' && i + 1 < n && b[i + 1] == '*' {
                    let mut depth = 1usize;
                    i += 2;
                    while i < n && depth > 0 {
                        if b[i] == '/' && i + 1 < n && b[i + 1] == '*' {
                            depth += 1;
                            i += 2;
                            continue;
                        }
                        if b[i] == '*' && i + 1 < n && b[i + 1] == '/' {
                            depth -= 1;
                            i += 2;
                            continue;
                        }
                        i += 1;
                    }
                    continue;
                }
                // 字符字面量：跳过，免得里面的单引号（`'"'`）把状态搞乱
                if c == '\'' {
                    if i + 3 < n && b[i + 1] == '\\' && b[i + 3] == '\'' {
                        i += 4;
                        continue;
                    }
                    if i + 2 < n && b[i + 2] == '\'' {
                        i += 3;
                        continue;
                    }
                    i += 1;
                    continue;
                }
                // 原始字符串 r"..." / r#"..."# / br#"..."#
                if c == 'r' && i + 1 < n && (b[i + 1] == '#' || b[i + 1] == '"') {
                    let mut j = i + 1;
                    let mut hashes = 0usize;
                    while j < n && b[j] == '#' {
                        hashes += 1;
                        j += 1;
                    }
                    if j < n && b[j] == '"' {
                        j += 1;
                        let close: Vec<char> = std::iter::once('"').chain(std::iter::repeat_n('#', hashes)).collect();
                        let end = find(&b, j, &close).unwrap_or(n);
                        out.push(b[j..end].iter().collect());
                        i = end + close.len();
                        continue;
                    }
                }
                // 普通字符串
                if c == '"' {
                    let mut j = i + 1;
                    let mut raw = String::new();
                    while j < n {
                        if b[j] == '\\' && j + 1 < n {
                            raw.push(b[j]);
                            raw.push(b[j + 1]);
                            j += 2;
                            continue;
                        }
                        if b[j] == '"' {
                            break;
                        }
                        raw.push(b[j]);
                        j += 1;
                    }
                    out.push(decode_escapes(&raw));
                    i = j + 1;
                    continue;
                }
                i += 1;
            }
            out
        }

        /// 递归收集目录下的 `.rs` 文件。
        pub fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
            let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("读不到 {}: {e}", dir.display()));
            for entry in entries {
                let path = entry.expect("读目录项失败").path();
                if path.is_dir() {
                    collect_rs(&path, out);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    out.push(path);
                }
            }
        }
    }

    /// `src/` 下不该再出现白名单之外的硬编码中文。
    ///
    /// 白名单读的是仓库根目录的 `i18n_skip.txt`——和 Python 侧的 `scan_cjk.py` 同一份文件，
    /// 免得两边各维护一份、慢慢对不上。
    #[test]
    fn no_hardcoded_chinese_outside_whitelist() {
        use std::fs;
        use std::path::Path;

        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let skip = scan::load_skip(root);

        let mut files = Vec::new();
        scan::collect_rs(&root.join("src"), &mut files);
        assert!(!files.is_empty(), "一个 rs 文件都没扫到，路径不对？");

        let mut offenders = Vec::new();
        for file in &files {
            let name = file.file_name().unwrap_or_default().to_string_lossy();
            if name == scan::SKIP_FILE {
                continue;
            }
            let src = fs::read_to_string(file).expect("读源文件失败");
            for lit in scan::string_literals(scan::strip_test_region(&src)) {
                if !scan::has_cjk(&lit) || skip.contains(&lit) {
                    continue;
                }
                offenders.push(format!("{}: {lit:?}", file.display()));
            }
        }

        assert!(
            offenders.is_empty(),
            "发现 {} 处白名单之外的硬编码中文。界面文案请改用 `i18n::tr` / `tr_args`；\
             确实不该翻译的（品牌名 / 写进持久化数据的值 / 写给模型的提示词），\
             加进 `i18n_skip.txt` 并写明理由：\n{}",
            offenders.len(),
            offenders.join("\n")
        );
    }
}
