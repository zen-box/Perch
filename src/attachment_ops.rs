use std::path::{Path, PathBuf};

use gpui_kit::*;
use gpui_kit_assets::IconName;
use uuid::Uuid;

use crate::app::{AppState, ToastLevel, ViewMode, runtime, update_state};
use crate::clipboard::{self, PastePayload};
use crate::file_store;
use crate::i18n::{AppLanguage, Key, tr, tr_args};
use crate::model::{Attachment, AttachmentKind};
use crate::model_info::{self, Capability};

/// 文件对话框里能选到的扩展名，**按能力拆成三组**：模型看不懂图片就不摆图片过滤器，
/// 摆了用户选完也发不出去。
///
/// Word / Excel / PPT 不在里面：它们不会进请求体，入口直接拒收（见 [`refusal_by_type`]）。
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];
const PDF_EXTS: &[&str] = &["pdf"];
const TEXT_EXTS: &[&str] = &[
    "txt", "md", "json", "rs", "py", "js", "ts", "c", "cpp", "go", "java", "sql", "sh", "yaml", "yml", "toml", "xml",
    "csv",
];

/// 这批附件要求目标模型具备哪些能力。
///
/// 只有**真会被发出去**的两类算：图片要 `Vision`，PDF 要 `Files`。文本与代码类不需要任何
/// 能力——它们会被直接拼进消息正文（`llm_request::effective_message_text`），任何模型都读得了，
/// 拿它们去卡模型是错的。
///
/// ⚠️ 这里刻意**不包含** Word / Excel / PPT：它们压根不会被塞进请求体
/// （`llm_request.rs` 只挑 `Image || is_pdf()`），拿 `Files` 去卡等于用一个假理由拦人。
/// 新加的这类文件在入口就被拒收了（[`refusal_by_type`]），这里只会碰到旧消息里存下的。
pub(crate) fn required_capabilities(attachments: &[Attachment]) -> Vec<Capability> {
    let mut needed = Vec::new();
    for attachment in attachments {
        let capability = if attachment.is_image() {
            Capability::Vision
        } else if attachment.is_pdf() {
            Capability::Files
        } else {
            continue;
        };
        if !needed.contains(&capability) {
            needed.push(capability);
        }
    }
    needed.sort();
    needed
}

/// 附件菜单里的一项，同时也是文件对话框过滤器的一组。
///
/// 菜单按类型分项、而不是一个「添加附件」直接开对话框：模型接不住哪一类，菜单里那一项
/// 就灰掉并写明原因。合成一个过滤器的话，纯文本模型下照样能选到图片，**选完才被
/// [`refusal_by_type`] 拒掉**——白跑一趟，用户还不知道自己错在哪。
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum AttachmentFilter {
    /// 图片与截图，要模型有 `Vision`
    Images,
    /// PDF，要模型有 `Files`
    Documents,
    /// 文本与代码，拼进正文，任何模型都收
    TextCode,
    /// 兜底：不在上面三组里的扩展名（`.log`、`.ini`……）
    AllFiles,
}

/// 附件菜单里的顺序：先摆最常用的图片，再是 PDF、文本与代码，最后才是兜底的「所有文件」。
///
/// 顺序只写一份，改菜单就是改这里——不然界面上的顺序和别处（比如文档、测试）各说各话。
pub(crate) const ATTACHMENT_FILTERS: [AttachmentFilter; 4] = [
    AttachmentFilter::Images,
    AttachmentFilter::Documents,
    AttachmentFilter::TextCode,
    AttachmentFilter::AllFiles,
];

impl AttachmentFilter {
    /// 菜单项与文件对话框过滤器共用的名字。
    ///
    /// 两处共用一份，是为了菜单上写的和对话框里摆的不会哪天对不上。
    pub(crate) fn label(self, lang: AppLanguage) -> &'static str {
        tr(
            lang,
            match self {
                Self::Images => Key::FilterImages,
                Self::Documents => Key::FilterDocuments,
                Self::TextCode => Key::FilterTextCode,
                Self::AllFiles => Key::FilterAllFiles,
            },
        )
    }

    /// 这一类要求模型具备的能力；`None` 表示任何模型都收。
    ///
    /// 只有图片和 PDF 算——文本与代码会被拼进消息正文（`llm_request::effective_message_text`），
    /// 拿能力去卡它们是错的。「所有文件」是兜底项，灰掉它等于把没列出来的文本扩展名一起封死。
    pub(crate) fn requires(self) -> Option<Capability> {
        match self {
            Self::Images => Some(Capability::Vision),
            Self::Documents => Some(Capability::Files),
            Self::TextCode | Self::AllFiles => None,
        }
    }

    /// 菜单项前面的图标。四类各一个，扫一眼就知道哪一项是哪一类。
    pub(crate) fn icon(self) -> IconName {
        match self {
            Self::Images => IconName::Image,
            Self::Documents => IconName::FileText,
            Self::TextCode => IconName::FileCode,
            Self::AllFiles => IconName::FolderOpen,
        }
    }

    /// 模型接不住这一类时，菜单项上的文案：点名是哪个模型、缺哪一项。
    ///
    /// `None` = 这一项永远可用（文本与代码、所有文件）。
    pub(crate) fn unavailable_label(self, model: &str, lang: AppLanguage) -> Option<String> {
        let key = match self {
            Self::Images => Key::AttachImagesUnavailable,
            Self::Documents => Key::AttachPdfUnavailable,
            Self::TextCode | Self::AllFiles => return None,
        };
        Some(tr_args(lang, key, &[model]))
    }

    /// 开文件对话框时摆的扩展名。
    fn exts(self) -> Vec<&'static str> {
        match self {
            Self::Images => IMAGE_EXTS.to_vec(),
            Self::Documents => PDF_EXTS.to_vec(),
            Self::TextCode => TEXT_EXTS.to_vec(),
            // 没列出来的扩展名（`.log`、`.vue`……）从这儿选；是不是能收由内容决定
            Self::AllFiles => vec!["*"],
        }
    }
}

/// 某个模型接不住这批附件的原因。
pub(crate) struct AttachmentObstacle {
    /// 界面上的模型名——用户得认出是哪一个模型接不住
    pub model: String,
    /// 缺的那项能力
    pub capability: Capability,
}

/// 添加附件时的闸门：这次要用的模型里，第一个缺某项能力的是谁。
///
/// 在界面线程算好再交给后台导入线程——后台读不到 `AppState`。
#[derive(Clone, Debug, Default)]
pub(crate) struct AttachmentGate {
    /// 第一个看不懂图片的模型（界面名）；`None` 表示都看得懂
    no_vision: Option<String>,
    /// 第一个读不了 PDF 的模型
    no_files: Option<String>,
}

impl AttachmentGate {
    /// 缺这项能力的模型名；都具备时返回 `None`。附件只看图片和 PDF 这两项。
    pub(crate) fn missing(&self, capability: Capability) -> Option<&str> {
        match capability {
            Capability::Vision => self.no_vision.as_deref(),
            Capability::Files => self.no_files.as_deref(),
            _ => None,
        }
    }

    /// 缺这项能力时拒收：点名是哪个模型、缺哪一项
    fn refusal(&self, capability: Capability, lang: AppLanguage) -> Option<Refusal> {
        self.missing(capability).map(|model| {
            Refusal::with_hint(
                tr_args(lang, Key::AttachmentNeedsCapability, &[model, capability.label(lang)]),
                Key::AttachmentCapabilityHint,
            )
        })
    }
}

/// 一个文件没加进来的原因。
#[derive(Debug)]
struct Refusal {
    /// 为什么不收
    reason: String,
    /// 怎么办。和原因分开存，是为了同一条建议只说一次——一次选了五张截图，
    /// 不该把「换个模型试试……」念五遍
    hint: Option<Key>,
}

impl Refusal {
    fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
            hint: None,
        }
    }

    fn with_hint(reason: impl Into<String>, hint: Key) -> Self {
        Self {
            reason: reason.into(),
            hint: Some(hint),
        }
    }
}

/// 把一批拒收拼成一条提示：每个文件一行原因，建议统一放在最后、每条只说一次。
fn refusal_summary(refusals: Vec<Refusal>, lang: AppLanguage) -> String {
    let mut lines = Vec::new();
    let mut hints = Vec::new();
    for refusal in refusals {
        lines.push(refusal.reason);
        if let Some(hint) = refusal.hint
            && !hints.contains(&hint)
        {
            hints.push(hint);
        }
    }
    lines.extend(hints.into_iter().map(|hint| tr(lang, hint).to_string()));
    lines.join("\n")
}

impl AppState {
    /// 这次要用的模型里，有没有接不住当前待发附件的。
    ///
    /// 对比模式必须**每个模型各查一遍**：三个模型里可能只有两个能看图，只查当前会话
    /// 那个模型的话，另外两个会带着图片去撞服务商的报错。
    pub(crate) fn attachment_obstacle(&self) -> Option<AttachmentObstacle> {
        self.attachment_obstacle_for(&self.pending_attachments, &self.compare_targets())
    }

    /// [`Self::attachment_obstacle`] 的实体。附件和目标模型都由调用方给——
    /// 「重新生成」用的不是输入框里的待发附件，而是历史消息里已经存下的那些。
    pub(crate) fn attachment_obstacle_for(
        &self,
        attachments: &[Attachment],
        targets: &[(String, String)],
    ) -> Option<AttachmentObstacle> {
        let needed = required_capabilities(attachments);
        if needed.is_empty() {
            return None;
        }
        for (provider_id, model_id) in targets {
            let capabilities = self.model_capabilities(provider_id, model_id);
            let Some(missing) = needed.iter().find(|capability| !capabilities.contains(capability)) else {
                continue;
            };
            return Some(AttachmentObstacle {
                model: self.model_label(provider_id, model_id),
                capability: *missing,
            });
        }
        None
    }

    /// 某个模型最终生效的能力：用户设置优先，没设置就按模型 ID 识别。
    ///
    /// 附件闸门和会话级工具选择（`tool_ops.rs`）都走这里——两处都得知道「这个模型
    /// 到底行不行」，各写一份迟早会不一致。
    pub(crate) fn model_capabilities(&self, provider_id: &str, model_id: &str) -> Vec<Capability> {
        match self.resolve_model(provider_id, model_id) {
            Some((_, model)) => model.effective_capabilities(),
            // 配置里查不到（模型刚被删掉之类）时退回按名字猜：发送前的检查既不该
            // 因为查不到就放行，也不该直接拦死
            None => model_info::detect(model_id, "").capabilities,
        }
    }

    /// 添加附件时的闸门：这次要用的模型里谁看不懂图片、谁读不了 PDF。
    ///
    /// 对比模式下**有一个模型**接不住就算接不住：文件对话框不摆对应的过滤器，
    /// 粘贴、选文件时直接拒收——收下了也会被发送前的检查拦下，白加一趟。
    pub(crate) fn attachment_gate(&self) -> AttachmentGate {
        let mut gate = AttachmentGate::default();
        for (provider_id, model_id) in self.compare_targets() {
            let capabilities = self.model_capabilities(&provider_id, &model_id);
            if gate.no_vision.is_none() && !capabilities.contains(&Capability::Vision) {
                gate.no_vision = Some(self.model_label(&provider_id, &model_id));
            }
            if gate.no_files.is_none() && !capabilities.contains(&Capability::Files) {
                gate.no_files = Some(self.model_label(&provider_id, &model_id));
            }
        }
        gate
    }

    /// 模型在界面上的名字。查不到就退回 id。
    fn model_label(&self, provider_id: &str, model_id: &str) -> String {
        self.resolve_model(provider_id, model_id)
            .map(|(_, model)| model.name.clone())
            .unwrap_or_else(|| model_id.to_string())
    }

    /// 发送前的最后一道闸：这批附件要发给的目标模型接不接得住。拦下来了返回 `true`。
    ///
    /// 输入框上方那条提示只是**提前**提醒，用户完全可以不理它继续点发送；这里才是真拦。
    /// 不拦的话请求会带着图片/PDF 直接发给服务商，换回来一个看不懂的错误——
    /// 用户看到的是「模型报错了」，而不是「我传了个它看不懂的东西」。
    pub(crate) fn block_unsupported_attachments(
        &mut self,
        attachments: Vec<Attachment>,
        targets: &[(String, String)],
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(obstacle) = self.attachment_obstacle_for(&attachments, targets) else {
            return false;
        };
        let lang = self.language();
        self.toast(
            ToastLevel::Error,
            tr_args(
                lang,
                Key::AttachmentUnsupported,
                &[&obstacle.model, obstacle.capability.label(lang)],
            ),
        );
        cx.notify();
        true
    }

    /// 把本地文件加入待发送的附件。读取、计算哈希、复制都在后台线程做，大文件不会卡住界面。
    ///
    /// 选文件、粘贴文件都走这里，所以模型接不住的格式在这里统一拒收（见 [`import_file`]）。
    pub fn add_attachment_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        // 语言和闸门要在 spawn 之前取好：闭包里只有 `this` / `cx`，读不到 `AppState`
        let lang = self.language();
        let gate = self.attachment_gate();
        cx.spawn(async move |this, cx| {
            let results = runtime()
                .spawn_blocking(move || {
                    paths
                        .iter()
                        .map(|path| import_file(path, &gate, lang))
                        .collect::<Vec<_>>()
                })
                .await
                .unwrap_or_else(|error| {
                    vec![Err(Refusal::new(tr_args(
                        lang,
                        Key::AddAttachmentFailed,
                        &[&error.to_string()],
                    )))]
                });
            update_state(&this, cx, |state, cx| state.finish_attachment_import(results, cx));
        })
        .detach();
    }

    fn finish_attachment_import(&mut self, results: Vec<Result<Attachment, Refusal>>, cx: &mut Context<Self>) {
        let lang = self.language();
        let mut added = 0usize;
        let mut refusals = Vec::new();
        for result in results {
            match result {
                Ok(attachment) => {
                    self.pending_attachments.push(attachment);
                    added += 1;
                }
                Err(refusal) => refusals.push(refusal),
            }
        }
        if added > 0 {
            self.toast(
                ToastLevel::Success,
                tr_args(lang, Key::AttachmentsAdded, &[&added.to_string()]),
            );
        }
        if !refusals.is_empty() {
            self.toast(ToastLevel::Error, refusal_summary(refusals, lang));
        }
        cx.notify();
    }

    /// 打开文件对话框挑附件。菜单里点了哪一项，就只摆哪一组的过滤器。
    ///
    /// 摆一组用不上的没有意义——用户选完还是会被 [`refusal_by_type`] 拒掉。真要绕过去
    /// （比如从「所有文件」里点了一张图）也还有那一层兜着，会说明原因。
    pub fn pick_attachments(&mut self, filter: AttachmentFilter, cx: &mut Context<Self>) {
        let lang = self.language();
        // 过滤器要在起线程之前算好：`self` 进不了那个闭包
        let (label, exts) = (filter.label(lang), filter.exts());
        let (tx, rx) = tokio::sync::oneshot::channel();
        // 系统文件对话框会阻塞调用它的线程，放到单独的线程里
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title(tr(lang, Key::PickAttachmentTitle))
                .add_filter(label, &exts)
                .pick_files();
            // 用户关掉对话框时接收端可能已经不在了，发送失败无所谓
            let _ = tx.send(picked);
        });

        cx.spawn(async move |this, cx| {
            let Ok(Some(paths)) = rx.await else { return };
            update_state(&this, cx, |state, cx| state.add_attachment_paths(paths, cx));
        })
        .detach();
    }

    /// 输入框里的粘贴（Ctrl+V、右键「粘贴」）。复制的文件和图片变成附件，
    /// 返回 true 表示已经处理，输入框不要再插入文字。
    pub fn handle_clipboard_paste(&mut self, item: &ClipboardItem, cx: &mut Context<Self>) -> bool {
        self.apply_paste(clipboard::classify(item.entries(), false), cx)
    }

    /// 焦点不在任何输入框时按 Ctrl+V（比如截完图点了一下对话区）：
    /// 文件和图片变成附件，文字插入输入框。
    pub fn paste_into_chat(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.view_mode != ViewMode::Chat {
            return;
        }
        let Some(item) = cx.read_from_clipboard() else { return };
        match clipboard::classify(item.entries(), false) {
            PastePayload::Text(text) => self.chat_input.update(cx, |input, cx| {
                input.focus(window, cx);
                input.insert(&text, window, cx);
            }),
            payload => {
                self.apply_paste(payload, cx);
            }
        }
    }

    /// 返回 true 表示这次粘贴已经作为附件处理
    fn apply_paste(&mut self, payload: PastePayload, cx: &mut Context<Self>) -> bool {
        let lang = self.language();
        match payload {
            PastePayload::Files(paths) => {
                self.add_attachment_paths(paths, cx);
                true
            }
            PastePayload::Image(image) => {
                self.add_clipboard_image(image, cx);
                true
            }
            PastePayload::FoldersOnly => {
                self.toast(ToastLevel::Info, tr(lang, Key::CannotPasteFolder));
                cx.notify();
                true
            }
            PastePayload::Text(_) | PastePayload::Empty => false,
        }
    }

    /// 剪贴板图片在后台转换格式、保存，完成后加到附件里
    fn add_clipboard_image(&mut self, image: Image, cx: &mut Context<Self>) {
        let lang = self.language();
        // 截图是最容易误加的：模型看不懂图片就当场说清楚，别等到发送时才被拦
        if let Some(mut refusal) = self.attachment_gate().refusal(Capability::Vision, lang) {
            refusal.reason = tr_args(lang, Key::PasteImageFailed, &[&refusal.reason]);
            self.toast(ToastLevel::Error, refusal_summary(vec![refusal], lang));
            cx.notify();
            return;
        }
        cx.spawn(async move |this, cx| {
            let result = runtime()
                .spawn_blocking(move || import_clipboard_image(&image, lang))
                .await
                .unwrap_or_else(|error| Err(tr_args(lang, Key::PasteImageFailed, &[&error.to_string()])));
            update_state(&this, cx, |state, cx| {
                match result {
                    Ok(attachment) => {
                        state.pending_attachments.push(attachment);
                        state.toast(ToastLevel::Success, tr(state.language(), Key::ImagePasted));
                    }
                    Err(error) => state.toast(ToastLevel::Error, error),
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn remove_pending_attachment(&mut self, attachment_id: &str, cx: &mut Context<Self>) {
        if let Some(pos) = self.pending_attachments.iter().position(|a| a.id == attachment_id) {
            self.pending_attachments.remove(pos);
            cx.notify();
        }
    }

    /// 在文件管理器里定位附件。
    ///
    /// 打不开必须提示：用户点了「打开」却一点反应都没有，只会以为程序卡住了。
    /// 其它平台暂时没接，界面那边也只在 Windows 下挂这个回调。
    #[cfg(target_os = "windows")]
    pub fn reveal_attachment(&mut self, path: &Path, cx: &mut Context<Self>) {
        let lang = self.language();
        if let Err(error) = crate::paths::reveal(path) {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::OpenFileFailed, &[&error.to_string()]),
            );
            cx.notify();
        }
    }
}

/// 在后台线程把本地文件复制进附件目录。
///
/// 入口只收**真能送到模型手里**的东西：收下之后发不出去（服务商报错），或者发出去了
/// 模型却没收到（请求体里悄悄跳过），用户都只会以为是模型出了问题。
fn import_file(path: &Path, gate: &AttachmentGate, lang: AppLanguage) -> Result<Attachment, Refusal> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_string();
    // 原因前面点名是哪个文件：一次选好几个时，用户得知道是哪个没进来
    let not_added = |refusal: Refusal| Refusal {
        reason: tr_args(lang, Key::AttachmentNotAdded, &[&name, &refusal.reason]),
        hint: refusal.hint,
    };
    let (kind, mime) = file_store::detect_kind_and_mime(&name);
    // 光看类型就能下结论的先拦，不用把几十 MB 的 PPT 读进内存再告诉用户发不了
    if let Some(refusal) = refusal_by_type(kind, mime, gate, lang) {
        return Err(not_added(refusal));
    }
    let bytes = file_store::read_file(path, lang).map_err(|error| not_added(Refusal::new(error.to_string())))?;
    let content = settle_content(&name, kind, mime, bytes, lang).map_err(&not_added)?;
    let saved = file_store::save_bytes(&content.bytes, &content.name, content.mime)
        .map_err(|error| not_added(Refusal::new(error.to_string())))?;
    Ok(Attachment {
        id: Uuid::new_v4().to_string(),
        kind: content.kind,
        name: content.name,
        mime: content.mime.to_string(),
        path: saved.storage_path,
        size: saved.size_bytes,
        hash: saved.hash,
    })
}

/// 只看类型就能拒收的：模型缺对应能力的图片和 PDF，以及永远发不出去的 Office 文件。
fn refusal_by_type(kind: AttachmentKind, mime: &str, gate: &AttachmentGate, lang: AppLanguage) -> Option<Refusal> {
    match kind {
        AttachmentKind::Image => gate.refusal(Capability::Vision, lang),
        AttachmentKind::Document if mime == "application/pdf" => gate.refusal(Capability::Files, lang),
        // Word / Excel / PPT：`llm_request.rs` 只挑图片和 PDF 进请求体，这几种收下了
        // 也只是摆在界面上，模型一个字都看不到
        AttachmentKind::Document => Some(Refusal::with_hint(
            tr(lang, Key::AttachmentOfficeUnsupported),
            Key::AttachmentOfficeHint,
        )),
        AttachmentKind::Text | AttachmentKind::Other => None,
    }
}

/// 导入时最终存下的内容：可能换了格式（BMP 转 PNG），也可能换了身份（认出来的文本）。
struct SettledContent {
    kind: AttachmentKind,
    mime: &'static str,
    name: String,
    bytes: Vec<u8>,
}

/// 读到内容以后再定：
/// - 文本得真是 UTF-8——发送时只认这个，读不出来的会被悄悄跳过；
/// - 不认识的扩展名看内容：是文本（.log、.ini、.vue……）就当文本收下，否则拒收；
/// - BMP 转成 PNG：OpenAI、Claude、Gemini 的图片接口都不收 BMP。
fn settle_content(
    name: &str,
    kind: AttachmentKind,
    mime: &'static str,
    bytes: Vec<u8>,
    lang: AppLanguage,
) -> Result<SettledContent, Refusal> {
    let keep = |kind: AttachmentKind, mime: &'static str, bytes: Vec<u8>| {
        Ok(SettledContent {
            kind,
            mime,
            name: name.to_string(),
            bytes,
        })
    };
    match kind {
        AttachmentKind::Image if mime == "image/bmp" => {
            let png = bmp_to_png(&bytes)
                .map_err(|error| Refusal::new(tr_args(lang, Key::AttachmentImageUnreadable, &[&error.to_string()])))?;
            Ok(SettledContent {
                kind,
                mime: "image/png",
                // 名字跟着改：看到 .bmp 的人会以为发出去的还是 BMP
                name: Path::new(name).with_extension("png").to_string_lossy().into_owned(),
                bytes: png,
            })
        }
        AttachmentKind::Text if !file_store::is_plain_text(&bytes) => Err(Refusal::with_hint(
            tr(lang, Key::AttachmentNotUtf8),
            Key::AttachmentNotUtf8Hint,
        )),
        AttachmentKind::Other if !file_store::is_plain_text(&bytes) => {
            Err(Refusal::new(tr(lang, Key::AttachmentBinaryUnsupported)))
        }
        AttachmentKind::Other => keep(AttachmentKind::Text, "text/plain", bytes),
        AttachmentKind::Image | AttachmentKind::Document | AttachmentKind::Text => keep(kind, mime, bytes),
    }
}

/// BMP 解码后重新编码成 PNG
fn bmp_to_png(bmp: &[u8]) -> image::ImageResult<Vec<u8>> {
    let decoded = image::load_from_memory_with_format(bmp, image::ImageFormat::Bmp)?;
    let mut png = std::io::Cursor::new(Vec::new());
    decoded.write_to(&mut png, image::ImageFormat::Png)?;
    Ok(png.into_inner())
}

/// 在后台线程把剪贴板图片转成常见格式并保存
fn import_clipboard_image(image: &Image, lang: AppLanguage) -> Result<Attachment, String> {
    let prepared = clipboard::prepare_image(image, lang)?;
    let name = format!(
        "paste_{}.{}",
        chrono::Local::now().format("%Y%m%d_%H%M%S"),
        prepared.extension
    );
    let saved = file_store::save_bytes(&prepared.bytes, &name, prepared.mime)
        .map_err(|error| tr_args(lang, Key::SaveClipboardImageFailed, &[&error.to_string()]))?;
    Ok(Attachment {
        id: Uuid::new_v4().to_string(),
        kind: AttachmentKind::Image,
        name,
        mime: prepared.mime.to_string(),
        path: saved.storage_path,
        size: saved.size_bytes,
        hash: saved.hash,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn attachment(name: &str, kind: AttachmentKind, mime: &str) -> Attachment {
        Attachment {
            id: name.to_string(),
            kind,
            name: name.to_string(),
            mime: mime.to_string(),
            path: String::new(),
            size: 0,
            hash: String::new(),
        }
    }

    /// 只有图片和 PDF 需要模型能力。文本与代码类会被拼进消息正文，任何模型都读得了——
    /// 把它们也算进去的话，一个纯文本模型连个 `.rs` 都发不出去。
    ///
    /// Word/Excel/PPT 也不算：它们现在压根不会进请求体，拿 `Files` 卡等于用假理由拦人。
    #[std::prelude::v1::test]
    fn only_images_and_pdfs_ask_for_a_capability() {
        let text = attachment("main.rs", AttachmentKind::Text, "text/x-rust");
        let docx = attachment(
            "notes.docx",
            AttachmentKind::Document,
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        );
        assert!(required_capabilities(&[text, docx]).is_empty());

        let image = |name: &str| attachment(name, AttachmentKind::Image, "image/png");
        let pdf = |name: &str| attachment(name, AttachmentKind::Document, "application/pdf");
        assert_eq!(required_capabilities(&[image("shot.png")]), vec![Capability::Vision]);
        assert_eq!(required_capabilities(&[pdf("manual.pdf")]), vec![Capability::Files]);
        // 两样都要时各算一条，顺序固定（Vision 在 Files 前面），提示文案才稳定
        assert_eq!(
            required_capabilities(&[image("shot.png"), pdf("manual.pdf")]),
            vec![Capability::Vision, Capability::Files]
        );
    }

    /// 附件菜单按类型分项：模型缺哪项能力，菜单里那一项就灰掉并写明原因。
    ///
    /// 这不是锦上添花——菜单项可用与否**必须**和 [`refusal_by_type`] 的判断一致，否则又回到
    /// 「选完才被拒」的老路上：用户点开菜单、选了一张 .png、等导入跑完，才收到一条拒收提示。
    #[std::prelude::v1::test]
    fn the_attachment_menu_greys_out_what_the_model_cannot_take() {
        let lang = AppLanguage::ZhCn;
        let name = "DeepSeek V4 Flash";

        // 图片要 Vision、PDF 要 Files
        assert_eq!(AttachmentFilter::Images.requires(), Some(Capability::Vision));
        assert_eq!(AttachmentFilter::Documents.requires(), Some(Capability::Files));

        // 灰掉的那两项要把模型名点出来，用户才知道该换谁
        let reason = AttachmentFilter::Images
            .unavailable_label(name, lang)
            .expect("图片项要写明原因");
        assert!(reason.contains(name), "灰掉的原因里要点名模型：{reason}");
        assert!(AttachmentFilter::Documents.unavailable_label(name, lang).is_some());

        // 文本与代码、所有文件**永远可用**：前者会被拼进消息正文，任何模型都读得了；
        // 后者是兜底，灰掉它等于把没列出来的文本扩展名（.log、.vue……）一起封死
        for filter in [AttachmentFilter::TextCode, AttachmentFilter::AllFiles] {
            assert_eq!(filter.requires(), None, "{filter:?} 不该拿能力卡人");
            assert!(filter.unavailable_label(name, lang).is_none());
        }
    }

    /// 每一组过滤器只摆自己那类的扩展名，互不串门；Office 扩展名哪一组都不摆
    /// （它们不会进请求体，入口就拒收了，摆出来只会让人白选）。
    #[std::prelude::v1::test]
    fn each_attachment_filter_offers_its_own_extensions() {
        let images = AttachmentFilter::Images.exts();
        assert!(images.contains(&"png") && images.contains(&"jpg") && !images.contains(&"pdf"));
        assert_eq!(AttachmentFilter::Documents.exts(), vec!["pdf"]);
        let text = AttachmentFilter::TextCode.exts();
        assert!(text.contains(&"rs") && text.contains(&"md") && !text.contains(&"png"));
        // 兜底那一项用通配：没列出来的扩展名从这儿选，收不收由内容判断
        assert_eq!(AttachmentFilter::AllFiles.exts(), vec!["*"]);

        for filter in [
            AttachmentFilter::Images,
            AttachmentFilter::Documents,
            AttachmentFilter::TextCode,
            AttachmentFilter::AllFiles,
        ] {
            for ext in ["docx", "xlsx", "pptx"] {
                assert!(!filter.exts().contains(&ext), "{ext} 不该出现在 {filter:?} 里");
            }
        }
    }

    fn gate(no_vision: Option<&str>, no_files: Option<&str>) -> AttachmentGate {
        AttachmentGate {
            no_vision: no_vision.map(str::to_string),
            no_files: no_files.map(str::to_string),
        }
    }

    /// 模型接不住的图片、PDF 在入口就拒收，并点名是哪个模型、缺哪项能力；
    /// Office 文件不管什么模型都拒收（它们不会进请求体）；文本与代码不看能力。
    #[std::prelude::v1::test]
    fn the_entry_refuses_what_the_models_cannot_take() {
        let lang = AppLanguage::ZhCn;
        let refuse = |name: &str, gate: &AttachmentGate| {
            let (kind, mime) = file_store::detect_kind_and_mime(name);
            refusal_by_type(kind, mime, gate, lang)
        };

        let text_only = gate(Some("DeepSeek V4 Flash"), Some("DeepSeek V4 Flash"));
        let refusal = refuse("shot.png", &text_only).expect("看不懂图片的模型不收图片");
        assert!(
            refusal.reason.contains("DeepSeek V4 Flash") && refusal.reason.contains(Capability::Vision.label(lang))
        );
        assert_eq!(refusal.hint, Some(Key::AttachmentCapabilityHint));
        let refusal = refuse("manual.pdf", &text_only).expect("读不了 PDF 的模型不收 PDF");
        assert!(refusal.reason.contains(Capability::Files.label(lang)));
        for text in ["main.rs", "notes.md", "server.log", "logo.svg"] {
            assert!(refuse(text, &text_only).is_none(), "{text} 拼进正文，任何模型都读得了");
        }

        // 对比模式里只有一个模型看不懂图片：图片照样拒收，PDF 放行
        let one_blind = gate(Some("mock-text"), None);
        assert!(refuse("shot.png", &one_blind).is_some());
        assert!(refuse("manual.pdf", &one_blind).is_none());

        let full = AttachmentGate::default();
        assert!(refuse("shot.png", &full).is_none() && refuse("manual.pdf", &full).is_none());
        for office in ["notes.docx", "sheet.xlsx", "slides.pptx", "old.doc"] {
            assert!(refuse(office, &full).is_some(), "{office} 进不了请求体，不能收");
        }
    }

    /// 读到内容以后：不认识的扩展名看内容，文本收下、二进制拒收；
    /// 认识的文本扩展名也得真是 UTF-8，否则发送时会被悄悄跳过。
    #[std::prelude::v1::test]
    fn unknown_extensions_are_judged_by_content() {
        let settle = |name: &str, bytes: &[u8]| {
            let (kind, mime) = file_store::detect_kind_and_mime(name);
            settle_content(name, kind, mime, bytes.to_vec(), AppLanguage::ZhCn)
        };

        let log = settle("server.log", b"INFO started\n").expect("文本日志要收下");
        assert_eq!(log.kind, AttachmentKind::Text);
        assert!(settle("archive.7z", b"7z\xBC\xAF\x27\x1C\x00\x04").is_err());
        // GBK 编码的 CSV：中文 Windows 上 Excel 默认就这么导出
        assert!(settle("data.csv", &[0xD6, 0xD0, 0xCE, 0xC4, b',', b'1']).is_err());
        assert!(settle("data.csv", "中文,1".as_bytes()).is_ok());
    }

    /// 一次拒收好几个文件时：每个文件一行原因，同一条建议只在最后说一次。
    /// 实测踩过：五个文件各带一遍「换个模型试试……」，提示框占了半个窗口。
    #[std::prelude::v1::test]
    fn the_same_hint_is_given_once() {
        let lang = AppLanguage::ZhCn;
        let blind = gate(Some("Mock Model"), None);
        let refusals = vec![
            blind.refusal(Capability::Vision, lang).unwrap(),
            blind.refusal(Capability::Vision, lang).unwrap(),
            Refusal::with_hint("docx", Key::AttachmentOfficeHint),
            Refusal::new("7z"),
        ];
        let summary = refusal_summary(refusals, lang);
        let capability_hint = tr(lang, Key::AttachmentCapabilityHint);
        assert_eq!(summary.matches(capability_hint).count(), 1);
        assert_eq!(summary.lines().count(), 6, "四行原因 + 两条建议：{summary}");
        assert!(summary.ends_with(tr(lang, Key::AttachmentOfficeHint)));
    }

    /// 服务商都不收 BMP：从磁盘选的 BMP 转成 PNG 再收，名字也跟着改。
    #[std::prelude::v1::test]
    fn bmp_files_are_converted_to_png() {
        let mut bmp = std::io::Cursor::new(Vec::new());
        image::RgbImage::from_pixel(4, 3, image::Rgb([40, 160, 90]))
            .write_to(&mut bmp, image::ImageFormat::Bmp)
            .unwrap();
        let (kind, mime) = file_store::detect_kind_and_mime("scan.bmp");
        let content = settle_content("scan.bmp", kind, mime, bmp.into_inner(), AppLanguage::ZhCn).unwrap();
        assert_eq!(content.kind, AttachmentKind::Image);
        assert_eq!(content.mime, "image/png");
        assert_eq!(content.name, "scan.png");
        assert!(content.bytes.starts_with(b"\x89PNG"));
    }
}
