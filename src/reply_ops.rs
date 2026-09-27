//! 发起回复与接收流式输出：组装请求、派发任务、把流事件写回消息，以及自动生成标题。

use gpui_kit::*;
use tokio::sync::oneshot;

use crate::agent_loop::AgentOrigin;
use crate::app::{AppState, ToastLevel, runtime, update_state};
use crate::config::{ChannelType, ModelConfig, ProviderConfig};
use crate::i18n::{AppLanguage, Key, tr};
use crate::llm::{ChatMessageReq, ChatRequest, StreamEvent, stream_chat};
use crate::llm_tools::{ToolSpec, tool_call_label};
use crate::model::{ChatMessage, MessageVariant, ReasoningLevel, ResolvedParams};
use crate::model_info::Capability;

/// 一次流式请求：会话里对应哪条消息、哪个版本，以及组装好的请求体。
pub(crate) struct Job {
    key: String,
    session_id: String,
    message_id: String,
    variant_id: Option<String>,
    /// 这一轮是不是 Agent 循环的一环（带了工具清单）。结束时靠它决定要不要续跑
    agent: bool,
    request: ChatRequest,
}

/// 组装一次请求需要知道的东西。
pub(crate) struct JobSpec<'a> {
    /// 请求替哪个对话发。不能默认用「当前打开的对话」：循环续跑时用户可能已经切走了
    pub session_id: &'a str,
    pub message_id: &'a str,
    pub variant_id: Option<&'a str>,
    pub provider_id: &'a str,
    pub model_id: &'a str,
    /// 要不要带工具清单。只有正常的一问一答（以及它的续跑）带：
    /// 多模型对比、起标题、「继续生成」都不需要工具，带上了模型反而可能只回一个调用
    pub with_tools: bool,
}

impl AppState {
    pub(crate) fn start_reply(&mut self, override_model: Option<(String, String)>, cx: &mut Context<Self>) {
        let active_id = self.storage.active_session_id.clone();
        let (default_provider, default_model) = self.config.default_model_selection();
        let (provider_id, model) = override_model.unwrap_or_else(|| {
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
        });
        if self.config.providers.is_empty() {
            self.push_local_assistant(tr(self.language(), Key::ErrNoChannelCreated));
            self.persist_storage(cx);
            cx.notify();
            return;
        }
        let mut assistant = ChatMessage::new_assistant();
        assistant.model = model.clone();
        let message_id = assistant.id.clone();
        if let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) {
            session.provider_id = provider_id.clone();
            session.model = model.clone();
            session.messages.push(assistant);
        }
        // 新的一问：轮数上限从头数
        self.agent.round = 0;
        let history = self.history_messages(&active_id);
        let spec = JobSpec {
            session_id: &active_id,
            message_id: &message_id,
            variant_id: None,
            provider_id: &provider_id,
            model_id: &model,
            with_tools: true,
        };
        let Some(job) = self.make_job(spec, history) else {
            // 先把文案取出来：下面要可变借用 self 去改消息，不能同时再读 self
            let error = tr(self.language(), Key::ErrNoModelInChannel).to_string();
            if let Some(message) = self.find_message_mut(&message_id) {
                message.is_streaming = false;
                message.error = Some(error);
            }
            self.persist_storage(cx);
            cx.notify();
            return;
        };
        self.scroll_to_end_pending = true;
        self.spawn_jobs(vec![job], cx);
    }

    pub(crate) fn start_compare(&mut self, targets: &[(String, String)], cx: &mut Context<Self>) {
        let active_id = self.storage.active_session_id.clone();
        let history = self.history_messages(&active_id);
        let mut variants = Vec::new();
        let mut jobs = Vec::new();
        for (provider_id, model) in targets {
            let variant = MessageVariant {
                id: uuid::Uuid::new_v4().to_string(),
                provider_id: provider_id.clone(),
                model: model.clone(),
                content: String::new(),
                reasoning_content: None,
                error: None,
                is_streaming: true,
                prompt_tokens: 0,
                completion_tokens: 0,
                speed_tps: 0.0,
                latency_ms: 0,
            };
            let spec = JobSpec {
                session_id: &active_id,
                message_id: "pending",
                variant_id: Some(&variant.id),
                provider_id,
                model_id: model,
                // 对比看的是各家的回答本身；版本里也没地方记工具调用
                with_tools: false,
            };
            if let Some(job) = self.make_job(spec, history.clone()) {
                variants.push(variant);
                jobs.push(job);
            }
        }
        if jobs.len() < 2 {
            self.toast(ToastLevel::Error, tr(self.language(), Key::ErrNotEnoughModels));
            cx.notify();
            return;
        }
        let mut assistant = ChatMessage::new_assistant();
        assistant.model = jobs[0].request.model.clone();
        let message_id = assistant.id.clone();
        for job in &mut jobs {
            job.message_id = message_id.clone();
            job.key = stream_key(&message_id, job.variant_id.as_deref());
        }
        assistant.variants = variants;
        if let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) {
            session.messages.push(assistant);
        }
        self.scroll_to_end_pending = true;
        self.spawn_jobs(jobs, cx);
    }

    pub(crate) fn history_messages(&self, session_id: &str) -> Vec<ChatMessageReq> {
        let Some(session) = self.session(session_id) else {
            return Vec::new();
        };
        let resolved = session.resolved_params(&self.config.system_prompt, self.config.temperature);
        let mut messages = vec![ChatMessageReq::new("system", resolved.system_prompt)];
        messages.extend(session.api_turns(resolved.context_limit));
        messages
    }

    /// 找到要调用的渠道和模型。模型被停用或删除时退回同渠道第一个可用模型。
    pub(crate) fn resolve_model(&self, provider_id: &str, model_id: &str) -> Option<(&ProviderConfig, &ModelConfig)> {
        let provider = self
            .config
            .providers
            .iter()
            .find(|provider| provider.id == provider_id)
            .or_else(|| self.config.get_active_provider())?;
        let model = provider
            .models
            .iter()
            .find(|model| model.id == model_id && model.enabled)
            .or_else(|| provider.models.iter().find(|model| model.enabled))?;
        Some((provider, model))
    }

    pub(crate) fn make_job(&self, spec: JobSpec, messages: Vec<ChatMessageReq>) -> Option<Job> {
        let (provider, model) = self.resolve_model(spec.provider_id, spec.model_id)?;
        let session = self.session(spec.session_id)?;
        let resolved = session.resolved_params(&self.config.system_prompt, self.config.temperature);
        let explicit_temperature = session
            .params
            .as_ref()
            .is_some_and(|params| params.temperature.is_some());
        let with_tools = spec.with_tools && self.config.local_tools_enabled;
        Some(Job {
            key: stream_key(spec.message_id, spec.variant_id),
            session_id: spec.session_id.to_string(),
            message_id: spec.message_id.to_string(),
            variant_id: spec.variant_id.map(str::to_string),
            agent: with_tools,
            request: chat_request(
                provider,
                model,
                messages,
                &resolved,
                explicit_temperature,
                tool_list_for(model, with_tools, &self.mcp.specs(&self.config.mcp_servers)),
                self.language(),
            ),
        })
    }

    pub(crate) fn spawn_jobs(&mut self, jobs: Vec<Job>, cx: &mut Context<Self>) {
        if jobs.is_empty() {
            return;
        }
        self.is_streaming = true;
        for job in jobs {
            let (cancel_tx, cancel_rx) = oneshot::channel();
            self.active_streams.insert(job.key.clone(), cancel_tx);
            let key = job.key.clone();
            let message_id = job.message_id.clone();
            let variant_id = job.variant_id.clone();
            // 记下这一轮是替谁发的：它结束时如果模型要求了工具调用，循环要靠这些接着跑
            if job.agent {
                self.agent.origin = Some(AgentOrigin {
                    session_id: job.session_id.clone(),
                    message_id: message_id.clone(),
                    provider_id: job.request.provider_id.clone(),
                    model: job.request.model.clone(),
                });
            }
            let request = job.request;
            cx.spawn(async move |this, cx| {
                let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
                runtime().spawn(async move {
                    stream_chat(request, tx, Some(cancel_rx)).await;
                });
                while let Some(event) = rx.recv().await {
                    update_state(&this, cx, |state, cx| {
                        state.apply_stream_event(&key, &message_id, variant_id.as_deref(), event, cx)
                    });
                }
                update_state(&this, cx, |state, cx| {
                    state.apply_stream_event(&key, &message_id, variant_id.as_deref(), StreamEvent::Done, cx)
                });
            })
            .detach();
        }
        self.persist_storage(cx);
        cx.notify();
    }

    fn apply_stream_event(
        &mut self,
        key: &str,
        message_id: &str,
        variant_id: Option<&str>,
        event: StreamEvent,
        cx: &mut Context<Self>,
    ) {
        if !self.active_streams.contains_key(key) {
            return;
        }
        let finished = matches!(event, StreamEvent::Done | StreamEvent::Error(_));
        if let Some(message) = self.find_message_mut(message_id) {
            if let Some(variant_id) = variant_id {
                if let Some(variant) = message.variants.iter_mut().find(|variant| variant.id == variant_id) {
                    apply_to_variant(variant, &event);
                }
                message.is_streaming = message.variants.iter().any(|variant| variant.is_streaming);
            } else {
                apply_to_message(message, &event);
            }
        }
        if finished {
            self.active_streams.remove(key);
            self.is_streaming = !self.active_streams.is_empty();
            if !self.is_streaming {
                // 先存盘再问循环要不要续跑：续跑会往会话里再插消息
                self.persist_storage(cx);
                if self.advance_agent_loop(message_id, cx) {
                    return;
                }
                // 按消息找对话，而不是用当前打开的对话：生成期间用户可能切走了
                if let Some(session_id) = self.session_of_message(message_id) {
                    self.maybe_autotitle(&session_id, cx);
                }
            }
        }
        cx.notify();
    }

    /// 这条消息在哪个对话里
    pub(crate) fn session_of_message(&self, message_id: &str) -> Option<String> {
        self.storage
            .sessions
            .iter()
            .find(|session| session.messages.iter().any(|message| message.id == message_id))
            .map(|session| session.id.clone())
    }

    pub(crate) fn maybe_autotitle(&mut self, session_id: &str, cx: &mut Context<Self>) {
        let Some(session) = self.session(session_id) else {
            return;
        };
        if !session.title_auto || session.messages.len() < 2 {
            return;
        }
        let excerpt = session
            .messages
            .iter()
            .find(|message| message.role == "user" && !message.local_only)
            .map(|message| message.content.chars().take(400).collect::<String>())
            .unwrap_or_default();
        if excerpt.is_empty() {
            return;
        }
        let provider_id = session.provider_id.clone();
        let model_id = session.model.clone();
        if let Some(session) = self
            .storage
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.title_auto = false;
        }
        let spec = JobSpec {
            session_id,
            message_id: "title",
            variant_id: None,
            provider_id: &provider_id,
            model_id: &model_id,
            with_tools: false,
        };
        let Some(job) = self.make_job(
            spec,
            vec![
                ChatMessageReq::new("system", "用不超过16个字给对话起标题，只输出标题本身，不要标点包裹。"),
                ChatMessageReq::new("user", excerpt),
            ],
        ) else {
            return;
        };
        let session_id = session_id.to_string();
        let mut request = job.request;
        request.stream = false;
        // 起标题用最弱的思考强度；还要思考的模型不限制输出长度，否则思考就把额度用完了
        let (levels, thinks) = self
            .resolve_model(&provider_id, &model_id)
            .map(|(_, model)| (model.effective_reasoning_levels(), model.thinks()))
            .unwrap_or_default();
        request.reasoning = levels.first().copied();
        let quiet = !thinks || request.reasoning == Some(ReasoningLevel::Off);
        request.max_tokens = if quiet { Some(32) } else { None };
        if request.temperature.is_some() {
            request.temperature = Some(0.2);
        }
        cx.spawn(async move |this, cx| {
            let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
            runtime().spawn(async move {
                stream_chat(request, tx, None).await;
            });
            let mut title = String::new();
            while let Some(event) = rx.recv().await {
                if let StreamEvent::Content(text) = event {
                    title.push_str(&text);
                }
            }
            let title = title
                .trim()
                .trim_matches('"')
                .trim()
                .chars()
                .take(24)
                .collect::<String>();
            if title.is_empty() {
                return;
            }
            update_state(&this, cx, |state, cx| {
                if let Some(session) = state
                    .storage
                    .sessions
                    .iter_mut()
                    .find(|session| session.id == session_id)
                {
                    session.title = title;
                    session.title_auto = false;
                    state.persist_storage(cx);
                    cx.notify();
                }
            });
        })
        .detach();
    }

    fn push_local_assistant(&mut self, text: &str) {
        let active_id = self.storage.active_session_id.clone();
        if let Some(session) = self.storage.sessions.iter_mut().find(|session| session.id == active_id) {
            let mut message = ChatMessage::new_assistant();
            message.is_streaming = false;
            message.content = text.to_string();
            session.messages.push(message);
        }
    }
}

fn stream_key(message_id: &str, variant_id: Option<&str>) -> String {
    match variant_id {
        Some(variant_id) => format!("{message_id}:{variant_id}"),
        None => message_id.to_string(),
    }
}

/// 这一轮交给模型的工具清单。空表示不带工具。
///
/// 两个条件都满足才带：用户开着「本地工具」总开关，且这个模型确实支持函数调用。
/// 不支持 tools 的模型（比如部分纯推理模型）收到 `tools` 字段可能直接报错，而
/// 「用户开了开关但选了个不支持的模型」是很常见的组合，所以这里按模型的
/// `Capability::Tools` 再挡一道。
///
/// MCP 工具排在本机那 5 个后面，顺序由服务器配置顺序决定——同一份工具集每次
/// 序列化出来要逐字节一致，对端才能命中 prompt 缓存。MCP 工具和本机工具共用
/// 这一个开关，理由见 `mcp_ops.rs` 的模块说明。
fn tool_list_for(model: &ModelConfig, enabled: bool, mcp_tools: &[ToolSpec]) -> Vec<ToolSpec> {
    if !enabled || !model.effective_capabilities().contains(&Capability::Tools) {
        return Vec::new();
    }
    let mut specs = crate::local_tools::specs();
    specs.extend_from_slice(mcp_tools);
    specs
}

/// 组装一次请求。
fn chat_request(
    provider: &ProviderConfig,
    model: &ModelConfig,
    messages: Vec<ChatMessageReq>,
    params: &ResolvedParams,
    explicit_temperature: bool,
    tools: Vec<ToolSpec>,
    lang: AppLanguage,
) -> ChatRequest {
    let levels = model.effective_reasoning_levels();
    // 对话没指定时用模型的默认强度；模型不支持的档位不发送
    let reasoning = params
        .reasoning
        .or(model.default_reasoning)
        .filter(|level| levels.contains(level));
    let max_output = model.effective_max_output();
    // OpenAI 的推理模型只接受默认温度：没有在对话参数里专门设置过就不发送
    let openai = matches!(
        provider.channel_type,
        ChannelType::OpenAiChat | ChannelType::OpenAiResponses
    );
    let omit_sampling = openai && model.thinks() && !explicit_temperature;
    ChatRequest {
        channel_type: provider.channel_type,
        base_url: provider.base_url.clone(),
        api_key: provider.api_key.clone(),
        model: model.id.clone(),
        provider_id: provider.id.clone(),
        messages,
        temperature: (!omit_sampling).then_some(params.temperature),
        top_p: if omit_sampling { None } else { params.top_p },
        max_tokens: params
            .max_tokens
            .map(|value| max_output.map_or(value, |cap| value.min(cap))),
        stream: params.stream,
        reasoning,
        max_output,
        model_thinks: model.thinks(),
        // 只有用户开了本地工具、且这个模型确实支持函数调用时才带上工具清单。
        // 不支持 tools 的模型（比如部分纯推理模型）收到 tools 字段可能直接报错，
        // 而"用户开了开关但选了个不支持的模型"是很常见的组合，
        // 所以这里按模型的 `Capability::Tools` 再挡一道。
        //
        // MCP 工具和本机工具共用这一个开关（见 `mcp_ops.rs` 的模块说明）。
        tools,
        extra_headers: provider
            .extra_headers
            .iter()
            .map(|header| (header.name.clone(), header.value.clone()))
            .collect(),
        proxy: provider.proxy.clone(),
        timeout_secs: if provider.timeout_secs == 0 {
            90
        } else {
            provider.timeout_secs
        },
        retries: provider.retries,
        lang,
    }
}

fn apply_to_message(message: &mut ChatMessage, event: &StreamEvent) {
    match event {
        StreamEvent::Thinking(text) => message.reasoning_content.get_or_insert_with(String::new).push_str(text),
        StreamEvent::Content(text) => message.content.push_str(text),
        // 两份都存：`tool_calls` 是给人看的短标签（界面上当 chip 渲染），
        // `called_tools` 是完整记录（下一轮要拿它把参数原样发回去）。
        // 只存标签的话回传时还原不出参数，模型会看到自己发过一个空调用。
        StreamEvent::ToolCall(call) => {
            message.tool_calls.push(tool_call_label(call));
            message.called_tools.push(call.clone());
        }
        StreamEvent::Metrics {
            tokens_prompt,
            tokens_completion,
            speed_tps,
            latency_ms,
        } => {
            message.prompt_tokens = *tokens_prompt;
            message.completion_tokens = *tokens_completion;
            message.speed_tps = *speed_tps;
            message.latency_ms = *latency_ms;
        }
        StreamEvent::Done => message.is_streaming = false,
        StreamEvent::Error(error) => {
            message.error = Some(error.clone());
            message.is_streaming = false;
        }
    }
}

fn apply_to_variant(variant: &mut MessageVariant, event: &StreamEvent) {
    match event {
        StreamEvent::Thinking(text) => variant.reasoning_content.get_or_insert_with(String::new).push_str(text),
        StreamEvent::Content(text) => variant.content.push_str(text),
        // 变体没有 tool_calls 字段：它记录的是「同一次请求的另一种答案」，
        // 工具调用属于整条消息层面的事，落在 Message 上。
        StreamEvent::ToolCall(_) => {}
        StreamEvent::Metrics {
            tokens_prompt,
            tokens_completion,
            speed_tps,
            latency_ms,
        } => {
            variant.prompt_tokens = *tokens_prompt;
            variant.completion_tokens = *tokens_completion;
            variant.speed_tps = *speed_tps;
            variant.latency_ms = *latency_ms;
        }
        StreamEvent::Done => variant.is_streaming = false,
        StreamEvent::Error(error) => {
            variant.error = Some(error.clone());
            variant.is_streaming = false;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::HeaderPair;

    fn provider(channel_type: ChannelType) -> ProviderConfig {
        ProviderConfig {
            id: "p".into(),
            name: "P".into(),
            channel_type,
            base_url: "https://example.test/v1".into(),
            api_path: String::new(),
            api_key: String::new(),
            api_key_ref: String::new(),
            enabled: true,
            models: Vec::new(),
            timeout_secs: 0,
            retries: 0,
            proxy: String::new(),
            extra_headers: vec![HeaderPair {
                name: "X-A".into(),
                value: "1".into(),
            }],
        }
    }

    fn params(reasoning: Option<ReasoningLevel>, max_tokens: Option<u32>) -> ResolvedParams {
        ResolvedParams {
            system_prompt: String::new(),
            temperature: 0.7,
            top_p: Some(0.9),
            max_tokens,
            context_limit: None,
            reasoning,
            stream: true,
        }
    }

    #[std::prelude::v1::test]
    fn reasoning_models_skip_default_temperature_on_openai() {
        let o3 = ModelConfig::new("o3-mini", "o3-mini");
        let request = chat_request(
            &provider(ChannelType::OpenAiChat),
            &o3,
            Vec::new(),
            &params(None, None),
            false,
            Vec::new(),
            AppLanguage::ZhCn,
        );
        assert_eq!(request.temperature, None);
        assert_eq!(request.top_p, None);
        let request = chat_request(
            &provider(ChannelType::OpenAiChat),
            &o3,
            Vec::new(),
            &params(None, None),
            true,
            Vec::new(),
            AppLanguage::ZhCn,
        );
        assert_eq!(request.temperature, Some(0.7), "explicit temperature is kept");

        let gpt = ModelConfig::new("gpt-4o", "GPT-4o");
        let request = chat_request(
            &provider(ChannelType::OpenAiChat),
            &gpt,
            Vec::new(),
            &params(None, None),
            false,
            Vec::new(),
            AppLanguage::ZhCn,
        );
        assert_eq!(request.temperature, Some(0.7));

        let claude = ModelConfig::new("claude-sonnet-4-5", "Claude");
        let request = chat_request(
            &provider(ChannelType::Claude),
            &claude,
            Vec::new(),
            &params(None, None),
            false,
            Vec::new(),
            AppLanguage::ZhCn,
        );
        assert_eq!(
            request.temperature,
            Some(0.7),
            "Claude still honours the global temperature"
        );
        assert_eq!(request.max_output, Some(64_000));
    }

    #[std::prelude::v1::test]
    fn model_default_reasoning_and_output_limit_apply() {
        let mut model = ModelConfig::new("claude-opus-4-1", "Opus");
        model.default_reasoning = Some(ReasoningLevel::High);
        let request = chat_request(
            &provider(ChannelType::Claude),
            &model,
            Vec::new(),
            &params(None, Some(100_000)),
            false,
            Vec::new(),
            AppLanguage::ZhCn,
        );
        assert_eq!(request.reasoning, Some(ReasoningLevel::High));
        assert_eq!(request.max_tokens, Some(32_000), "clamped to the model's output limit");

        let request = chat_request(
            &provider(ChannelType::Claude),
            &model,
            Vec::new(),
            &params(Some(ReasoningLevel::Off), None),
            false,
            Vec::new(),
            AppLanguage::ZhCn,
        );
        assert_eq!(
            request.reasoning,
            Some(ReasoningLevel::Off),
            "the conversation's choice wins"
        );

        // 模型不支持的档位不发送
        let plain = ModelConfig::new("gpt-4o", "GPT-4o");
        let request = chat_request(
            &provider(ChannelType::OpenAiChat),
            &plain,
            Vec::new(),
            &params(Some(ReasoningLevel::High), None),
            false,
            Vec::new(),
            AppLanguage::ZhCn,
        );
        assert_eq!(request.reasoning, None);
    }

    #[std::prelude::v1::test]
    fn tools_are_only_offered_when_the_switch_is_on() {
        let model = ModelConfig::new("gpt-4o", "GPT-4o");
        let mcp = vec![ToolSpec::no_args("mcp__files__read_file", "读文件")];

        assert!(tool_list_for(&model, false, &mcp).is_empty(), "开关关着就一个都别给");

        let tools = tool_list_for(&model, true, &mcp);
        assert_eq!(tools.len(), crate::local_tools::specs().len() + 1);
        assert_eq!(
            tools.last().map(|tool| tool.name.as_str()),
            Some("mcp__files__read_file"),
            "MCP 工具排在本机工具后面，顺序稳定"
        );
    }
}
