//! 会话导出。
//!
//! 这里原本还放着本地工具的执行器（`execute_local_tool`、用 `:::` 拼参数那一套）。
//! P3-2 把工具改成了「模型可调用」之后，那部分整体搬去了 `local_tools.rs`——
//! 那边按结构化 JSON 参数执行、带权限分级，而且请求体里的工具声明和这里的
//! 执行分支必须是同一份定义，分成两个文件迟早会对不上。
//!
//! 现在这个模块只剩导出，名字保留是因为 `AGENTS.md` 的模块表里已经这么叫了。

use std::fs;

/// 导出当前对话为 Markdown 文档
pub fn export_session_to_markdown(
    title: &str,
    messages: &[crate::model::ChatMessage],
) -> Result<String, std::io::Error> {
    let sanitized_title = title.replace(|c: char| !c.is_alphanumeric() && c != '_' && c != '-', "_");
    let filename = format!("{}.md", sanitized_title);
    let mut md = format!(
        "# {}\n\n*Exported from Perch on {}*\n\n---\n\n",
        title,
        chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
    );

    for msg in messages {
        let speaker = if msg.role == "user" {
            "### 👤 User"
        } else {
            "### 🤖 Assistant"
        };
        md.push_str(&format!("{} ({})\n\n", speaker, msg.created_at));

        if let Some(reasoning) = &msg.reasoning_content {
            md.push_str(&format!(
                "> **Thought Process:**\n> {}\n\n",
                reasoning.replace('\n', "\n> ")
            ));
        }

        md.push_str(&format!("{}\n\n---\n\n", msg.content));
    }

    fs::write(&filename, &md)?;
    Ok(filename)
}
