//! 用户装进数据目录的 Skills：扫描、解析、按需读取。
//!
//! 格式是 `%APPDATA%\Perch\skills\<目录名>\SKILL.md`，开头一小段 front matter 写
//! `name` 和 `description`，正文是给模型看的做法说明；同一个目录里可以放脚本和资料。
//! 系统提示词里只列「名字 + 一句描述」，正文要模型自己调 `load_skill` 去取——
//! 平时不占上下文，这正是 Skills 存在的理由。
//!
//! **为什么不并进 `local_tools.rs`**：那两个工具读的只有 Perch 自己的 skills 目录，
//! 碰不到用户的文件系统，所以**对话模式也能用**（见 `AGENT_MODE_PLAN.md` 第九节）；
//! 而 `local_tools` 里每一个工具都要过「项目目录 + 越界授权」那套边界，套在这里是错的。
//! 但 skill 附带的**脚本要执行**时得走 `run_command`——那才是本机工具，只有智能体模式能用。
//!
//! 不引入 YAML 依赖（`serde_yaml` 已停止维护）：front matter 只用得上两个字符串字段，
//! 手写几十行比拉一个依赖更省事，也更好测。
//!
//! ⚠️ 这里**不碰 GPUI**。扫描结果缓存在 [`SkillCatalog`] 里，由 `skill_ops.rs` 负责重建——
//! `tool_ops::source_groups` 在**渲染期间**跑，那里做文件 I/O 是错的。

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::Instant;

use serde_json::json;

use crate::llm_tools::{ToolResult, ToolSpec};
use crate::local_tools::{MAX_RESULT_CHARS, PendingTool, truncate_middle};

/// 每个 skill 的入口文件名，固定，不认别的。
pub const SKILL_FILE: &str = "SKILL.md";
pub const LOAD_SKILL: &str = "load_skill";
pub const READ_SKILL_FILE: &str = "read_skill_file";

/// 一个 SKILL.md 最大读这么多字节。再大就不是"说明"了，多半是塞了别的东西。
const MAX_SKILL_BYTES: u64 = 256 * 1024;
/// `read_skill_file` 一次最多读这么多字节。
const MAX_FILE_BYTES: u64 = 512 * 1024;
/// 一个 skill 最多列这么多附带文件。
const MAX_BUNDLED_FILES: usize = 200;
/// 目录递归深度上限，防病态结构。
const MAX_DEPTH: usize = 8;
/// 清单里一句描述最多这么长。描述是手写的，写成一整段会把系统提示词撑起来。
const MAX_DESCRIPTION_CHARS: usize = 200;

/// 一个装好的 skill。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skill {
    /// 目录名。**模型调 `load_skill` 用的就是它**，所以它同时是 id 和显示名。
    pub id: String,
    /// front matter 里的 `name`，只给界面显示用；缺了就用 `id`。
    pub title: String,
    /// front matter 里的 `description`。清单里给模型看的就是这一句。
    pub description: String,
    /// SKILL.md 正文（不含 front matter）。
    pub body: String,
    pub dir: PathBuf,
    /// 附带文件的相对路径（`/` 分隔），不含 SKILL.md 自己。
    pub files: Vec<String>,
}

/// 装好的 skill 快照。
///
/// **缓存而不是每次现扫**：`tool_ops::source_groups` 在渲染期间跑，那里做文件 I/O 会把
/// 界面拖住；而工具执行在后台线程上，又拿不到 `AppState`。两边共用这一份快照，
/// 由 `skill_ops::reload_skills` 重建：启动、导入之后，以及打开输入框上的「技能」、
/// 切到技能设置页的时候（手工拷进目录的技能靠这两处才会被看到）。
///
/// 正文也一起缓存：一个 SKILL.md 通常几 KB，全装进来也就几百 KB，
/// 换来的是 `load_skill` 和「手动插入」都不用再读盘。
#[derive(Clone, Debug, Default)]
pub struct SkillCatalog {
    items: Vec<Skill>,
}

impl SkillCatalog {
    /// 重新扫一遍 skills 目录。
    ///
    /// 读不出来（目录不存在、权限不对）时返回空的：**不能因为一个目录读不了就崩**，
    /// 那会让整个程序起不来。skills 目录在 `paths::skills_dir()` 里已经建过。
    pub fn reload() -> Self {
        let root = crate::paths::skills_dir();
        let mut items = Vec::new();
        if let Ok(entries) = fs::read_dir(&root) {
            for entry in entries.flatten() {
                if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
                    continue;
                }
                let Some(id) = entry.file_name().to_str().map(str::to_string) else {
                    // 非 UTF-8 的目录名没法当参数传给模型，跳过
                    continue;
                };
                if let Some(skill) = read_skill(&id, &entry.path()) {
                    items.push(skill);
                }
            }
        }
        items.sort_by(|a, b| a.id.cmp(&b.id));
        Self { items }
    }

    pub fn all(&self) -> &[Skill] {
        &self.items
    }

    pub fn get(&self, id: &str) -> Option<&Skill> {
        self.items.iter().find(|skill| skill.id == id)
    }

    /// 界面上真正会用的那些（`disabled` 里的不算）。
    pub fn enabled<'a>(&'a self, disabled: &'a [String]) -> impl Iterator<Item = &'a Skill> + 'a {
        self.items
            .iter()
            .filter(move |skill| !disabled.iter().any(|id| id == &skill.id))
    }

    /// 这次对话真正能用的技能：会话里勾了的、而且没被全局停用的。
    ///
    /// 勾过、后来又被删掉的技能自然不在里面（快照里没有它），不用另做清理。
    pub fn picked<'a>(&'a self, picked: &'a [String], disabled: &'a [String]) -> impl Iterator<Item = &'a Skill> + 'a {
        self.enabled(disabled)
            .filter(move |skill| picked.iter().any(|id| id == &skill.id))
    }

    /// 只含 [`Self::picked`] 那几个技能的快照，交给后台执行 `load_skill` / `read_skill_file`。
    ///
    /// 执行时只认这一份：这次对话没勾的技能，模型就算猜到名字也读不到。
    pub fn picked_only(&self, picked: &[String], disabled: &[String]) -> Self {
        Self {
            items: self.picked(picked, disabled).cloned().collect(),
        }
    }
}

/// 读一个 skill 目录。读不出来就返回 `None`（当它不存在，不报错）。
fn read_skill(id: &str, dir: &Path) -> Option<Skill> {
    let path = dir.join(SKILL_FILE);
    let meta = fs::metadata(&path).ok()?;
    if !meta.is_file() || meta.len() > MAX_SKILL_BYTES {
        return None;
    }
    let text = fs::read_to_string(&path).ok()?;
    let (head, body) = split_front_matter(&text);
    let field = |key: &str| {
        head.iter()
            .find(|(name, _)| name == key)
            .map(|(_, value)| value.trim().to_string())
            .filter(|value| !value.is_empty())
    };
    let description = field("description").unwrap_or_else(|| first_line(&body));
    Some(Skill {
        id: id.to_string(),
        title: field("name").unwrap_or_else(|| id.to_string()),
        description: truncate_middle(&description.replace('\n', " "), MAX_DESCRIPTION_CHARS),
        body,
        dir: dir.to_path_buf(),
        files: bundled_files(dir),
    })
}

/// 没有 `description` 时拿正文第一行顶上——总比在清单里显示一个空白强。
fn first_line(body: &str) -> String {
    body.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .trim_start_matches('#')
        .trim()
        .to_string()
}

/// 拆开 front matter 和正文。**纯函数，可以直接测。**
///
/// 认不出来时不报错，整篇当正文：用户手写的文件格式千奇百怪，
/// 为此让整个 skill 从清单里消失不值得。缺 `description` 由调用方兜底。
fn split_front_matter(text: &str) -> (Vec<(String, String)>, String) {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let lines: Vec<&str> = text.lines().collect();
    if lines.first().map(|line| line.trim()) != Some("---") {
        return (Vec::new(), text.trim().to_string());
    }
    // 找收尾那行。只认**开头和结尾都是 `---`** 的写法，避免把正文里的一条横线当成结束
    let Some(end) = lines
        .iter()
        .skip(1)
        .position(|line| line.trim() == "---")
        .map(|ix| ix + 1)
    else {
        return (Vec::new(), text.trim().to_string());
    };
    let head = lines[1..end]
        .iter()
        .filter_map(|line| {
            let (key, value) = line.split_once(':')?;
            let key = key.trim().to_ascii_lowercase();
            let value = value.trim().trim_matches(|c| c == '"' || c == '\'').trim().to_string();
            (!key.is_empty()).then_some((key, value))
        })
        .collect();
    (head, lines[end + 1..].join("\n").trim().to_string())
}

/// skill 目录里的附带文件（相对路径，`/` 分隔）。
fn bundled_files(root: &Path) -> Vec<String> {
    let mut out = Vec::new();
    collect_files(root, root, 0, &mut out);
    out.sort();
    out
}

fn collect_files(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
    if depth > MAX_DEPTH || out.len() >= MAX_BUNDLED_FILES {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if out.len() >= MAX_BUNDLED_FILES {
            return;
        }
        let path = entry.path();
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            collect_files(root, &path, depth + 1, out);
        } else if kind.is_file()
            && let Ok(rel) = path.strip_prefix(root)
        {
            let rel = rel.to_string_lossy().replace('\\', "/");
            // 入口文件自己不用列：正文已经整篇给出去了
            if rel != SKILL_FILE {
                out.push(rel);
            }
        }
    }
}

/// 把相对路径拼到 skill 目录上，**不许爬出去**。
///
/// 按 `components()` 判而不是查字符串里有没有 `..`：`a/../../b`、`./..` 这类写法靠字符串
/// 判断很容易漏，而且 Windows 上还有 `C:\` 这种绝对路径——只接受全是普通组件的相对路径，
/// 其余一律拒掉。
fn safe_join(root: &Path, rel: &str) -> Option<PathBuf> {
    let rel = rel.trim();
    if rel.is_empty() {
        return None;
    }
    let mut path = PathBuf::new();
    for component in Path::new(rel).components() {
        match component {
            Component::Normal(part) => path.push(part),
            // `.` 不改变指向，放行（模型很爱写 `./docs/a.md`）；
            // `..`、根、盘符一律拒——那些都能爬到技能目录外面。
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(root.join(path))
}

/// 两个工具声明。**只在会话勾了 Skill 来源、而且真的装了 skill 时才发出去。**
pub fn specs() -> Vec<ToolSpec> {
    vec![
        ToolSpec::new(
            LOAD_SKILL,
            "读取一个 Skill 的完整说明。可用的 Skill 清单见系统提示词，先用这个读完说明再动手。",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Skill 的名字（清单里冒号前面那个）" }
                },
                "required": ["name"]
            }),
        ),
        ToolSpec::new(
            READ_SKILL_FILE,
            "读取某个 Skill 附带的文件（脚本、模板、参考资料）。",
            json!({
                "type": "object",
                "properties": {
                    "name": { "type": "string", "description": "Skill 的名字" },
                    "path": { "type": "string", "description": "相对该 Skill 目录的路径，见 load_skill 返回的清单" }
                },
                "required": ["name", "path"]
            }),
        ),
    ]
}

/// 这两个工具名归 skills 管，不归 `local_tools`。
///
/// `agent_loop` 靠它决定走哪条执行路，以及要不要弹授权卡片——**这两个工具永远免确认**：
/// 它们只读 Perch 自己的 skills 目录，碰不到用户的文件系统。
pub fn is_skill_tool(name: &str) -> bool {
    matches!(name, LOAD_SKILL | READ_SKILL_FILE)
}

/// 执行一次调用。**会读盘，调用方必须放到后台线程**（和 `local_tools::execute` 一样）。
pub fn execute(pending: &PendingTool, catalog: &SkillCatalog) -> ToolResult {
    let started = Instant::now();
    let (content, is_error) = match pending.name.as_str() {
        LOAD_SKILL => load_skill(pending, catalog),
        READ_SKILL_FILE => read_skill_file(pending, catalog),
        other => (format!("There is no tool named `{other}`."), true),
    };
    ToolResult {
        id: pending.id.clone(),
        name: pending.name.clone(),
        content: truncate_middle(&content, MAX_RESULT_CHARS),
        is_error,
        duration_ms: started.elapsed().as_millis() as u64,
        exit_code: None,
    }
}

fn load_skill(pending: &PendingTool, catalog: &SkillCatalog) -> (String, bool) {
    let Some(name) = pending.string_arg("name") else {
        return ("缺少参数 name，或者它不是字符串。".to_string(), true);
    };
    let Some(skill) = catalog.get(name.trim()) else {
        return (unknown_skill_message(name, catalog), true);
    };
    let mut out = format!("# {}\n\n{}", skill.title, skill.body);
    if !skill.files.is_empty() {
        out.push_str("\n\n## 附带文件\n\n");
        for file in &skill.files {
            out.push_str(&format!("- {file}\n"));
        }
    }
    (out, false)
}

/// 名字对不上时把清单列出来——只说"没有"的话，模型下一轮多半还会猜一次。
fn unknown_skill_message(name: &str, catalog: &SkillCatalog) -> String {
    let available: Vec<&str> = catalog.all().iter().map(|skill| skill.id.as_str()).collect();
    if available.is_empty() {
        return format!("There is no skill named `{name}`, and no skill is installed.");
    }
    format!(
        "There is no skill named `{name}`. Installed skills: {}.",
        available.join(", ")
    )
}

fn read_skill_file(pending: &PendingTool, catalog: &SkillCatalog) -> (String, bool) {
    let Some(name) = pending.string_arg("name") else {
        return ("缺少参数 name，或者它不是字符串。".to_string(), true);
    };
    let Some(rel) = pending.string_arg("path") else {
        return ("缺少参数 path，或者它不是字符串。".to_string(), true);
    };
    let Some(skill) = catalog.get(name.trim()) else {
        return (unknown_skill_message(name, catalog), true);
    };
    let Some(path) = safe_join(&skill.dir, rel) else {
        return (format!("路径 `{rel}` 不合法：只能读这个 Skill 目录里的文件。"), true);
    };
    match fs::metadata(&path) {
        Ok(meta) if meta.is_file() && meta.len() > MAX_FILE_BYTES => {
            return (format!("`{rel}` 有 {} 字节，太大了，不读。", meta.len()), true);
        }
        Ok(meta) if meta.is_file() => {}
        _ => return (format!("Skill `{}` 里没有 `{rel}`。", skill.id), true),
    }
    match fs::read(&path) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(text) => (text, false),
            Err(_) => (format!("`{rel}` 不是文本文件，读不了。"), true),
        },
        Err(error) => (format!("无法读取 {rel}：{error}"), true),
    }
}

/// 系统提示词里列 Skill 的那一段。**只给名字和一句描述**，正文等模型自己来取。
///
/// 一条都没有时返回 `None`，调用方就别往提示词里塞这一节——空清单只会让模型
/// 反复去试一个不存在的工具。
pub fn catalog_prompt<'a>(skills: impl Iterator<Item = &'a Skill>) -> Option<String> {
    // 写成一行（不用 `\` 续行）：`i18n_skip.txt` 是按**解码后的完整字面量**逐条比对的，
    // 续行会在字符串里留下反斜杠和换行，白名单里没法照抄。改这里记得同步白名单。
    let mut out = String::from(
        "# Skills\n\n这些是用户装好的技能包。要用哪个，先调 `load_skill` 读它的完整说明，它附带的脚本和资料用 `read_skill_file` 取。\n\n",
    );
    let mut count = 0;
    for skill in skills {
        out.push_str(&format!("- {}: {}\n", skill.id, skill.description));
        count += 1;
    }
    (count > 0).then_some(out)
}

/// 测试用的快照构造。
///
/// **只给测试用**，不做成公开构造函数：正常路径只有 `reload()` 一条，
/// 多一条「凭空造一个快照」的路，迟早会有别的地方拿它绕开磁盘扫描。
#[cfg(test)]
impl SkillCatalog {
    pub(crate) fn for_tests(items: Vec<Skill>) -> Self {
        Self { items }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn skill(id: &str, description: &str) -> Skill {
        Skill {
            id: id.to_string(),
            title: id.to_string(),
            description: description.to_string(),
            body: format!("{id} 的做法"),
            dir: PathBuf::from("/tmp").join(id),
            files: Vec::new(),
        }
    }

    #[test]
    fn front_matter_is_split_from_the_body() {
        let (head, body) =
            split_front_matter("---\nname: 写周报\ndescription: 把流水账整理成周报\n---\n\n先看本周的提交。\n");

        assert_eq!(
            head,
            vec![
                ("name".to_string(), "写周报".to_string()),
                ("description".to_string(), "把流水账整理成周报".to_string()),
            ]
        );
        assert_eq!(body, "先看本周的提交。");
    }

    #[test]
    fn front_matter_quotes_and_bom_are_tolerated() {
        let (head, body) = split_front_matter("\u{feff}---\nName: \"写周报\"\n---\n正文\n");

        assert_eq!(head, vec![("name".to_string(), "写周报".to_string())]);
        assert_eq!(body, "正文");
    }

    #[test]
    fn a_file_without_front_matter_is_all_body() {
        let (head, body) = split_front_matter("# 直接用\n\n正文\n");

        assert!(head.is_empty());
        assert_eq!(body, "# 直接用\n\n正文");
    }

    #[test]
    fn an_unclosed_front_matter_is_treated_as_body() {
        // 有开头没结尾：宁可整篇当正文，也不要把它切没了
        let (head, body) = split_front_matter("---\nname: 写周报\n正文\n");

        assert!(head.is_empty());
        assert!(body.contains("正文"));
    }

    #[test]
    fn a_relative_path_may_not_escape_the_skill_folder() {
        let root = Path::new("/skills/demo");

        assert_eq!(
            safe_join(root, "docs/a.md"),
            Some(PathBuf::from("/skills/demo/docs/a.md"))
        );
        assert_eq!(safe_join(root, "./a.md"), Some(PathBuf::from("/skills/demo/a.md")));
        // 这几种都能爬出去，一个都不能放过
        assert!(safe_join(root, "../secrets.txt").is_none());
        assert!(safe_join(root, "docs/../../secrets.txt").is_none());
        assert!(safe_join(root, "").is_none());
    }

    #[test]
    fn the_catalog_prompt_lists_only_what_is_installed() {
        let one = skill("weekly", "把流水账整理成周报");
        let two = skill("deploy", "按检查清单发布");

        let text = catalog_prompt([&one, &two].into_iter()).unwrap();
        assert!(text.contains("- weekly: 把流水账整理成周报"));
        assert!(text.contains("- deploy: 按检查清单发布"));
        // 正文不能进提示词——那正是 Skills 要避免的事
        assert!(!text.contains("的做法"));

        assert!(catalog_prompt([].iter()).is_none(), "一个都没有时不该塞一节空的");
    }

    #[test]
    fn disabled_skills_drop_out_of_the_enabled_list() {
        let catalog = SkillCatalog {
            items: vec![skill("weekly", "周报"), skill("deploy", "发布")],
        };
        let disabled = vec!["weekly".to_string()];

        let ids: Vec<&str> = catalog.enabled(&disabled).map(|skill| skill.id.as_str()).collect();
        assert_eq!(ids, vec!["deploy"]);
        assert_eq!(catalog.all().len(), 2, "停用只是不展示，不该从快照里消失");
    }

    #[test]
    fn reading_a_missing_skill_reports_what_is_installed() {
        let catalog = SkillCatalog {
            items: vec![skill("weekly", "周报")],
        };
        let pending = PendingTool {
            id: "call_1".into(),
            name: LOAD_SKILL.into(),
            arguments: json!({ "name": "nope" }),
        };

        let (content, is_error) = load_skill(&pending, &catalog);

        assert!(is_error);
        assert!(content.contains("weekly"), "要顺带把清单给它：{content}");
    }
}
