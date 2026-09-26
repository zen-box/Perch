use std::fs;
use std::process::Command;

pub struct AgentToolResult {
    pub display: String,
    pub output: String,
    pub is_error: bool,
}

/// 执行本地真实工具调用
pub fn execute_local_tool(name: &str, arg: &str) -> AgentToolResult {
    match name {
        "read_file" => {
            let path_str = arg.trim().trim_matches('"');
            match fs::read_to_string(path_str) {
                Ok(content) => {
                    let line_count = content.lines().count();
                    AgentToolResult {
                        display: format!("Read {} ({} lines)", path_str, line_count),
                        output: content,
                        is_error: false,
                    }
                }
                Err(e) => AgentToolResult {
                    display: format!("Read {} (Error)", path_str),
                    output: format!("Failed to read file {}: {}", path_str, e),
                    is_error: true,
                },
            }
        }
        "write_file" => {
            let parts: Vec<&str> = arg.splitn(2, ":::").collect();
            if parts.len() == 2 {
                let path_str = parts[0].trim().trim_matches('"');
                let content = parts[1];
                match fs::write(path_str, content) {
                    Ok(_) => AgentToolResult {
                        display: format!("Write > {}", path_str),
                        output: format!("Successfully wrote {} bytes to {}", content.len(), path_str),
                        is_error: false,
                    },
                    Err(e) => AgentToolResult {
                        display: format!("Write > {} (Error)", path_str),
                        output: format!("Failed to write file {}: {}", path_str, e),
                        is_error: true,
                    },
                }
            } else {
                AgentToolResult {
                    display: "Write (Invalid args)".to_string(),
                    output: "Invalid format. Expected: <path>:::<content>".to_string(),
                    is_error: true,
                }
            }
        }
        "list_dir" => {
            let dir_str = if arg.trim().is_empty() {
                "."
            } else {
                arg.trim().trim_matches('"')
            };
            match fs::read_dir(dir_str) {
                Ok(entries) => {
                    let mut items = Vec::new();
                    for entry in entries.flatten() {
                        if let Ok(name) = entry.file_name().into_string() {
                            let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
                            if is_dir {
                                items.push(format!("{}/", name));
                            } else {
                                items.push(name);
                            }
                        }
                    }
                    items.sort();
                    AgentToolResult {
                        display: format!("List directory {}", dir_str),
                        output: items.join("\n"),
                        is_error: false,
                    }
                }
                Err(e) => AgentToolResult {
                    display: format!("List directory {} (Error)", dir_str),
                    output: format!("Failed to read directory {}: {}", dir_str, e),
                    is_error: true,
                },
            }
        }
        "git_status" => match Command::new("git").arg("status").arg("--short").output() {
            Ok(output) => {
                let text = String::from_utf8_lossy(&output.stdout).to_string();
                let trimmed = text.trim();
                let summary = if trimmed.is_empty() {
                    "Clean working tree (no changes)".to_string()
                } else {
                    trimmed.to_string()
                };
                AgentToolResult {
                    display: "Git status".to_string(),
                    output: summary,
                    is_error: false,
                }
            }
            Err(e) => AgentToolResult {
                display: "Git status (Error)".to_string(),
                output: format!("Git command failed: {}", e),
                is_error: true,
            },
        },
        "bash" | "exec_command" => {
            let cmd_str = arg.trim();
            #[cfg(target_os = "windows")]
            let output_res = Command::new("powershell")
                .args(["-NoProfile", "-Command", cmd_str])
                .output();

            #[cfg(not(target_os = "windows"))]
            let output_res = Command::new("sh").args(["-c", cmd_str]).output();

            match output_res {
                Ok(output) => {
                    let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                    let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                    let combined = if stderr.is_empty() {
                        stdout
                    } else if stdout.is_empty() {
                        stderr
                    } else {
                        format!("{}\n{}", stdout, stderr)
                    };
                    AgentToolResult {
                        display: format!("Bash > {}", cmd_str),
                        output: if combined.trim().is_empty() {
                            "(Empty output)".to_string()
                        } else {
                            combined
                        },
                        is_error: !output.status.success(),
                    }
                }
                Err(e) => AgentToolResult {
                    display: format!("Bash > {} (Error)", cmd_str),
                    output: format!("Failed to execute: {}", e),
                    is_error: true,
                },
            }
        }
        _ => AgentToolResult {
            display: format!("Unknown tool: {}", name),
            output: format!("Tool {} is not supported.", name),
            is_error: true,
        },
    }
}

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
