//! 审计日志的两个操作：开关、打开目录。
//!
//! 日志本身写在 `audit.rs` 里（那是数据层，不碰 GPUI）。这里只做两件"用户点了才发生"的事：
//! 拨开关、在文件管理器里打开日志目录。
//!
//! **开关是全局的**：`audit.rs` 用一个静态量控制，不走会话状态。理由见那儿的注释——
//! 记录的调用点散在 `agent_loop` 的七八个分支里，每个分支各查一遍配置迟早会漏。

use gpui_kit::*;

use crate::app::{AppState, ToastLevel};
use crate::i18n::{Key, tr_args};

impl AppState {
    /// 打开 / 关掉审计日志。关掉之后新产生的调用不再记录，**已有的日志文件不动**——
    /// 用户关它是为了"别再记了"，不是"把之前的删掉"。
    pub(crate) fn toggle_audit_log(&mut self, cx: &mut Context<Self>) {
        self.config.audit_log_enabled = !self.config.audit_log_enabled;
        // 先同步给数据层再存盘：存盘失败时界面上的开关已经拨过去了，
        // 这时候"记录行为"和"界面显示"必须一致，不能一个开一个关。
        crate::audit::set_enabled(self.config.audit_log_enabled);
        self.persist_config(cx);
        cx.notify();
    }

    /// 在文件管理器里打开日志目录。
    pub(crate) fn open_audit_log_dir(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        if let Err(error) = crate::paths::reveal(&crate::paths::logs_dir()) {
            self.toast(
                ToastLevel::Error,
                tr_args(lang, Key::AuditLogOpenFolderFailed, &[&error.to_string()]),
            );
            cx.notify();
        }
    }
}
