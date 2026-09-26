use std::path::{Path, PathBuf};

use gpui_kit::*;
use uuid::Uuid;

use crate::app::{AppState, ToastLevel, ViewMode, runtime, update_state};
use crate::clipboard::{self, PastePayload};
use crate::file_store;
use crate::model::{Attachment, AttachmentKind};

impl AppState {
    /// 把本地文件加入待发送的附件。读取、计算哈希、复制都在后台线程做，大文件不会卡住界面。
    pub fn add_attachment_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        cx.spawn(async move |this, cx| {
            let results = runtime()
                .spawn_blocking(move || paths.iter().map(|path| import_file(path)).collect::<Vec<_>>())
                .await
                .unwrap_or_else(|error| vec![Err(format!("添加附件失败：{error}"))]);
            update_state(&this, cx, |state, cx| state.finish_attachment_import(results, cx));
        })
        .detach();
    }

    fn finish_attachment_import(&mut self, results: Vec<Result<Attachment, String>>, cx: &mut Context<Self>) {
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
            self.toast(ToastLevel::Success, format!("已添加 {added} 个附件"));
        }
        if !errors.is_empty() {
            self.toast(ToastLevel::Error, errors.join("\n"));
        }
        cx.notify();
    }

    pub fn pick_attachments(&mut self, cx: &mut Context<Self>) {
        let (tx, rx) = tokio::sync::oneshot::channel();
        // 系统文件对话框会阻塞调用它的线程，放到单独的线程里
        std::thread::spawn(move || {
            let files = rfd::FileDialog::new()
                .set_title("选择附件 (支持图片、文档、代码与文本)")
                .add_filter(
                    "常用文件 (图片/文档/代码)",
                    &[
                        "png", "jpg", "jpeg", "webp", "gif", "bmp", "pdf", "docx", "xlsx", "pptx", "txt", "md", "json",
                        "rs", "py", "js", "ts", "html", "css", "c", "cpp", "go", "java", "sql", "sh", "yaml", "yml",
                        "toml", "csv",
                    ],
                )
                .add_filter("图片文件", &["png", "jpg", "jpeg", "webp", "gif", "bmp"])
                .add_filter("文档 (PDF/Office)", &["pdf", "docx", "xlsx", "pptx"])
                .add_filter(
                    "文本与代码",
                    &[
                        "txt", "md", "json", "rs", "py", "js", "ts", "c", "cpp", "go", "java", "sql", "sh", "yaml",
                        "yml", "toml", "xml", "csv",
                    ],
                )
                .add_filter("所有文件 (*.*)", &["*"])
                .pick_files();
            // 用户关掉对话框时接收端可能已经不在了，发送失败无所谓
            let _ = tx.send(files);
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
                self.toast(ToastLevel::Info, "不能粘贴文件夹，请选择文件夹里的文件");
                cx.notify();
                true
            }
            PastePayload::Text(_) | PastePayload::Empty => false,
        }
    }

    /// 剪贴板图片在后台转换格式、保存，完成后加到附件里
    fn add_clipboard_image(&mut self, image: Image, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let result = runtime()
                .spawn_blocking(move || import_clipboard_image(&image))
                .await
                .unwrap_or_else(|error| Err(format!("粘贴图片失败：{error}")));
            update_state(&this, cx, |state, cx| {
                match result {
                    Ok(attachment) => {
                        state.pending_attachments.push(attachment);
                        state.toast(ToastLevel::Success, "已从剪贴板粘贴图片");
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
        if let Err(error) = std::process::Command::new("explorer").arg(path).spawn() {
            self.toast(ToastLevel::Error, format!("打开文件失败: {error}"));
            cx.notify();
        }
    }
}

/// 在后台线程把本地文件复制进附件目录
fn import_file(path: &Path) -> Result<Attachment, String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file")
        .to_string();
    let (kind, mime) = file_store::detect_kind_and_mime(&name);
    let saved = file_store::save_file_from_path(path).map_err(|error| format!("「{name}」没有添加：{error}"))?;
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
fn import_clipboard_image(image: &Image) -> Result<Attachment, String> {
    let prepared = clipboard::prepare_image(image)?;
    let name = format!(
        "paste_{}.{}",
        chrono::Local::now().format("%Y%m%d_%H%M%S"),
        prepared.extension
    );
    let saved = file_store::save_bytes(&prepared.bytes, &name, prepared.mime)
        .map_err(|error| format!("保存剪贴板图片失败：{error}"))?;
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
