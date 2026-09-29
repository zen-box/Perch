//! 会话级的工具状态：模式（对话 / 智能体）、勾了哪些工具来源、用哪些技能、智能体的权限档和项目目录。
//!
//! 持久化在 `ChatSession::tools` 里（`model.rs`）。怎么读写、怎么组装成真正发出去的
//! 工具清单，在 `tool_ops.rs`。

use serde::{Deserialize, Serialize};

/// 会话的工作模式。
///
/// **核心区别是「能不能碰本机文件」，不是「带不带工具」**：对话也能用 MCP 与 Skills，
/// 只是碰不到你的硬盘。这个区分必须落在代码里（见 [`SessionTools::effective_sources`]），
/// 不能只靠界面不显示那个勾选框——界面漏一处，承诺就成了空话。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionMode {
    /// 问答、写作、翻译、查资料。能用 MCP 与 Skills，**不给本机工具**。
    #[default]
    Chat,
    /// 让它动手改东西：对话的全部 + 读写本机文件 + 执行命令。
    Agent,
}

/// 一个工具来源。
///
/// **选择器按这个粒度勾，不按单个工具勾**：勾一台 MCP 服务器就是把它的全部工具
/// 交给模型，具体用哪个功能由模型按用户的问题自己挑——那本来就是模型该干的活。
/// 让用户先搞清 fetch 有哪几个功能再逐个决定，对普通人是门槛。
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ToolSource {
    /// 本机文件与命令。**只有智能体模式才生效**。
    Local,
    /// 某一台 MCP 服务器（勾一台 = 整台都带）。
    Mcp { server_id: String },
    /// **老数据**：勾了「技能」这一整条来源，当时的意思是「装了的、没停用的全带」。
    ///
    /// 现在技能按会话逐个勾选（[`SessionTools::skills`]），启动时由
    /// `tool_ops::migrate_legacy_skill_source` 展开成具体的技能。这个变体只为读得懂老存档
    /// 留着（持久化过的枚举变体不能删），新代码不会再写出它。
    Skill,
}

/// 智能体操作本机时的权限档。
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Permission {
    /// 只读本机文件免确认；写文件 / 跑命令 / 读敏感文件每次弹卡片。
    #[default]
    Default,
    /// 本机工具全部直接执行，不弹卡片。仍然保留两条硬底线（见 AGENTS.md §11）。
    Full,
}

/// 老格式（只有 `enabled` + `picked`）读进来时的暂存。
///
/// 老数据里 `picked: None` 的含义是**「当时能用的全带」**，而新模型的默认值是
/// 「智能体只带本机」——两者不是一个意思。直接按新语义读，老用户的 MCP 工具会
/// 无声消失，所以先把老字段原样接下来，等 `AppConfig` 加载完（那时才知道配置里
/// 有哪些服务器）再由应用层展开，见 `tool_ops::migrate_legacy_tool_state`。
#[derive(Clone, Debug, PartialEq)]
pub enum LegacyTools {
    /// 老数据 `picked: null`：当时能用的全带。
    All,
    /// 老数据 `picked: [...]`：用户显式勾过的这批工具名。
    Picked(Vec<String>),
}

/// 一个会话要用哪些工具、从哪些来源要、本机权限多大。
///
/// **这份状态是唯一的**：composer 上那个「对话 / 智能体」开关只是它的快捷表达。
/// 分成两份状态迟早会出现「模式说对话、清单却还在发」这种自相矛盾。
///
/// ⚠️ 新会话和旧数据都是 `ChatSession::tools == None`，按 **对话** 处理（一个工具都不带）。
/// 工具调用是有副作用的操作，默认不开比默认开安全；要用的用户在输入框上切一下就行。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(from = "SessionToolsRaw")]
pub struct SessionTools {
    /// 模式。对话不给本机工具，智能体给。
    #[serde(default)]
    pub mode: SessionMode,
    /// 勾上的工具来源。`None` = 没动过选择器，按模式的默认值算。
    ///
    /// 存**来源**不存工具名：勾一台服务器就是一条记录，服务器换了工具清单也不用改存档。
    /// 某台服务器被删了，对应的那条记录在组装清单时自然匹配不上，不用做迁移。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sources: Option<Vec<ToolSource>>,
    /// 智能体的本机权限档。对话模式下无意义。
    #[serde(default)]
    pub permission: Permission,
    /// 智能体的项目目录（绝对路径）。相对路径按它算，见 `local_tools.rs`。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workspace: Option<String>,
    /// 这次对话要用的技能（技能 id，也就是技能的目录名）。空 = 一个都不用。
    ///
    /// 和 MCP 一样由用户在输入框上逐个勾，默认一个都不勾：装了技能不等于每次对话都要用。
    /// 勾了之后技能被删掉的，那条记录在组装清单时自然对不上，不用做迁移。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    /// 单独停用的工具（本机工具名 / MCP 暴露名）。
    ///
    /// 来源勾上了、但用户点名不要的那些。存在这里而不是只靠"不勾来源"，
    /// 是为了保住「整台服务器都带，但这个别用」这种常见诉求。
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub disabled_tools: Vec<String>,
    /// 迁移暂存，**不写盘**。见 [`LegacyTools`]。
    #[serde(skip_serializing)]
    pub(crate) legacy: Option<LegacyTools>,
}

/// 反序列化用的中转结构。
///
/// 同时认两套字段：有 `mode` 的是新格式，只有 `enabled` / `picked` 的是老格式。
/// 用一个 `from` 中转而不是给每个字段写 `deserialize_with`，是因为老格式的
/// **字段组合**（而不是单个字段）决定了新值，写在 `From` 里一眼能看全。
#[derive(Deserialize)]
struct SessionToolsRaw {
    #[serde(default)]
    mode: Option<SessionMode>,
    #[serde(default)]
    sources: Option<Vec<ToolSource>>,
    #[serde(default)]
    permission: Permission,
    #[serde(default)]
    workspace: Option<String>,
    #[serde(default)]
    skills: Vec<String>,
    #[serde(default)]
    disabled_tools: Vec<String>,
    /// 老格式：是不是智能体模式。
    #[serde(default)]
    enabled: Option<bool>,
    /// 老格式：勾过的工具名。`None` 表示"没动过选择器"，当时的语义是**全带**。
    #[serde(default)]
    picked: Option<Vec<String>>,
}

impl From<SessionToolsRaw> for SessionTools {
    fn from(raw: SessionToolsRaw) -> Self {
        // 有 `mode` 就是新格式，直接用
        if let Some(mode) = raw.mode {
            return Self {
                mode,
                sources: raw.sources,
                permission: raw.permission,
                workspace: raw.workspace,
                skills: raw.skills,
                disabled_tools: raw.disabled_tools,
                legacy: None,
            };
        }
        // 老格式。来源这一项留空，由 `tool_ops::migrate_legacy_tool_state` 填——
        // 反序列化的时候拿不到配置里的服务器清单，没法在这里展开。
        let (mode, legacy) = match (raw.enabled.unwrap_or(false), raw.picked) {
            (true, None) => (SessionMode::Agent, Some(LegacyTools::All)),
            (true, Some(names)) => (SessionMode::Agent, Some(LegacyTools::Picked(names))),
            (false, None) => (SessionMode::Chat, None),
            (false, Some(names)) => (SessionMode::Chat, Some(LegacyTools::Picked(names))),
        };
        Self {
            mode,
            sources: None,
            permission: Permission::Default,
            workspace: None,
            skills: Vec::new(),
            disabled_tools: Vec::new(),
            legacy,
        }
    }
}

impl SessionTools {
    /// 这个会话是不是智能体模式（能碰本机文件）。
    ///
    /// 注意 `sources == None` 的含义**跟着模式走**：对话是「什么都不带」，
    /// 智能体是「只带本机工具」。所以切模式只要改 `mode` 就够了，
    /// 不需要另外造一份默认值——默认值本身是模式相关的。
    pub fn is_agent(&self) -> bool {
        self.mode == SessionMode::Agent
    }

    /// 存档里那份来源（`None` 时按模式默认值补上）。
    ///
    /// **改选择器要用这一份，不要用 [`Self::effective_sources`]**：后者在对话模式下
    /// 会把 `Local` 剔掉，拿它去回写就等于「在对话模式下点一次全不选，把本机那条
    /// 一起抹了」，切回智能体时凭空少一项。
    pub fn stored_sources(&self) -> Vec<ToolSource> {
        match &self.sources {
            Some(list) => list.clone(),
            None if self.is_agent() => vec![ToolSource::Local],
            None => Vec::new(),
        }
    }

    /// 真正生效的工具来源。
    ///
    /// ⚠️ **对话模式一定会把 `Local` 剔掉**，哪怕存档里塞了它（比如用户先勾了本机、
    /// 再切回对话）。「切回对话就碰不到硬盘」是给用户的承诺，得是**结构性**的，
    /// 不能指望界面记得把那个勾选框藏起来。
    pub fn effective_sources(&self) -> Vec<ToolSource> {
        let mut sources = self.stored_sources();
        if !self.is_agent() {
            sources.retain(|source| !matches!(source, ToolSource::Local));
        }
        sources
    }

    /// 这个来源这次要不要带。
    pub fn wants_source(&self, source: &ToolSource) -> bool {
        if matches!(source, ToolSource::Local) && !self.is_agent() {
            return false;
        }
        self.effective_sources().iter().any(|item| item == source)
    }

    /// 这个工具被单独停用了吗（来源勾了、但用户点名不要它）。
    pub fn is_disabled(&self, name: &str) -> bool {
        self.disabled_tools.iter().any(|item| item == name)
    }

    /// 取出迁移暂存。取走之后就是 `None`，不会再展开第二遍。
    pub(crate) fn take_legacy(&mut self) -> Option<LegacyTools> {
        self.legacy.take()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_mode_never_hands_out_local_tools() {
        // 这条是承诺的结构性保证：存档里塞了 `Local`，只要模式是对话就必须被剔掉。
        // 用户可能先勾了本机工具、再切回对话，界面那边漏藏一次勾选框不该变成安全漏洞。
        let tools = SessionTools {
            sources: Some(vec![
                ToolSource::Local,
                ToolSource::Mcp {
                    server_id: "fetch".into(),
                },
            ]),
            ..SessionTools::default()
        };
        assert_eq!(
            tools.effective_sources(),
            vec![ToolSource::Mcp {
                server_id: "fetch".into()
            }],
            "对话模式必须把本机来源剔掉，MCP 留着"
        );
        assert!(!tools.wants_source(&ToolSource::Local));

        // 反过来，智能体模式照样认它
        let agent = SessionTools {
            mode: SessionMode::Agent,
            ..tools
        };
        assert!(agent.wants_source(&ToolSource::Local));
    }

    #[test]
    fn legacy_tool_state_is_recognized_but_not_expanded_here() {
        // 老格式：`enabled` + `picked`。展开要等配置加载完（那时才知道有哪些服务器），
        // 所以这里只该留下一个「待展开」的标记，不能自己猜出一份来源表。
        let raw = r#"{"enabled":true,"picked":null}"#;
        let tools: SessionTools = serde_json::from_str(raw).unwrap();
        assert!(tools.is_agent());
        assert_eq!(tools.sources, None);
        assert_eq!(tools.legacy, Some(LegacyTools::All));

        let raw = r#"{"enabled":true,"picked":["read_file","mcp__fetch__fetch"]}"#;
        let tools: SessionTools = serde_json::from_str(raw).unwrap();
        assert!(tools.is_agent());
        assert_eq!(
            tools.legacy,
            Some(LegacyTools::Picked(vec![
                "read_file".into(),
                "mcp__fetch__fetch".into()
            ]))
        );

        // 老对话（`enabled: false`）没有来源要展开，直接就是空的
        let tools: SessionTools = serde_json::from_str(r#"{"enabled":false}"#).unwrap();
        assert!(!tools.is_agent());
        assert_eq!(tools.legacy, None);
        assert!(tools.effective_sources().is_empty());

        // 新格式认 `mode`，不再看老字段
        let tools: SessionTools = serde_json::from_str(r#"{"mode":"agent","enabled":false,"picked":["x"]}"#).unwrap();
        assert!(tools.is_agent());
        assert_eq!(tools.legacy, None);
    }

    #[test]
    fn the_legacy_marker_is_never_written_back() {
        // 展开标记是纯运行时的东西，写回磁盘就等于把一次性的迁移变成永久的字段。
        let tools: SessionTools = serde_json::from_str(r#"{"enabled":true,"picked":null}"#).unwrap();
        let json = serde_json::to_string(&tools).unwrap();
        assert!(!json.contains("legacy"), "{json}");
        assert!(!json.contains("enabled"), "{json}");
        assert!(!json.contains("picked"), "{json}");
        assert!(json.contains(r#""mode":"agent""#), "{json}");
    }
}
