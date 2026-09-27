//! 智能体的「项目目录」：选、换、清，以及它引出的那几条界面判断。
//!
//! 状态只有一份，存在 `ChatSession::tools.workspace`（一个字符串），这里只做读写和判断，
//! **不另建缓存**——两份状态迟早会出现「按钮显示 A、执行时按 B 算」这种自相矛盾。
//!
//! 这个目录为什么这么重要：它同时是**相对路径的基准**、**命令的工作目录**、
//! 和**「里面 / 外面」的分界线**（目录之外每次都要用户点头）。
//! 没设它的时候本机工具一个都不给——"拿程序自己的安装目录兜底"正是这次要消掉的那个坑
//! （产品决策，见 AGENTS.md §11）。
//!
//! 选目录要走系统文件夹对话框，那是阻塞的，**不能放在界面线程**（会把窗口冻住），
//! 所以开一条后台线程、选完用 channel 递回来，和导入备份是同一条路。

use gpui_kit::*;

use crate::app::{AppState, update_state};
use crate::i18n::{Key, tr};
use crate::local_tools::ProjectDir;

impl AppState {
    /// 当前会话的项目目录。没设、或者存的是相对路径（基准不确定，等于没有边界）时是 `None`。
    pub(crate) fn session_workspace(&self) -> Option<ProjectDir> {
        self.storage
            .get_active_session()
            .and_then(|session| session.workspace())
    }

    /// 本机工具现在是不是因为「没选项目目录」而用不了。
    ///
    /// 界面拿它决定要不要提示。只在智能体模式下有意义——对话模式本来就不该有本机工具。
    pub(crate) fn workspace_missing(&self) -> bool {
        self.session_is_agent() && self.session_workspace().is_none()
    }

    /// 弹系统文件夹对话框选一个项目目录。
    pub(crate) fn pick_session_workspace(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        let (tx, rx) = tokio::sync::oneshot::channel();
        std::thread::spawn(move || {
            let picked = rfd::FileDialog::new()
                .set_title(tr(lang, Key::WorkspacePickTitle))
                .pick_folder();
            let _ = tx.send(picked);
        });

        cx.spawn(async move |this, cx| {
            let Ok(Some(path)) = rx.await else { return };
            update_state(&this, cx, |state, cx| {
                // 存绝对路径的字符串形式。`ProjectDir::parse` 把相对路径判成没设，
                // 而对话框给的一定是绝对路径，这里不用额外处理。
                state.set_session_workspace(Some(path.display().to_string()), cx);
            });
        })
        .detach();
    }

    /// 清掉项目目录。清完本机工具立刻不可用——闸门是按需算的，没有缓存要清。
    pub(crate) fn clear_session_workspace(&mut self, cx: &mut Context<Self>) {
        self.set_session_workspace(None, cx);
    }

    /// 写进当前会话并落盘。
    ///
    /// 换目录**不清** `disabled_tools`：那份黑名单是按工具名记的，换目录之后名字照样对得上；
    /// 清掉反而会让用户遇到「我明明停用过某个工具」的困惑。
    fn set_session_workspace(&mut self, value: Option<String>, cx: &mut Context<Self>) {
        let Some(session) = self.storage.get_active_session_mut() else {
            return;
        };
        let tools = session.tools.get_or_insert_with(Default::default);
        tools.workspace = value;
        self.persist_storage(cx);
        cx.notify();
    }
}
