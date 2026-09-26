"""i18n 迁移的文案表（按批次追加）。

ENTRIES: (Key 名, 简体中文, English, 日本語, 繁體中文)
  - Key 名一旦定下不要再改（会牵动调用点）
  - 简体中文必须与源码里的字面量**逐字一致**，apply_i18n.py 靠它做匹配
  - 含换行的文案直接写真实换行，脚本会转义成 `\\n`

SKIP: 明确不翻译的中文字面量白名单（品牌名、关键词匹配表、发给模型的协议值等）
"""

ENTRIES = [
    # ---- ui/empty_state.rs ----
    ("EmptyNoProviderHint", "还没有配置模型渠道，先添加一个吧", "No provider configured yet. Add one to get started.", "プロバイダーが未設定です。まず追加してください。", "還沒有設定模型渠道，先新增一個吧"),
    ("EmptyGreeting", "今天想聊点什么？", "What would you like to talk about?", "今日は何を話しましょうか？", "今天想聊點什麼？"),
    ("PresetTranslate", "翻译", "Translate", "翻訳", "翻譯"),
    ("PresetTranslateDesc", "中英互译，保留原文格式", "Translate between Chinese and English, keeping the original formatting", "中国語と英語の相互翻訳（元の書式は保持）", "中英互譯，保留原文格式"),
    ("PresetTranslatePrompt", "请把下面的内容翻译成英文（如果原文是英文则翻译成中文），保留原有格式：\n", "Translate the following into English (or into Chinese if it is already English), keeping the original formatting:\n", "以下の内容を英語に翻訳してください（原文が英語の場合は中国語に）。書式はそのまま保持してください：\n", "請把下面的內容翻譯成英文（如果原文是英文則翻譯成中文），保留原有格式：\n"),
    ("PresetPolish", "润色文字", "Polish", "文章の推敲", "潤飾文字"),
    ("PresetPolishDesc", "让表达更通顺、更专业", "Make the wording smoother and more professional", "より読みやすく、プロらしい表現に", "讓表達更通順、更專業"),
    ("PresetPolishPrompt", "请帮我润色下面这段文字，使表达更通顺专业，并说明主要改动：\n", "Please polish the text below to make it smoother and more professional, and explain the main changes:\n", "以下の文章を読みやすくプロらしい表現に推敲し、主な変更点も説明してください：\n", "請幫我潤飾下面這段文字，讓表達更通順專業，並說明主要改動：\n"),
    ("PresetSummary", "总结要点", "Summarize", "要点をまとめる", "總結要點"),
    ("PresetSummaryDesc", "提炼长文的核心内容", "Extract the key points from a long text", "長文の核心を抽出", "提煉長文的核心內容"),
    ("PresetSummaryPrompt", "请用要点的形式总结下面的内容：\n", "Please summarize the following content as bullet points:\n", "以下の内容を箇条書きで要約してください：\n", "請用要點的形式總結下面的內容：\n"),
    ("PresetExplainCode", "解释一段代码", "Explain code", "コードを解説", "解釋一段程式碼"),
    ("PresetExplainCodeDesc", "粘贴代码，让 AI 逐段讲解", "Paste code and let the AI walk through it", "コードを貼り付けて、AI に順に解説させます", "貼上程式碼，讓 AI 逐段講解"),
    ("PresetExplainCodePrompt", "请逐段解释下面这段代码：\n", "Please explain the following code section by section:\n", "以下のコードを順を追って解説してください：\n", "請逐段解釋下面這段程式碼：\n"),
    ("AddModelChannel", "添加模型渠道", "Add a provider", "プロバイダーを追加", "新增模型渠道"),

    # ---- ui/composer.rs ----
    ("Remove", "移除", "Remove", "削除", "移除"),
    ("VisionNotSupportedHint", "提示：当前选中的模型未标注视觉能力，建议切换为支持视觉的多模态模型", "Note: the selected model is not marked as vision-capable. Consider switching to a multimodal model.", "注意：選択中のモデルは画像対応が未設定です。マルチモーダルモデルへの切り替えをおすすめします。", "提示：目前選取的模型未標註視覺能力，建議切換為支援視覺的多模態模型"),
    ("AddAttachment", "添加附件 (图片/文档/表格/代码)", "Add attachment (image / document / spreadsheet / code)", "添付を追加（画像・文書・表計算・コード）", "新增附件 (圖片/文件/表格/程式碼)"),
    ("StopGenerating", "停止生成", "Stop generating", "生成を停止", "停止生成"),
    ("SendEnter", "发送 (Enter)", "Send (Enter)", "送信（Enter）", "發送 (Enter)"),
    ("SendCompareEnter", "对比发送 (Enter)", "Send for comparison (Enter)", "比較送信（Enter）", "對比發送 (Enter)"),
    ("ClearQuote", "取消引用", "Clear quote", "引用を解除", "取消引用"),
    ("PromptTemplateHint", "提示词模板 · 回车或点击插入", "Prompt templates · press Enter or click to insert", "プロンプトテンプレート・Enter またはクリックで挿入", "提示詞模板 · Enter 或點擊插入"),

    # ---- ui/message_user.rs ----
    ("Copy", "复制", "Copy", "コピー", "複製"),
    ("EditAndResend", "编辑并重发", "Edit and resend", "編集して再送信", "編輯並重送"),
    ("Quote", "引用", "Quote", "引用", "引用"),

    # ---- ui/message_variants.rs ----
    ("Adopt", "采用", "Use this", "採用", "採用"),
    ("Thinking", "正在思考…", "Thinking…", "考えています…", "正在思考…"),
    ("ThinkingProcess", "思考过程", "Reasoning", "思考プロセス", "思考過程"),
    ("Generating", "正在生成…", "Generating…", "生成中…", "正在生成…"),
    ("ComparePickHint", "模型对比输出中，采用一条后继续对话：", "Comparing model outputs. Pick one to continue the conversation:", "モデル比較の出力中です。1 つ採用すると会話を続けられます：", "模型對比輸出中，採用一條後繼續對話："),

    # ---- ui/model_picker.rs ----
    ("NoModelConfigured", "未配置模型", "No model configured", "モデル未設定", "未設定模型"),
    ("NoAvailableModel", "还没有可用的模型", "No models available yet", "利用できるモデルがありません", "還沒有可用的模型"),
    ("NoMatchingModel", "没有匹配的模型", "No matching model", "一致するモデルがありません", "沒有符合的模型"),
    ("ManageModelChannel", "管理模型渠道", "Manage providers", "プロバイダーを管理", "管理模型渠道"),

    # ---- ui/settings.rs ----
    ("PromptTemplates", "提示词", "Prompts", "プロンプト", "提示詞"),
]

SKIP = set()

# 写进 src/i18n.rs 的分节注释，一眼看出这批 key 覆盖了哪些界面
BATCH_TITLE = "4.3-b：待补"
