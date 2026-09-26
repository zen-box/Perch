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

    # ---- ui/params.rs ----
    ("DefaultValue", "默认", "Default", "デフォルト", "預設"),

    # ---- ui/sidebar.rs ----
    ("Pin", "置顶", "Pinned", "ピン留め", "置頂"),
    ("Unpin", "取消置顶", "Unpin", "ピン留めを解除", "取消置頂"),
    ("Favorite", "收藏", "Favorite", "お気に入り", "收藏"),
    ("Unfavorite", "取消收藏", "Remove from favorites", "お気に入りを解除", "取消收藏"),
    ("SidebarNoMatch", "没有匹配的对话", "No matching chats", "一致するチャットがありません", "沒有符合的對話"),
    ("All", "全部", "All", "すべて", "全部"),
    ("Rename", "重命名", "Rename", "名前を変更", "重新命名"),
    ("RemoveFromFolder", "移出文件夹", "Remove from folder", "フォルダから外す", "移出資料夾"),
    ("NewFolder", "新建文件夹…", "New folder…", "新しいフォルダ…", "新增資料夾…"),
    ("MoveToFolder", "移到「{}」", "Move to \"{}\"", "「{}」へ移動", "移到「{}」"),
    ("DateToday", "今天", "Today", "今日", "今天"),
    ("DateYesterday", "昨天", "Yesterday", "昨日", "昨天"),
    ("DateLast7Days", "近 7 天", "Last 7 days", "過去 7 日", "近 7 天"),
    ("DateLast30Days", "近 30 天", "Last 30 days", "過去 30 日", "近 30 天"),
    ("DateEarlier", "更早", "Earlier", "それ以前", "更早"),

    # ---- ui/markdown_image.rs ----
    ("Image", "图片", "Image", "画像", "圖片"),
    ("ImageBadBase64", "Base64 图片数据无效或格式不支持", "The Base64 image data is invalid or in an unsupported format", "Base64 画像データが無効か、対応していない形式です", "Base64 圖片資料無效或格式不支援"),
    ("ImageUnsupportedUrl", "不支持的图片地址", "Unsupported image URL", "対応していない画像 URL です", "不支援的圖片網址"),
    ("ImageLoading", "图片加载中…", "Loading image…", "画像を読み込み中…", "圖片載入中…"),
    ("ImageLoadFailed", "图片加载失败：{}", "Failed to load image: {}", "画像の読み込みに失敗しました：{}", "圖片載入失敗：{}"),
    ("ImageHttpStatus", "服务器返回 HTTP {}", "The server returned HTTP {}", "サーバーが HTTP {} を返しました", "伺服器回傳 HTTP {}"),
    ("Retry", "重试", "Retry", "再試行", "重試"),
    ("OpenInBrowser", "在浏览器中打开", "Open in browser", "ブラウザで開く", "在瀏覽器中開啟"),
    ("ImageBadFormat", "不是能识别的图片格式", "Not a recognizable image format", "判別できない画像形式です", "不是能辨識的圖片格式"),
    ("ImageRemote", "远程图片", "Remote image", "リモート画像", "遠端圖片"),
    ("CopyLink", "复制链接", "Copy link", "リンクをコピー", "複製連結"),
    ("ImageLinkCopied", "图片链接已复制", "Image link copied", "画像リンクをコピーしました", "圖片連結已複製"),
    ("Close", "关闭", "Close", "閉じる", "關閉"),
    ("ImageInlineBase64", "Base64 内联图片", "Inline Base64 image", "インライン Base64 画像", "Base64 內嵌圖片"),
    ("CopyBase64", "复制 Base64", "Copy Base64", "Base64 をコピー", "複製 Base64"),
    ("Base64Copied", "Base64 数据已复制", "Base64 data copied", "Base64 データをコピーしました", "Base64 資料已複製"),

    # ---- 4.3-c：参数面板 / 会话视图 / 会话操作 ----
    # 参数面板
    ("Params", "参数", "Parameters", "パラメータ", "參數"),
    ("ChatParams", "对话参数", "Chat parameters", "会話パラメータ", "對話參數"),
    ("ParamsDefaultHint", "留空或选择“默认”时使用全局设置", "Leave empty or choose “Default” to use the global setting", "空欄または「デフォルト」を選ぶと全体設定が使われます", "留空或選擇「預設」時使用全域設定"),
    ("TemperatureLabel", "温度", "Temperature", "温度", "溫度"),
    ("MaxTokens", "最大 tokens", "Max tokens", "最大 tokens", "最大 tokens"),
    ("ContextMessages", "上下文条数", "Context messages", "コンテキスト件数", "上下文則數"),
    ("DefaultWithArg", "默认（{}）", "Default ({})", "デフォルト（{}）", "預設（{}）"),
    ("ReasoningEffort", "思考强度", "Reasoning effort", "思考の強度", "思考強度"),
    ("StreamingOutput", "流式输出", "Streaming", "ストリーミング出力", "串流輸出"),
    ("RestoreDefaults", "恢复默认", "Restore defaults", "デフォルトに戻す", "恢復預設"),
    ("Compare", "对比", "Compare", "比較", "對比"),
    ("CompareCount", "对比 {}", "Compare {}", "比較 {}", "對比 {}"),
    ("ModelCompare", "模型对比", "Model comparison", "モデル比較", "模型對比"),
    ("Clear", "清空", "Clear", "クリア", "清除"),
    ("CurrentModelBaseline", "当前模型（基准）", "Current model (baseline)", "現在のモデル（基準）", "目前模型（基準）"),
    ("Current", "当前", "Current", "現在", "目前"),
    ("ComparePickModels", "选择 1 到 2 个模型与当前模型对比：", "Pick 1–2 models to compare against the current one:", "現在のモデルと比較するモデルを 1〜2 個選んでください：", "選擇 1 到 2 個模型與目前模型對比："),
    ("NoOtherEnabledModel", "没有其他已启用的模型", "No other enabled models", "他に有効なモデルがありません", "沒有其他已啟用的模型"),
    ("StartCompareWithCount", "开始对比 ({} 个模型)", "Start comparison ({} models)", "比較を開始（{} モデル）", "開始對比（{} 個模型）"),
    ("PleasePickCompareModel", "请选择对比模型", "Select models to compare", "比較するモデルを選んでください", "請選擇對比模型"),
    # 会话视图
    ("JumpToLatest", "回到最新", "Jump to latest", "最新へ戻る", "回到最新"),
    ("MessageCount", "{} 条消息", "{} messages", "{} 件のメッセージ", "{} 則訊息"),
    ("NewChatShortcut", "新建对话 (Ctrl+N)", "New chat (Ctrl+N)", "新規チャット (Ctrl+N)", "新增對話 (Ctrl+N)"),
    ("ExportMarkdown", "导出为 Markdown", "Export as Markdown", "Markdown として書き出す", "匯出為 Markdown"),
    ("ExportJson", "导出 JSON 备份", "Export JSON backup", "JSON バックアップを書き出す", "匯出 JSON 備份"),
    ("ImportJson", "从 JSON 备份恢复", "Restore from JSON backup", "JSON バックアップから復元", "從 JSON 備份還原"),
    ("ClearCurrentChat", "清空当前对话", "Clear current chat", "現在の会話をクリア", "清空目前對話"),
    ("ImportConfirmHint", "已读取 JSON 备份。恢复会覆盖本机会话、提示词和渠道配置，不会写入 API Key。", "The JSON backup has been read. Restoring overwrites local chats, prompts and provider settings; API keys are not included.", "JSON バックアップを読み込みました。復元すると本機のチャット・プロンプト・プロバイダー設定が上書きされます（API キーは含まれません）。", "已讀取 JSON 備份。還原會覆蓋本機會話、提示詞和渠道設定，不會寫入 API Key。"),
    ("Restore", "恢复", "Restore", "復元", "還原"),
    ("AdoptCompareFirst", "请先采用一条对比回答，再继续对话", "Adopt one of the comparison replies before continuing", "先に比較回答を 1 つ採用してから続けてください", "請先採用一則對比回答，再繼續對話"),
    ("FileTypeText", "文本/代码", "Text / code", "テキスト・コード", "文字／程式碼"),
    ("FileTypeSheet", "表格", "Spreadsheet", "表計算", "試算表"),
    ("FileTypeFile", "文件", "File", "ファイル", "檔案"),
    ("ToolAuthRequired", "需要你的授权", "Permission required", "許可が必要です", "需要你的授權"),
    ("ToolAuthHint", "即将在本机执行下面的命令，请确认内容安全：", "The command below is about to run on this machine. Make sure it is safe:", "次のコマンドを本機で実行します。内容が安全か確認してください：", "即將在本機執行下面的指令，請確認內容安全："),
    ("Deny", "拒绝", "Deny", "拒否", "拒絕"),
    ("AllowOnce", "允许执行一次", "Allow once", "一度だけ許可", "允許執行一次"),
    # 会话操作（toast 与继续生成指令）
    ("CompareNeedInput", "请在输入框输入问题或添加图片后再开始对比", "Type a question or add an image before starting a comparison", "比較を始める前に、質問を入力するか画像を追加してください", "請在輸入框輸入問題或新增圖片後再開始對比"),
    ("CompareNeedModels", "请至少勾选 1 个要对比的模型", "Select at least 1 model to compare", "比較するモデルを 1 つ以上選んでください", "請至少勾選 1 個要對比的模型"),
    ("CompareMaxTwo", "最多选择 2 个对比模型（共 3 个模型 PK）", "At most 2 comparison models (3 models in total)", "比較モデルは最大 2 つ（合計 3 モデル）", "最多選擇 2 個對比模型（共 3 個模型 PK）"),
    ("NoUserMessageToRegenerate", "没有可重答的用户消息", "No user message to answer again", "再回答できるユーザーメッセージがありません", "沒有可重答的使用者訊息"),
    ("CanOnlyContinueAfterDone", "只能在已完成的回答后继续生成", "You can only continue after a finished reply", "完了した回答の後でのみ続きを生成できます", "只能在已完成的回答後繼續生成"),
    ("ContinuePrompt", "请从上次中断的地方继续，不要重复已有内容。", "Continue from where you left off, and do not repeat what you already wrote.", "前回中断したところから続けてください。既に書いた内容は繰り返さないでください。", "請從上次中斷的地方繼續，不要重複已有內容。"),
    ("CurrentModelUnavailable", "当前模型不可用", "The current model is unavailable", "現在のモデルは利用できません", "目前模型無法使用"),
    ("CannotDeleteWhileGenerating", "生成过程中不能删除消息", "Cannot delete a message while generating", "生成中はメッセージを削除できません", "生成過程中不能刪除訊息"),
    ("SessionDeleted", "对话已删除", "Chat deleted", "チャットを削除しました", "對話已刪除"),
    ("ExportedTo", "已导出至 {}", "Exported to {}", "{} に書き出しました", "已匯出至 {}"),
    ("ExportFailed", "导出失败: {}", "Export failed: {}", "書き出しに失敗しました: {}", "匯出失敗: {}"),
    ("ChatCleared", "当前对话已清空", "Current chat cleared", "現在の会話をクリアしました", "目前對話已清空"),
]

# 存进数据的值（新建会话的默认标题、默认文件夹名）不是界面文案：
# 它们是**持久化数据**，换界面语言不该改动已有数据，也不该改动新数据。
# 统一收成常量，比散落的字面量好维护，也避免被误当成漏翻的文案。
CONST_MAP = {
    "新对话": "DEFAULT_SESSION_TITLE",
    "默认": "DEFAULT_SESSION_FOLDER",
}

# 只有这些文件里的「默认 / 新对话」是数据；ui/params.rs 里的「默认」是界面文案，
# 走 tr(lang, Key::DefaultValue)，所以不在这里。
CONST_FILES = {
    "src/model.rs",
    "src/session_folder_ops.rs",
    "src/session_list_ops.rs",
    "src/session_ops.rs",
    "src/params_ops.rs",
}

SKIP = set()

# 同一个中文在不同界面是两个意思时，在这里指名该文件该用哪个 key。
# 典型：「关闭」既是推理档位的 Off，也是弹窗的 Close。
# 另外，源码里带命名参数的格式串（`{folder_name}`）和表里的 `{}` 对不上，
# 也要在这里显式指路。
FILE_KEY_OVERRIDE = {
    ("src/ui/markdown_image.rs", "关闭"): "Close",
    ("src/ui/sidebar.rs", "移到「{folder_name}」"): "MoveToFolder",
    ("src/ui/markdown_image.rs", "服务器返回 HTTP {status}"): "ImageHttpStatus",
    # 源码里是命名参数（`{compare_count}` / `{total_count}`），与表里的 `{}` 对不上
    ("src/ui/params.rs", "对比 {compare_count}"): "CompareCount",
    ("src/ui/params.rs", "开始对比 ({total_count} 个模型)"): "StartCompareWithCount",
}

# 写进 src/i18n.rs 的分节注释，一眼看出这批 key 覆盖了哪些界面
BATCH_TITLE = "4.3-c：参数面板 / 会话视图 / 会话操作"
