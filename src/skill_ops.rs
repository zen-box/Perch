//! 技能（Skills）的读写操作：导入、停用 / 启用、重新扫描、打开目录。
//!
//! 状态分两处，各管各的：
//! - **装了什么**在磁盘上（`%APPDATA%\Perch\skills\`），扫描结果缓存在 `AppState::skills`；
//! - **停用了哪些**在配置里（`AppConfig::disabled_skills`）。
//!
//! 分开的理由：技能是用户自己往目录里放的，装一个不该还要来界面上勾一下才能用；
//! 而「不用了」是一个选择，得跟着配置走、重启之后还在。
//!
//! 导入要选目录（阻塞）也要复制文件（可能很大），所以**整件事都在后台线程上做完**，
//! 回界面线程只做「换快照 + 弹提示」。一个技能目录带着几百 MB 的参考资料是很正常的，
//! 在界面线程上复制会把窗口冻住（§7）。

use std::path::PathBuf;

use gpui_kit::*;

use crate::app::{AppState, ToastLevel, update_state};
use crate::i18n::{Key, tr, tr_args};
use crate::skills::SkillCatalog;

/// 导入的结果。**后台线程只负责算，界面线程负责说**——所以这里不装 `String` 以外的
/// 界面文案，更不在后台线程里调 `tr`（那时拿不到当前语言）。
enum ImportOutcome {
    /// 装好了，带上技能名
    Installed(String),
    /// 目录里没有 `SKILL.md`
    NoEntryFile,
    /// 已经有一个同名的了
    AlreadyInstalled(String),
    /// 目录名不是合法 UTF-8，当不了技能名
    BadFolderName,
    /// 复制过程中出错
    Failed(String),
}

impl AppState {
    /// 重新扫一遍技能目录。导入之后调，也可以让用户手动触发。
    ///
    /// **扫描放在界面线程上**：这里只读每个技能目录的 `SKILL.md`（每个上限 256 KB），
    /// 通常是几个文件的事；为它再搭一层后台任务，复杂度比省下的那一下更贵。
    pub(crate) fn reload_skills(&mut self, cx: &mut Context<Self>) {
        self.skills = SkillCatalog::reload();
        cx.notify();
    }

    /// 停用 / 启用一个技能。
    ///
    /// 存的是**停用名单**而不是启用名单：技能是用户自己往目录里放的，装一个就该能用；
    /// 存启用名单的话，每装一个新的都得回来勾一次。
    pub(crate) fn toggle_skill(&mut self, id: &str, cx: &mut Context<Self>) {
        match self.config.disabled_skills.iter().position(|item| item == id) {
            Some(ix) => {
                self.config.disabled_skills.remove(ix);
            }
            None => self.config.disabled_skills.push(id.to_string()),
        }
        // 存不下来也要重绘：开关已经拨过去了，不重绘就会停在旧样子，
        // 用户以为改成功了。失败提示由 `persist_config` 自己弹。
        self.persist_config(cx);
        cx.notify();
    }

    /// 在文件管理器里打开技能目录，让用户自己往里放 / 删。
    pub(crate) fn open_skills_dir(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        if let Err(error) = crate::paths::reveal(&crate::paths::skills_dir()) {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::SkillOpenFolderFailed, &[&error.to_string()]),
            );
            cx.notify();
        }
    }

    /// 从磁盘上的任意目录导入一个技能。
    pub(crate) fn import_skill(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title(tr(lang, Key::SkillImportTitle))
                .pick_folder();
            let _ = tx.send(picked);
        });

        cx.spawn(async move |this, cx| {
            let Ok(Some(path)) = rx.await else { return };
            // 复制也挪到后台线程：这一步要读整个目录，慢起来是真的慢
            let outcome = std::thread::spawn(move || install(path))
                .join()
                .unwrap_or_else(|_| ImportOutcome::Failed("copy thread panicked".to_string()));
            update_state(&this, cx, |state, cx| state.report_import(outcome, cx));
        })
        .detach();
    }

    /// 导入的结果回来了：换快照 + 弹提示。
    fn report_import(&mut self, outcome: ImportOutcome, cx: &mut Context<Self>) {
        let lang = self.language();
        match outcome {
            ImportOutcome::Installed(id) => {
                // 装完立刻重扫：新技能要马上出现在设置页和选择器里，
                // 让用户自己再点一次「重新扫描」是多余的一步
                self.skills = SkillCatalog::reload();
                self.toast(ToastLevel::Success, tr_args(lang, Key::SkillImported, &[&id]));
            }
            ImportOutcome::NoEntryFile => self.toast(ToastLevel::Error, tr(lang, Key::SkillNoEntryFile)),
            ImportOutcome::AlreadyInstalled(id) => {
                self.toast(ToastLevel::Error, tr_args(lang, Key::SkillAlreadyInstalled, &[&id]))
            }
            ImportOutcome::BadFolderName => self.toast(ToastLevel::Error, tr(lang, Key::SkillBadFolderName)),
            ImportOutcome::Failed(error) => {
                self.toast(ToastLevel::Error, tr_args(lang, Key::SkillImportFailed, &[&error]))
            }
        }
        cx.notify();
    }
}

/// 把一个选中的目录复制成技能。**在后台线程上跑，不碰任何界面状态。**
///
/// 校验按「复制之前能拦的都拦掉」的顺序做：没有 `SKILL.md` 的目录复制进来也只是个
/// 死目录，用户还得自己去删。
fn install(source: PathBuf) -> ImportOutcome {
    let root = crate::paths::skills_dir();
    let Some(name) = source.file_name().and_then(|name| name.to_str()).map(str::to_string) else {
        return ImportOutcome::BadFolderName;
    };
    if !source.join(crate::skills::SKILL_FILE).is_file() {
        return ImportOutcome::NoEntryFile;
    }
    let target = root.join(&name);
    // 已经躺在技能目录里、或者目标位置被占了：都按"已经装过"处理。
    //
    // 前一条是必须的，不只是"重复导入"这么简单：`source == target` 时
    // `copy_dir_all` 会往自己里面递归复制自己，直到路径太长或者磁盘写满。
    if source == target || target.exists() {
        return ImportOutcome::AlreadyInstalled(name);
    }
    match crate::paths::copy_dir_all(&source, &target) {
        Ok(()) => ImportOutcome::Installed(name),
        // 复制到一半失败：把半成品删掉。留着的话它会被当成一个装好的技能扫出来，
        // 而里面的 SKILL.md 可能正好没复制到——用户看到的是一个坏技能，且不知道坏在哪。
        Err(error) => {
            let _ = std::fs::remove_dir_all(&target);
            ImportOutcome::Failed(error.to_string())
        }
    }
}
