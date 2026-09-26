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
    VisionNotSupportedHint => { "提示：当前选中的模型未标注视觉能力，建议切换为支持视觉的多模态模型", "Note: the selected model is not marked as vision-capable. Consider switching to a multimodal model.", "注意：選択中のモデルは画像対応が未設定です。マルチモーダルモデルへの切り替えをおすすめします。", "提示：目前選取的模型未標註視覺能力，建議切換為支援視覺的多模態模型" },
    AddAttachment => { "添加附件 (图片/文档/表格/代码)", "Add attachment (image / document / spreadsheet / code)", "添付を追加（画像・文書・表計算・コード）", "新增附件 (圖片/文件/表格/程式碼)" },
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
    CapabilitiesHint => { "目前只用于标注和筛选；图片、文档附件和联网搜索接入后会按这里判断模型能否使用", "Currently used for labelling and filtering only; once images, document attachments and web search land, this decides whether the model can be used", "現在はラベルと絞り込みのみに使用されます。画像・文書添付・ウェブ検索が入ると、ここでモデルが使えるか判断します", "目前只用於標註和篩選；圖片、文件附件和聯網搜尋接入後會按這裡判斷模型能否使用" },
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
/// 这类地方用 [`current`] / [`t`] 读全局，其余地方一律显式传 `lang`。
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
pub fn current(cx: &App) -> AppLanguage {
    cx.try_global::<CurrentLanguage>().copied().unwrap_or_default().0
}

/// 读当前界面语言并查一条文案，等价于 `tr(current(cx), key)`。
///
/// 目前只有 [`current`] + [`tr`] 的调用点；这个便捷封装是给接下来的应用层迁移
/// （错误提示、toast 之类只拿到 `&App` 的地方）准备的。
#[allow(dead_code)]
pub fn t(cx: &App, key: Key) -> &'static str {
    tr(current(cx), key)
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
