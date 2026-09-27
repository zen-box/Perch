//! 会话与消息的操作：发送、重新生成、继续生成、编辑重发、删除、引用、多版本，以及本地工具与发送前的准备。

use gpui_kit::*;

use crate::agent_loop::{ApprovalSource, PendingToolApproval};
use crate::app::{AppState, ToastLevel};
use crate::i18n::{Key, tr};
use crate::llm::ChatMessageReq;
use crate::model::{ChatMessage, DEFAULT_SESSION_TITLE};
use crate::reply_ops::JobSpec;

impl AppState {
    pub fn cancel_streaming(&mut self, cx: &mut Context<Self>) {
        let senders: Vec<_> = self.active_streams.drain().map(|(_, sender)| sender).collect();
        for sender in senders {
            let _ = sender.send(());
        }
        // 正在执行的工具一并结束，循环也不再往下跑。要在下面统一复位 is_streaming 之前做：
        // 那一步会把执行中的占位块也标成完成，就来不及写「已停止」了
        self.stop_agent();
        self.is_streaming = false;
        for session in &mut self.storage.sessions {
            for message in &mut session.messages {
                message.is_streaming = false;
                for variant in &mut message.variants {
                    variant.is_streaming = false;
                }
            }
        }
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn active_target(&self) -> (String, String) {
        let (default_provider, default_model) = self.config.default_model_selection();
        self.storage
            .get_active_session()
            .map(|session| {
                (
                    if session.provider_id.is_empty() {
                        default_provider.clone()
                    } else {
                        session.provider_id.clone()
                    },
                    if session.model.is_empty() || session.model == "default" {
                        default_model.clone()
                    } else {
                        session.model.clone()
                    },
                )
            })
            .unwrap_or((default_provider, default_model))
    }

    pub fn compare_targets(&self) -> Vec<(String, String)> {
        let current = self.active_target();
        let mut targets = vec![current.clone()];
        for item in &self.compare_selection {
            if item != &current && !targets.contains(item) {
                targets.push(item.clone());
            }
        }
        targets
    }

    pub fn send_message(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lang = self.language();
        let user_prompt = self.chat_input.read(cx).value().trim().to_string();
        if (user_prompt.is_empty() && self.pending_attachments.is_empty()) || self.is_streaming {
            return;
        }
        if self.block_while_approval_pending(cx) {
            return;
        }
        if self
            .storage
            .get_active_session()
            .is_some_and(|session| session.has_unresolved_compare())
        {
            self.toast(ToastLevel::Error, tr(lang, Key::AdoptCompareFirst));
            cx.notify();
            return;
        }
        let targets = self.compare_targets();
        if self.block_unsupported_attachments(self.pending_attachments.clone(), &targets, cx) {
            return;
        }
        if !self.compare_selection.is_empty() && targets.len() >= 2 {
            self.send_compare(window, cx);
            return;
        }
        self.chat_input.update(cx, |input, cx| input.set_value("", window, cx));
        if self.handle_local_tool(&user_prompt, cx) {
            return;
        }
        let quote = self.pending_quote.take();
        self.prepare_user_turn(&user_prompt, quote);
        self.start_reply(None, cx);
    }

    pub fn send_compare(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let lang = self.language();
        let user_prompt = self.chat_input.read(cx).value().trim().to_string();
        if self.is_streaming || self.block_while_approval_pending(cx) {
            return;
        }
        if user_prompt.is_empty() && self.pending_attachments.is_empty() {
            self.toast(ToastLevel::Info, tr(lang, Key::CompareNeedInput));
            cx.notify();
            return;
        }
        let targets = self.compare_targets();
        if targets.len() < 2 {
            self.toast(ToastLevel::Error, tr(lang, Key::CompareNeedModels));
            cx.notify();
            return;
        }
        // 对比模式更要拦：几个模型里可能只有一部分接得住这批附件
        if self.block_unsupported_attachments(self.pending_attachments.clone(), &targets, cx) {
            return;
        }
        self.compare_selection.clear();
        self.chat_input.update(cx, |input, cx| input.set_value("", window, cx));
        let quote = self.pending_quote.take();
        self.prepare_user_turn(&user_prompt, quote);
        self.start_compare(&targets, cx);
    }

    pub fn regenerate_message(
        &mut self,
        message_id: &str,
        provider_id: Option<String>,
        model_id: Option<String>,
        cx: &mut Context<Self>,
    ) {
        let lang = self.language();
        if self.is_streaming || self.block_while_approval_pending(cx) {
            return;
        }
        let active_id = self.storage.active_session_id.clone();
        let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) else {
            return;
        };
        let Some(index) = session
            .messages
            .iter()
            .position(|message| message.id == message_id && message.role == "assistant")
        else {
            return;
        };
        if !session.messages[..index].iter().any(|message| message.role == "user") {
            self.toast(ToastLevel::Error, tr(lang, Key::NoUserMessageToRegenerate));
            cx.notify();
            return;
        }
        session.messages.truncate(index);
        if let (Some(provider_id), Some(model_id)) = (provider_id, model_id) {
            session.provider_id = provider_id;
            session.model = model_id;
        }
        self.start_reply(None, cx);
    }

    pub fn continue_message(&mut self, cx: &mut Context<Self>) {
        let lang = self.language();
        if self.is_streaming || self.block_while_approval_pending(cx) {
            return;
        }
        let active_id = self.storage.active_session_id.clone();
        let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) else {
            return;
        };
        let Some(message) = session.messages.last_mut() else {
            return;
        };
        if message.role != "assistant" || message.content.is_empty() || !message.variants.is_empty() {
            self.toast(ToastLevel::Error, tr(lang, Key::CanOnlyContinueAfterDone));
            cx.notify();
            return;
        }
        message.is_streaming = true;
        let message_id = message.id.clone();
        let model = message.model.clone();
        let provider_id = session.provider_id.clone();
        let mut history = self.history_messages(&active_id);
        history.push(ChatMessageReq::new("user", tr(lang, Key::ContinuePrompt)));
        let spec = JobSpec {
            session_id: &active_id,
            message_id: &message_id,
            variant_id: None,
            provider_id: &provider_id,
            model_id: &model,
            // 「继续」是把一段被截断的回答接着写完，不需要工具
            with_tools: false,
        };
        let Some(job) = self.make_job(spec, history) else {
            self.toast(ToastLevel::Error, tr(lang, Key::CurrentModelUnavailable));
            cx.notify();
            return;
        };
        self.spawn_jobs(vec![job], cx);
    }

    pub fn delete_message(&mut self, message_id: &str, cx: &mut Context<Self>) {
        let lang = self.language();
        if self.block_while_approval_pending(cx) {
            return;
        }
        if self.is_streaming {
            self.toast(ToastLevel::Error, tr(lang, Key::CannotDeleteWhileGenerating));
            cx.notify();
            return;
        }
        let active_id = self.storage.active_session_id.clone();
        if let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) {
            session.messages.retain(|message| message.id != message_id);
        }
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn quote_message(&mut self, message_id: &str, cx: &mut Context<Self>) {
        let quote = self.storage.get_active_session().and_then(|session| {
            session
                .messages
                .iter()
                .find(|message| message.id == message_id)
                .map(|message| {
                    let text = if message.content.is_empty() {
                        message
                            .variants
                            .iter()
                            .map(|variant| variant.content.as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    } else {
                        message.content.clone()
                    };
                    text.chars().take(800).collect::<String>()
                })
        });
        self.pending_quote = quote.filter(|text| !text.trim().is_empty());
        cx.notify();
    }

    pub fn clear_quote(&mut self, cx: &mut Context<Self>) {
        self.pending_quote = None;
        cx.notify();
    }

    pub fn begin_edit_message(&mut self, message_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let content = self.storage.get_active_session().and_then(|session| {
            session
                .messages
                .iter()
                .find(|message| message.id == message_id && message.role == "user")
                .map(|message| message.content.clone())
        });
        let Some(content) = content else { return };
        self.edit_message_id = Some(message_id.to_string());
        self.edit_message_input.update(cx, |input, cx| {
            input.set_value(&content, window, cx);
            input.focus(window, cx);
        });
    }

    pub fn confirm_edit_message(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        let Some(message_id) = self.edit_message_id.clone() else {
            return false;
        };
        let text = self.edit_message_input.read(cx).value().trim().to_string();
        if text.is_empty() || self.is_streaming || self.block_while_approval_pending(cx) {
            return false;
        }
        let active_id = self.storage.active_session_id.clone();
        let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) else {
            return false;
        };
        let Some(index) = session.messages.iter().position(|message| message.id == message_id) else {
            return false;
        };
        session.messages[index].content = text;
        session.messages.truncate(index + 1);
        self.edit_message_id = None;
        self.edit_message_input
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.start_reply(None, cx);
        true
    }

    pub fn adopt_variant(&mut self, message_id: &str, variant_id: &str, cx: &mut Context<Self>) {
        let active_id = self.storage.active_session_id.clone();
        let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) else {
            return;
        };
        let Some(message) = session.messages.iter_mut().find(|message| message.id == message_id) else {
            return;
        };
        let Some(variant) = message
            .variants
            .iter()
            .find(|variant| variant.id == variant_id)
            .cloned()
        else {
            return;
        };
        message.content = variant.content;
        message.reasoning_content = variant.reasoning_content;
        message.error = variant.error;
        message.model = variant.model.clone();
        message.prompt_tokens = variant.prompt_tokens;
        message.completion_tokens = variant.completion_tokens;
        message.speed_tps = variant.speed_tps;
        message.latency_ms = variant.latency_ms;
        message.is_streaming = false;
        // 保留 variants，以便用户随时左右翻页查看其他模型的回答
        session.model = variant.model;
        session.provider_id = variant.provider_id;
        self.persist_storage(cx);
        cx.notify();
    }

    pub fn switch_variant(&mut self, message_id: &str, target_variant_ix: usize, cx: &mut Context<Self>) {
        let active_id = self.storage.active_session_id.clone();
        let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) else {
            return;
        };
        let Some(message) = session.messages.iter_mut().find(|message| message.id == message_id) else {
            return;
        };
        if target_variant_ix >= message.variants.len() {
            return;
        }
        let variant = message.variants[target_variant_ix].clone();
        message.content = variant.content;
        message.reasoning_content = variant.reasoning_content;
        message.error = variant.error;
        message.model = variant.model.clone();
        message.prompt_tokens = variant.prompt_tokens;
        message.completion_tokens = variant.completion_tokens;
        message.speed_tps = variant.speed_tps;
        message.latency_ms = variant.latency_ms;
        session.model = variant.model;
        session.provider_id = variant.provider_id;
        self.persist_storage(cx);
        cx.notify();
    }

    /// 手打的斜杠命令。走的是和 Agent 循环**同一套执行器**，只是不经过模型：
    /// 用户已经明确说了要跑什么，没必要再让模型转述一遍（还费 token）。
    ///
    /// 结果存成没有调用 id 的 `tool` 消息，之后发给模型时当作一段普通文字
    /// （见 `model.rs::api_message`）。读敏感文件的命令和结果都标成只在本机显示：
    /// 用户同意读取是想自己看，不代表同意把 `.env`、私钥交给模型服务商。
    fn handle_local_tool(&mut self, user_prompt: &str, cx: &mut Context<Self>) -> bool {
        // 斜杠命令走的也是本机工具，所以和模型发起时守同一套闸门：全局开关关着不认，
        // **对话模式也不认**——「对话碰不到你的硬盘」是给用户的承诺，手打的命令
        // 不该是例外（否则用户切到对话、随手打一个 /read，读到的还是真文件）。
        if !self.config.local_tools_enabled || !self.session_is_agent() {
            return false;
        }
        let (tool, arguments, needs_confirm, sensitive) =
            if user_prompt.starts_with("/ls") || user_prompt.starts_with("/dir") {
                let path = user_prompt
                    .trim_start_matches("/ls")
                    .trim_start_matches("/dir")
                    .trim()
                    .to_string();
                ("list_directory", serde_json::json!({ "path": path }), false, false)
            } else if let Some(path) = user_prompt.strip_prefix("/read ") {
                // 敏感文件手打也要确认——模型能读的东西，手打同样能读
                let sensitive = crate::local_tools::is_sensitive_path(std::path::Path::new(path.trim()));
                (
                    "read_file",
                    serde_json::json!({ "path": path.trim() }),
                    sensitive,
                    sensitive,
                )
            } else if user_prompt == "/git" || user_prompt.starts_with("/git ") {
                ("git_status", serde_json::json!({}), false, false)
            } else if let Some(command) = user_prompt
                .strip_prefix("/bash ")
                .or_else(|| user_prompt.strip_prefix("/sh "))
            {
                (
                    "run_command",
                    serde_json::json!({ "command": command.trim() }),
                    true,
                    false,
                )
            } else {
                return false;
            };
        let session_id = self.storage.active_session_id.clone();
        let mut command_message = ChatMessage::new_user(user_prompt.to_string());
        command_message.local_only = sensitive;
        self.push_to_session(&session_id, command_message);

        let pending = crate::local_tools::PendingTool {
            id: String::new(),
            name: tool.to_string(),
            arguments,
        };
        if needs_confirm {
            self.agent.pending = Some(PendingToolApproval {
                session_id,
                tool: pending,
                source: ApprovalSource::Manual { local_only: sensitive },
            });
            self.persist_storage(cx);
            cx.notify();
        } else {
            self.start_tool_run(
                &session_id,
                vec![pending],
                None,
                false,
                crate::audit::Decision::Auto,
                cx,
            );
        }
        true
    }

    fn prepare_user_turn(&mut self, user_prompt: &str, quote: Option<String>) {
        let active_id = self.storage.active_session_id.clone();
        let attachments = std::mem::take(&mut self.pending_attachments);
        let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) else {
            return;
        };
        if session.messages.is_empty() && session.title_auto {
            let title_source = if user_prompt.trim().is_empty() {
                if let Some(att) = attachments.first() {
                    format!("[{}] {}", att.kind.title_label(), att.name)
                } else {
                    DEFAULT_SESSION_TITLE.to_string()
                }
            } else {
                user_prompt.to_string()
            };
            session.title = truncate_title(&title_source);
        }
        let mut message = ChatMessage::new_user(user_prompt.to_string());
        message.quote = quote.filter(|text| !text.trim().is_empty());
        message.attachments = attachments;
        session.messages.push(message);
        session.updated_at = chrono::Local::now().format("%Y-%m-%d %H:%M").to_string();
    }
}

fn truncate_title(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() > 18 {
        format!("{}...", trimmed.chars().take(18).collect::<String>())
    } else {
        trimmed.to_string()
    }
}
