use std::path::{Path, PathBuf};

use gpui_kit::*;
use uuid::Uuid;

use crate::app::{AppState, ToastLevel, ViewMode, runtime, update_state};
use crate::clipboard::{self, PastePayload};
use crate::file_store;
use crate::i18n::{AppLanguage, Key, tr, tr_args};
use crate::model::{Attachment, AttachmentKind};
use crate::model_info::{self, Capability};

/// 文件对话框里能选到的扩展名，**按能力拆成三组**：模型看不懂图片就不摆图片过滤器，
/// 摆了用户选完也发不出去。
const IMAGE_EXTS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif", "bmp"];
const DOCUMENT_EXTS: &[&str] = &["pdf", "docx", "xlsx", "pptx"];
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
/// ⚠️ 这里刻意**不包含** Word / Excel / PPT：它们目前压根不会被塞进请求体
/// （`llm_request.rs` 只挑 `Image || is_pdf()`），拿 `Files` 去卡等于用一个假理由拦人。
/// 这个洞另外记着。
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

/// 「常用文件」过滤器里的扩展名：模型接得住的几类拼起来。
fn common_exts(vision: bool, files: bool) -> Vec<&'static str> {
    let mut exts = Vec::new();
    if vision {
        exts.extend_from_slice(IMAGE_EXTS);
    }
    if files {
        exts.extend_from_slice(DOCUMENT_EXTS);
    }
    exts.extend_from_slice(TEXT_EXTS);
    exts
}

/// 某个模型接不住这批附件的原因。
pub(crate) struct AttachmentObstacle {
    /// 界面上的模型名——用户得认出是哪一个模型接不住
    pub model: String,
    /// 缺的那项能力
    pub capability: Capability,
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

    /// 这次要用的模型**都**具备的能力（交集）。
    ///
    /// 文件对话框按它决定摆哪些过滤器：对比模式下有一个模型看不懂图片，就别把
    /// 「图片文件」摆出来——用户选完还是会被发送前的检查拦下，白选一趟。
    pub(crate) fn common_capabilities(&self) -> Vec<Capability> {
        let mut common: Option<Vec<Capability>> = None;
        for (provider_id, model_id) in self.compare_targets() {
            let capabilities = self.model_capabilities(&provider_id, &model_id);
            common = Some(match common {
                None => capabilities,
                Some(prev) => prev
                    .into_iter()
                    .filter(|capability| capabilities.contains(capability))
                    .collect(),
            });
        }
        common.unwrap_or_default()
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
    pub fn add_attachment_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        // 语言要在 spawn 之前取好：闭包里只有 `this` / `cx`，读不到 `AppState`
        let lang = self.language();
        cx.spawn(async move |this, cx| {
            let results = runtime()
                .spawn_blocking(move || paths.iter().map(|path| import_file(path, lang)).collect::<Vec<_>>())
                .await
                .unwrap_or_else(|error| vec![Err(tr_args(lang, Key::AddAttachmentFailed, &[&error.to_string()]))]);
            update_state(&this, cx, |state, cx| state.finish_attachment_import(results, cx));
        })
        .detach();
    }

    fn finish_attachment_import(&mut self, results: Vec<Result<Attachment, String>>, cx: &mut Context<Self>) {
        let lang = self.language();
        let mut added = 0usize;
        let mut errors = Vec::new();
        for result in results {
            match result {
                Ok(attachment) => {
                    self.pending_attachments.push(attachment);
                    added += 1;
                }
                Err(error) => errors.push(error),
            }
        }
        if added > 0 {
            self.toast(
                ToastLevel::Success,
                tr_args(lang, Key::AttachmentsAdded, &[&added.to_string()]),
            );
        }
        if !errors.is_empty() {
            self.toast(ToastLevel::Error, errors.join("\n"));
        }
        cx.notify();
    }

    pub fn pick_attachments(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        // 过滤器要在起线程之前算好：`self` 进不了那个闭包
        let capabilities = self.common_capabilities();
        let vision = capabilities.contains(&Capability::Vision);
        let files = capabilities.contains(&Capability::Files);
        let (tx, rx) = tokio::sync::oneshot::channel();
        // 系统文件对话框会阻塞调用它的线程，放到单独的线程里
        std::thread::spawn(move || {
            let mut dialog = rfd::FileDialog::new()
                .set_title(tr(lang, Key::PickAttachmentTitle))
                .add_filter(tr(lang, Key::FilterCommonFiles), &common_exts(vision, files));
            if vision {
                dialog = dialog.add_filter(tr(lang, Key::FilterImages), IMAGE_EXTS);
            }
            if files {
                dialog = dialog.add_filter(tr(lang, Key::FilterDocuments), DOCUMENT_EXTS);
            }
            let files_picked = dialog
                .add_filter(tr(lang, Key::FilterTextCode), TEXT_EXTS)
                .add_filter(tr(lang, Key::FilterAllFiles), &["*"])
                .pick_files();
            // 用户关掉对话框时接收端可能已经不在了，发送失败无所谓
            let _ = tx.send(files_picked);
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

/// 在后台线程把本地文件复制进附件目录
fn import_file(path: &Path, lang: AppLanguage) -> Result<Attachment, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_string();
    let (kind, mime) = file_store::detect_kind_and_mime(&name);
    let saved = file_store::save_file_from_path(path, lang)
        .map_err(|error| tr_args(lang, Key::AttachmentNotAdded, &[&name, &error.to_string()]))?;
    Ok(Attachment {
        id: Uuid::new_v4().to_string(),
        kind,
        name,
        mime: mime.to_string(),
        path: saved.storage_path,
        size: saved.size_bytes,
        hash: saved.hash,
    })
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

    /// 文件对话框按模型能力摆过滤器：看不懂图片的模型，列表里就不该有「图片文件」，
    /// 否则用户选完还是会被发送前的检查拦下，白选一趟。
    #[std::prelude::v1::test]
    fn the_picker_only_offers_what_the_model_can_take() {
        let text_only = common_exts(false, false);
        assert!(text_only.contains(&"rs") && text_only.contains(&"md"));
        assert!(!text_only.contains(&"png") && !text_only.contains(&"pdf"));

        let full = common_exts(true, true);
        assert!(full.contains(&"png") && full.contains(&"pdf") && full.contains(&"rs"));
    }
}
