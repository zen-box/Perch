//! 会话级的工具来源：composer 上那个「对话 / 智能体」开关，以及「本次对话用哪些工具」。
//!
//! 状态只有一份，存在 `ChatSession::tools`（见 [`crate::model::SessionTools`]）。这里只是
//! 把它读写出来给界面用，**不另建缓存**——两份状态迟早会出现「模式说对话、清单却还在发」
//! 这种自相矛盾。
//!
//! **粒度是「来源」不是「工具」**：勾一台 MCP 服务器就是把它的全部工具交给模型，
//! 具体用哪个功能由模型按用户的问题自己挑。逐个工具勾选对普通人是门槛，
//! 想精细控制的走「高级」那一层（`disabled_tools`）。
//!
//! 这里做的全是「用户明确点了才发生」的改动。模型自己不能决定用不用工具，也不能决定
//! 用哪些——那是用户在这次对话开始前选好的（产品决策，见 AGENTS.md §11）。

use gpui_kit::*;

use crate::app::AppState;
use crate::config::{AppConfig, McpServerConfig};
use crate::llm_tools::ToolSpec;
use crate::mcp_ops::McpState;
use crate::model::{ChatSession, LegacyTools, SessionMode, SessionTools, ToolSource};
use crate::model_info::Capability;

/// 选择器里的一条来源下挂着的工具（只在「高级」折叠里显示）。
pub(crate) struct ToolOption {
    /// 勾选时存下来的名字：本机工具就是它自己，MCP 工具是暴露名（`mcp__<服务器 id>__<工具>`）。
    pub name: String,
    pub description: String,
}

/// 来源的显示名。
///
/// 本机那项要跟着界面语言走，所以只在这里留个标记，文案由界面层渲染；
/// 服务器名是用户自己起的，原样显示。
///
/// （Skills 那一项等接入之后再加，见 `AGENT_MODE_PLAN.md` 第九节。）
pub(crate) enum SourceLabel {
    Local,
    McpServer(String),
}

/// 选择器里的一行来源。
pub(crate) struct SourceGroup {
    pub source: ToolSource,
    pub label: SourceLabel,
    /// 这条来源下有几个工具。0 表示还没连上、或者服务器一个工具都没暴露。
    pub tools: Vec<ToolOption>,
}

impl SourceGroup {
    pub fn tool_count(&self) -> usize {
        self.tools.len()
    }
}

/// 这个会话现在会带上哪些工具。**闸门只有这一份实现**。
///
/// 选择器的计数、按钮角标、真正发出去的清单全从这里取，免得出现
/// 「选择器里显示 13 个、实际发出去 0 个」这种对不上的情况。
///
/// 顺序固定：本机工具在前，MCP 按 `config.mcp_servers` 的顺序。同一份工具集每次
/// 序列化出来要逐字节一致，对端才能命中 prompt 缓存——**所以这里只做过滤，绝不重排**。
pub(crate) fn session_tool_specs(
    session: &ChatSession,
    local_tools_enabled: bool,
    mcp: &McpState,
    servers: &[McpServerConfig],
) -> Vec<ToolSpec> {
    // 走 `wants_source` 而不是自己展开来源表：对话模式必须剔掉 `Local` 那条结构性保证
    // 在 `SessionTools` 里，闸门这边照它执行就行，别在这里再实现一遍。
    let wants = |source: &ToolSource| session.tools.as_ref().is_some_and(|tools| tools.wants_source(source));
    let mut specs = Vec::new();

    // 本机工具。`wants_source` 已经把对话模式下的 `Local` 挡掉了，这里只管全局开关：
    // 那个开关**只管本机**，不再卡 MCP（它以前叫「本地工具」却管着 MCP，语义是错的）。
    if local_tools_enabled && wants(&ToolSource::Local) {
        specs.extend(crate::local_tools::specs());
    }

    for (server, tools) in mcp.usable_by_server(servers) {
        let source = ToolSource::Mcp {
            server_id: server.id.clone(),
        };
        if wants(&source) {
            specs.extend(tools.into_iter().map(|tool| tool.spec()));
        }
    }

    specs.retain(|spec| !session.is_tool_disabled(&spec.name));
    specs
}

/// 切模式之后该往存档里写哪份来源表。
///
/// 抽成纯函数是因为 `edit_session_tools` 要 GPUI 的 `Context`、测不了，
/// 而这里的规则（什么时候该补本机来源）恰恰是最容易写错的那一条。
///
/// `tools` 传的是**改模式之前**的状态。
fn sources_after_mode_change(tools: &SessionTools, agent: bool) -> Option<Vec<ToolSource>> {
    // 从对话切到智能体、而来源表里没有本机：补上。
    //
    // 为什么该补：对话模式下本机那一行根本不显示，用户没机会对它表过态。
    // 不补的话，用户勾了一台 MCP 服务器、再切到智能体，会得到"一个能碰文件的智能体
    // 却碰不到文件"——切模式这个动作看着就像没生效。
    //
    // 什么时候不补：本来就在智能体模式（`was_agent`），说明用户是在能看到本机那一行的
    // 情况下选择不要它的，那就尊重他的选择。
    if agent && !tools.is_agent() && !tools.stored_sources().contains(&ToolSource::Local) {
        let mut sources = tools.stored_sources();
        sources.insert(0, ToolSource::Local);
        return Some(sources);
    }
    tools.sources.clone()
}

/// 把老格式（`enabled` + `picked`）的会话工具状态展开成新格式。
///
/// **必须在 `AppConfig` 读完之后调**：老数据里 `picked: None` 的含义是
/// 「当时能用的全带」，展开要知道配置里有哪些服务器。放在反序列化里做不了这件事。
///
/// 为什么要展开而不是按新默认值算：新默认是「智能体只带本机」，直接套上去，
/// 老用户的 MCP 工具会**无声消失**——他没改过任何设置，工具却不见了。
/// 数据向后兼容是铁律，这里宁可把他的现状原样保留（他要改自己会去改）。
///
/// 返回是否改动过；改动过就要落盘。
pub(crate) fn migrate_legacy_tool_state(sessions: &mut [ChatSession], config: &AppConfig) -> bool {
    // 只算启用的服务器：停用的那台本来就没往清单里贡献过任何工具
    let server_ids: Vec<String> = config
        .mcp_servers
        .iter()
        .filter(|server| server.enabled)
        .map(|server| server.id.clone())
        .collect();
    let local_names: Vec<String> = crate::local_tools::specs().into_iter().map(|spec| spec.name).collect();

    let mut changed = false;
    for session in sessions.iter_mut() {
        let Some(tools) = session.tools.as_mut() else {
            continue;
        };
        let Some(legacy) = tools.take_legacy() else {
            continue;
        };
        let mut sources = match legacy {
            LegacyTools::All => {
                let mut list = vec![ToolSource::Local];
                list.extend(
                    server_ids
                        .iter()
                        .cloned()
                        .map(|server_id| ToolSource::Mcp { server_id }),
                );
                list
            }
            LegacyTools::Picked(names) => names
                .iter()
                .filter_map(|name| source_of(name, &local_names, &server_ids))
                .collect(),
        };
        sources.dedup();
        tools.sources = Some(sources);
        changed = true;
    }
    changed
}

/// 从老数据里存下的工具名反推它属于哪个来源。
///
/// 认不出来的名字直接丢掉——和旧代码「对不上的名字在组装清单时自然被过滤掉」一个道理：
/// 服务器被删了、或者工具被重命名了，那条记录本来就已经失效。
fn source_of(name: &str, local_names: &[String], server_ids: &[String]) -> Option<ToolSource> {
    if local_names.iter().any(|local| local == name) {
        return Some(ToolSource::Local);
    }
    let (server_id, _) = crate::mcp::parse_tool_name(name)?;
    // 服务器可能已经被删了：那就没有这条来源，别造一个指向不存在服务器的记录
    server_ids.iter().any(|id| id == server_id).then(|| ToolSource::Mcp {
        server_id: server_id.to_string(),
    })
}

impl AppState {
    /// 当前会话是不是「智能体」模式（可以碰本机文件）。
    ///
    /// 只是第一道闸：全局的「允许智能体读写本机文件」开关和模型的 `Capability::Tools`
    /// 各自还有一道，三处都过了才真的带工具，见 [`session_tool_specs`]。
    pub(crate) fn session_is_agent(&self) -> bool {
        self.storage.get_active_session().is_some_and(ChatSession::is_agent)
    }

    /// 这条来源在当前会话里勾上了没有。
    ///
    /// 用「存档那份」而不是「生效那份」：对话模式下本机那一行根本不显示，
    /// 但切回智能体时它该还是勾着的，不能因为切过一次模式就丢。
    pub(crate) fn is_source_picked(&self, source: &ToolSource) -> bool {
        match self
            .storage
            .get_active_session()
            .and_then(|session| session.tools.as_ref())
        {
            None => false,
            Some(tools) => tools.stored_sources().iter().any(|item| item == source),
        }
    }

    /// 这次要用的模型支不支持工具调用。
    ///
    /// 选择器拿它提示用户「勾了也不会生效」，免得对着一个不支持函数的模型反复切换模式。
    pub(crate) fn tools_supported_by_model(&self) -> bool {
        let (provider_id, model_id) = self.active_target();
        self.model_capabilities(&provider_id, &model_id)
            .contains(&Capability::Tools)
    }

    /// 这个工具在当前会话里被单独停用了吗。
    ///
    /// 和 [`Self::is_source_picked`] 一起给「高级」那一层用：来源勾了、但这个工具被点名不要。
    pub(crate) fn session_has_disabled(&self, name: &str) -> bool {
        self.storage
            .get_active_session()
            .is_some_and(|session| session.is_tool_disabled(name))
    }

    /// 打开选择器时要展示的来源：本机一项（**只有智能体模式才出现**），
    /// 每台「有工具可给」的 MCP 服务器各一项。
    ///
    /// 顺序跟着 `config.mcp_servers` 走，和真正发出去的清单一致，用户好对照。
    pub(crate) fn source_groups(&self) -> Vec<SourceGroup> {
        let mut groups = Vec::new();

        // 本机那一行只在智能体模式下出现。对话模式不摆它，是因为摆出来也只能灰着——
        // 而灰着的勾选框比不显示更让人困惑（想勾勾不上，还不知道为什么）。
        if self.session_is_agent() {
            let local = crate::local_tools::specs();
            if !local.is_empty() {
                groups.push(SourceGroup {
                    source: ToolSource::Local,
                    label: SourceLabel::Local,
                    tools: local
                        .into_iter()
                        .map(|spec| ToolOption {
                            name: spec.name,
                            description: spec.description,
                        })
                        .collect(),
                });
            }
        }

        for (server, tools) in self.mcp.usable_by_server(&self.config.mcp_servers) {
            groups.push(SourceGroup {
                source: ToolSource::Mcp {
                    server_id: server.id.clone(),
                },
                label: SourceLabel::McpServer(server.name.clone()),
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

    /// 当前会话实际会带上的工具数量，用来做按钮角标。
    ///
    /// 走的是和真正发出去时**同一个**函数，所以这个数字不会骗人。
    pub(crate) fn picked_tool_count(&self) -> usize {
        self.storage
            .get_active_session()
            .map(|session| {
                session_tool_specs(
                    session,
                    self.config.local_tools_enabled,
                    &self.mcp,
                    &self.config.mcp_servers,
                )
                .len()
            })
            .unwrap_or(0)
    }

    /// 切「对话 / 智能体」。
    ///
    /// 切到对话**不动**已勾的来源：用户可能只是这一轮想省点 token，切回来还该是原来那套。
    /// （对话模式自己会把本机来源挡掉，所以"切过去就碰不到硬盘"仍然成立。）
    ///
    /// 切到智能体时**补上本机来源**：对话模式下那一行根本不显示，用户没机会对它表过态。
    /// 不补的话「切到智能体」这个动作看着就像没生效——它还是碰不到文件。
    pub(crate) fn set_session_mode(&mut self, agent: bool, cx: &mut Context<Self>) {
        self.edit_session_tools(cx, move |tools| {
            // 先算来源、再改模式：算的时候要看的是"切之前是什么模式"
            tools.sources = sources_after_mode_change(tools, agent);
            tools.mode = if agent { SessionMode::Agent } else { SessionMode::Chat };
        });
    }

    /// 勾上 / 取消一条来源。
    ///
    /// **不自动切模式**：勾一台 MCP 服务器在对话模式下也是成立的（那正是这次改动的
    /// 目的之一），没必要把用户推进智能体模式去。
    pub(crate) fn toggle_session_source(&mut self, source: &ToolSource, cx: &mut Context<Self>) {
        let source = source.clone();
        self.edit_session_tools(cx, move |tools| {
            let mut sources = tools.stored_sources();
            match sources.iter().position(|item| item == &source) {
                Some(ix) => {
                    sources.remove(ix);
                }
                None => sources.push(source),
            }
            tools.sources = Some(sources);
        });
    }

    /// 全选 / 全不选。
    ///
    /// 只动**这次看得见**的来源：对话模式看不到本机那一项，点「全不选」就不该把
    /// 本机那条也一起抹掉（切回智能体时凭空少一项，用户会以为丢了设置）。
    pub(crate) fn set_all_session_sources(&mut self, picked: bool, cx: &mut Context<Self>) {
        let visible: Vec<ToolSource> = self.source_groups().into_iter().map(|group| group.source).collect();
        self.edit_session_tools(cx, move |tools| {
            let mut sources = tools.stored_sources();
            if picked {
                for source in visible {
                    if !sources.contains(&source) {
                        sources.push(source);
                    }
                }
            } else {
                sources.retain(|source| !visible.contains(source));
            }
            tools.sources = Some(sources);
        });
    }

    /// 单独停用 / 恢复一个工具（「高级」那一层）。
    pub(crate) fn toggle_disabled_tool(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.to_string();
        self.edit_session_tools(cx, move |tools| {
            match tools.disabled_tools.iter().position(|item| item == &name) {
                Some(ix) => {
                    tools.disabled_tools.remove(ix);
                }
                None => tools.disabled_tools.push(name),
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
        // 首次改动：从默认值起步（对话模式、不带任何来源）
        let tools = session.tools.get_or_insert_with(SessionTools::default);
        edit(tools);
        self.persist_storage(cx);
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::McpTransport;

    /// 智能体的默认工具状态：没动过选择器 = 只带本机工具、不带任何 MCP。
    fn agent_tools() -> SessionTools {
        SessionTools {
            mode: SessionMode::Agent,
            ..Default::default()
        }
    }

    fn server(id: &str, enabled: bool) -> McpServerConfig {
        McpServerConfig {
            id: id.to_string(),
            name: id.to_string(),
            enabled,
            transport: McpTransport::Stdio {
                command: "npx".into(),
                args: Vec::new(),
                cwd: None,
            },
            secret_ref: String::new(),
            disabled_tools: Vec::new(),
        }
    }

    fn config_with(servers: Vec<McpServerConfig>) -> AppConfig {
        AppConfig {
            mcp_servers: servers,
            ..AppConfig::default()
        }
    }

    /// 造一个只关心工具状态的会话表。
    fn session_with(tools: SessionTools) -> Vec<ChatSession> {
        let mut session = ChatSession::new("t".into(), "f".into(), "m".into(), "p".into());
        session.tools = Some(tools);
        vec![session]
    }

    fn sources_of(sessions: &[ChatSession]) -> Option<Vec<ToolSource>> {
        sessions[0].tools.as_ref().unwrap().sources.clone()
    }

    #[std::prelude::v1::test]
    fn legacy_all_expands_to_every_enabled_server_plus_local() {
        // 老数据 `{"enabled": true, "picked": null}` 的含义是"当时能用的全带"。
        // 直接套新默认值（只带本机）会让老用户的 MCP 工具无声消失。
        let raw: SessionTools = serde_json::from_str(r#"{"enabled":true,"picked":null}"#).unwrap();
        let mut sessions = session_with(raw);
        let config = config_with(vec![server("fetch", true), server("off", false)]);

        assert!(migrate_legacy_tool_state(&mut sessions, &config));
        assert_eq!(
            sources_of(&sessions),
            Some(vec![
                ToolSource::Local,
                ToolSource::Mcp {
                    server_id: "fetch".into()
                },
            ]),
            "停用的服务器不该被带进来"
        );
    }

    #[std::prelude::v1::test]
    fn legacy_picked_is_turned_back_into_sources() {
        let raw: SessionTools = serde_json::from_str(
            r#"{"enabled":true,"picked":["read_file","mcp__fetch__fetch","mcp__gone__x","nonsense"]}"#,
        )
        .unwrap();
        let mut sessions = session_with(raw);
        let config = config_with(vec![server("fetch", true)]);

        assert!(migrate_legacy_tool_state(&mut sessions, &config));
        assert_eq!(
            sources_of(&sessions),
            Some(vec![
                ToolSource::Local,
                ToolSource::Mcp {
                    server_id: "fetch".into()
                },
            ]),
            "服务器已被删的名字和认不出来的名字都该丢掉"
        );
    }

    #[std::prelude::v1::test]
    fn migration_only_runs_once() {
        let raw: SessionTools = serde_json::from_str(r#"{"enabled":true,"picked":null}"#).unwrap();
        let mut sessions = session_with(raw);
        let config = config_with(vec![server("fetch", true)]);

        assert!(migrate_legacy_tool_state(&mut sessions, &config));
        // 第二次不该再改动：标记已经取走了。否则用户手动取消勾选后，
        // 下次启动又会被"展开"回原样，改了等于没改。
        assert!(!migrate_legacy_tool_state(&mut sessions, &config));
    }

    #[std::prelude::v1::test]
    fn new_format_sessions_are_left_alone() {
        let tools = SessionTools {
            sources: Some(Vec::new()),
            ..agent_tools()
        };
        let mut sessions = session_with(tools.clone());
        let config = config_with(vec![server("fetch", true)]);
        assert!(!migrate_legacy_tool_state(&mut sessions, &config));
        assert_eq!(sources_of(&sessions), tools.sources);
    }

    #[std::prelude::v1::test]
    fn the_global_switch_only_gates_local_tools() {
        // 这条是修 bug：全局开关以前叫「本地工具」却卡着 MCP——用户关着它、只勾一台
        // MCP 服务器，结果一个工具都发不出去。现在它只管本机。
        let mcp = McpState::default();

        // 对话模式 + 勾了 MCP：开关关着也照样允许（清单空是因为没连上服务器）
        let chat = ChatSession {
            tools: Some(SessionTools {
                sources: Some(vec![ToolSource::Mcp {
                    server_id: "fetch".into(),
                }]),
                ..SessionTools::default()
            }),
            ..ChatSession::new("t".into(), "f".into(), "m".into(), "p".into())
        };
        assert!(session_tool_specs(&chat, false, &mcp, &[]).is_empty());
        assert!(
            !chat.tool_sources().is_empty(),
            "对话模式下 MCP 来源是生效的，只有本机被剔掉"
        );

        // 智能体 + 本机来源：开关关着就该一个本机工具都没有
        let agent = ChatSession {
            tools: Some(agent_tools()),
            ..ChatSession::new("t".into(), "f".into(), "m".into(), "p".into())
        };
        assert!(session_tool_specs(&agent, false, &mcp, &[]).is_empty());
        assert!(!session_tool_specs(&agent, true, &mcp, &[]).is_empty());
    }

    #[std::prelude::v1::test]
    fn switching_to_agent_brings_the_local_source_along() {
        // 在对话里勾了一台 MCP 服务器，然后切到智能体：本机来源要补上，
        // 否则"能碰文件的智能体"碰不到文件，切模式看着像没生效。
        let chat_with_mcp = SessionTools {
            sources: Some(vec![ToolSource::Mcp {
                server_id: "fetch".into(),
            }]),
            ..SessionTools::default()
        };
        assert_eq!(
            sources_after_mode_change(&chat_with_mcp, true),
            Some(vec![
                ToolSource::Local,
                ToolSource::Mcp {
                    server_id: "fetch".into()
                },
            ])
        );

        // 本来就在智能体里、用户主动取消过本机：切模式不该把它加回来
        let agent_without_local = SessionTools {
            sources: Some(vec![ToolSource::Mcp {
                server_id: "fetch".into(),
            }]),
            ..agent_tools()
        };
        assert_eq!(
            sources_after_mode_change(&agent_without_local, true),
            agent_without_local.sources
        );

        // 切到对话不动来源表（本机那一项留着，切回智能体时还在）
        assert_eq!(sources_after_mode_change(&agent_tools(), false), agent_tools().sources);
    }

    #[std::prelude::v1::test]
    fn disabling_one_tool_takes_it_out_of_the_list() {
        let session = ChatSession {
            tools: Some(SessionTools {
                disabled_tools: vec!["run_command".into()],
                ..agent_tools()
            }),
            ..ChatSession::new("t".into(), "f".into(), "m".into(), "p".into())
        };
        let specs = session_tool_specs(&session, true, &McpState::default(), &[]);
        assert!(!specs.is_empty());
        assert!(!specs.iter().any(|spec| spec.name == "run_command"));
        assert!(specs.iter().any(|spec| spec.name == "read_file"));
    }
}
