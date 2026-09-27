//! 会话级的工具选择：composer 上那个「对话 / 智能体」开关，以及「本次对话用哪些工具」。
//!
//! 状态只有一份，存在 `ChatSession::tools`（见 [`crate::model::SessionTools`]）。这里只是
//! 把它读写出来给界面用，**不另建缓存**——两份状态迟早会出现「模式说对话、清单却还在发」
//! 这种自相矛盾。
//!
//! 这里做的全是「用户明确点了才发生」的改动。模型自己不能决定用不用工具，也不能决定
//! 用哪些——那是用户在这次对话开始前选好的（产品决策，见 AGENTS.md §11）。

use gpui_kit::*;

use crate::app::AppState;
use crate::model::SessionTools;
use crate::model_info::Capability;

/// 工具选择器里的一项。
pub(crate) struct ToolOption {
    /// 勾选时存下来的名字：本机工具就是它自己，MCP 工具是暴露名（`mcp__<服务器 id>__<工具>`）。
    pub name: String,
    pub description: String,
}

/// 分组标题。
///
/// 本机那组的标题要跟着界面语言走，所以只在这里留个标记，文案由界面层渲染；
/// 服务器名是用户自己起的，原样显示。
pub(crate) enum ToolGroupLabel {
    Local,
    McpServer(String),
}

/// 工具选择器里的一组（本机工具 / 某台 MCP 服务器 / 将来的 Skills）。
pub(crate) struct ToolGroup {
    pub label: ToolGroupLabel,
    pub tools: Vec<ToolOption>,
}

impl AppState {
    /// 当前会话是不是「智能体」模式（会把工具清单交给模型）。
    ///
    /// 只是第一道闸：全局的「本地工具」总开关和模型的 `Capability::Tools` 各自还有一道，
    /// 三处都过了才真的带工具，见 `reply_ops::tool_list_for`。
    pub(crate) fn session_tools_enabled(&self) -> bool {
        self.storage
            .get_active_session()
            .is_some_and(|session| session.tools_enabled())
    }

    /// 这次要用的模型支不支持工具调用。
    ///
    /// 选择器拿它提示用户「勾了也不会生效」，免得对着一个不支持函数的模型反复切换模式。
    pub(crate) fn tools_supported_by_model(&self) -> bool {
        let (provider_id, model_id) = self.active_target();
        self.model_capabilities(&provider_id, &model_id)
            .contains(&Capability::Tools)
    }

    /// 打开选择器时要展示的分组：本机一组，每台「有工具可给」的 MCP 服务器各一组。
    ///
    /// 顺序跟着 `config.mcp_servers` 走，和真正发出去的清单一致，用户好对照。
    pub(crate) fn tool_groups(&self) -> Vec<ToolGroup> {
        let mut groups = Vec::new();

        let local = crate::local_tools::specs();
        if !local.is_empty() {
            groups.push(ToolGroup {
                label: ToolGroupLabel::Local,
                tools: local
                    .into_iter()
                    .map(|spec| ToolOption {
                        name: spec.name,
                        description: spec.description,
                    })
                    .collect(),
            });
        }

        for (server, tools) in self.mcp.usable_by_server(&self.config.mcp_servers) {
            groups.push(ToolGroup {
                label: ToolGroupLabel::McpServer(server.name.clone()),
                tools: tools
                    .into_iter()
                    .map(|tool| ToolOption {
                        name: tool.exposed.clone(),
                        description: tool.description.clone(),
                    })
                    .collect(),
            });
        }

        groups
    }

    /// 当前所有可用工具的名字（本机 + MCP），顺序与 [`Self::tool_groups`] 一致。
    ///
    /// 勾选是**存名字不存下标**：服务器顺序变了、某台服务器被删了，名字都还对得上，
    /// 对不上的在组装清单时自然被过滤掉（见 `reply_ops::tool_list_for`）。
    pub(crate) fn available_tool_names(&self) -> Vec<String> {
        self.tool_groups()
            .into_iter()
            .flat_map(|group| group.tools.into_iter().map(|tool| tool.name))
            .collect()
    }

    /// 这个工具在当前会话里勾上了没有。
    ///
    /// **不看模式**：切到对话模式后选择器仍然显示上次勾的那一套，切回智能体就是它，
    /// 不会因为切过一次模式就把选择清空。
    pub(crate) fn is_tool_picked(&self, name: &str) -> bool {
        match self
            .storage
            .get_active_session()
            .and_then(|session| session.tools.as_ref())
        {
            // 没设过 = 全带（默认）
            None => true,
            Some(tools) => tools.wants(name),
        }
    }

    /// 当前会话实际会带上的工具数量，用来做按钮角标。
    pub(crate) fn picked_tool_count(&self) -> usize {
        let names = self.available_tool_names();
        names.iter().filter(|name| self.is_tool_picked(name)).count()
    }

    /// 切「对话 / 智能体」。
    ///
    /// 切到对话**不动**已勾的选择：用户可能只是这一轮想省点 token，切回来还该是原来那套。
    pub(crate) fn set_session_tools_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.edit_session_tools(cx, |tools| tools.enabled = enabled);
    }

    /// 勾上 / 取消一个工具。
    ///
    /// 会顺带把模式切成智能体：勾工具就是「我想让它用工具」这个意图最直接的表达，
    /// 勾完却什么都没发生才是真的让人困惑。
    pub(crate) fn toggle_session_tool(&mut self, name: &str, cx: &mut Context<Self>) {
        let all = self.available_tool_names();
        self.edit_session_tools(cx, |tools| {
            tools.enabled = true;
            let mut picked = match &tools.picked {
                Some(picked) => picked.clone(),
                // 第一次动选择器：先把「当前能用的」全当作已勾，再摘掉这一个
                None => all.clone(),
            };
            match picked.iter().position(|item| item == name) {
                Some(ix) => {
                    picked.remove(ix);
                }
                None => picked.push(name.to_string()),
            }
            tools.picked = normalize(picked, &all);
        });
    }

    /// 全选 / 全不选。
    ///
    /// 全选写成「隐式全带」而不是把所有名字列一遍：落盘更小，而且以后新加的工具
    /// （比如新装了一台 MCP 服务器）会自动带上，符合「没动过就是全带」的默认。
    pub(crate) fn set_all_session_tools(&mut self, picked: bool, cx: &mut Context<Self>) {
        self.edit_session_tools(cx, |tools| {
            if picked {
                *tools = SessionTools::all();
            } else {
                tools.enabled = true;
                tools.picked = Some(Vec::new());
            }
        });
    }

    /// 改当前会话的工具状态，然后落盘。
    ///
    /// 会话不存在（理论上不会，`StorageData` 至少有一个会话）时什么都不做，不 panic。
    fn edit_session_tools(&mut self, cx: &mut Context<Self>, edit: impl FnOnce(&mut SessionTools)) {
        let Some(session) = self.storage.get_active_session_mut() else {
            return;
        };
        // 首次改动：从默认值（对话模式、全带）起步
        let tools = session.tools.get_or_insert_with(SessionTools::default);
        edit(tools);
        self.persist_storage(cx);
        cx.notify();
    }
}

/// 勾选集合正好等于「当前全部」时退回 `None`（隐式全带）。
///
/// 这样「取消一个又勾回来」不会在存档里留下一个恰好等于全集的显式白名单，
/// 否则以后新装 MCP 服务器时，那些新工具反而不会自动带上。
///
/// `all` 为空时不塌缩：`None`（隐式全带）与 `Some(vec![])`（显式全不选）在数值上都是
/// 空集，但语义相反——将来新装了服务器，前者会自动带上、后者不会。
fn normalize(picked: Vec<String>, all: &[String]) -> Option<Vec<String>> {
    if !all.is_empty() && picked.len() == all.len() && all.iter().all(|name| picked.contains(name)) {
        None
    } else {
        Some(picked)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[std::prelude::v1::test]
    fn a_full_selection_collapses_to_the_implicit_default() {
        let all = vec!["a".to_string(), "b".to_string()];
        assert_eq!(normalize(vec!["a".into(), "b".into()], &all), None);
        assert_eq!(normalize(vec!["b".into(), "a".into()], &all), None);
    }

    #[std::prelude::v1::test]
    fn a_partial_selection_stays_explicit() {
        let all = vec!["a".to_string(), "b".to_string()];
        assert_eq!(normalize(vec!["a".into()], &all), Some(vec!["a".into()]));
        assert_eq!(normalize(Vec::new(), &all), Some(Vec::new()));
    }

    #[std::prelude::v1::test]
    fn an_empty_catalogue_never_collapses_to_all() {
        // 「全不选」必须留在存档里：塌缩成 `None` 就等于「以后有什么就带什么」，
        // 和用户点的那一下意思正好相反。
        assert_eq!(normalize(Vec::new(), &[]), Some(Vec::new()));
        assert_eq!(normalize(Vec::new(), &["a".to_string()]), Some(Vec::new()));
    }
}
