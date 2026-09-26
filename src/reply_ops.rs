//! 发起回复与接收流式输出：组装请求、派发任务、把流事件写回消息，以及自动生成标题。

use gpui_kit::*;
use tokio::sync::oneshot;

use crate::app::{AppState, ToastLevel, runtime, update_state};
use crate::config::{ChannelType, ModelConfig, ProviderConfig};
use crate::i18n::{AppLanguage, Key, tr};
use crate::llm::{ChatMessageReq, ChatRequest, StreamEvent, stream_chat};
use crate::llm_tools::tool_call_label;
use crate::model::{ChatMessage, MessageVariant, ReasoningLevel, ResolvedParams};

/// 一次流式请求：会话里对应哪条消息、哪个版本，以及组装好的请求体。
pub(crate) struct Job {
    key: String,
    message_id: String,
    variant_id: Option<String>,
    request: ChatRequest,
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
        let history = self.history_messages();
        let Some(job) = self.make_job(&message_id, None, &provider_id, &model, history) else {
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
        let history = self.history_messages();
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
            if let Some(job) = self.make_job("pending", Some(&variant.id), provider_id, model, history.clone()) {
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

    pub(crate) fn history_messages(&self) -> Vec<ChatMessageReq> {
        let Some(session) = self.storage.get_active_session() else {
            return Vec::new();
        };
        let resolved = session.resolved_params(&self.config.system_prompt, self.config.temperature);
        let mut messages = vec![ChatMessageReq::new("system", resolved.system_prompt)];
        messages.extend(
            session
                .api_turns(resolved.context_limit)
                .into_iter()
                .map(|(role, content, attachments)| ChatMessageReq::with_attachments(role, content, attachments)),
        );
        messages
    }

    /// 找到要调用的渠道和模型。模型被停用或删除时退回同渠道第一个可用模型。
    fn resolve_model(&self, provider_id: &str, model_id: &str) -> Option<(&ProviderConfig, &ModelConfig)> {
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

    pub(crate) fn make_job(
        &self,
        message_id: &str,
        variant_id: Option<&str>,
        provider_id: &str,
        model_id: &str,
        messages: Vec<ChatMessageReq>,
    ) -> Option<Job> {
        let (provider, model) = self.resolve_model(provider_id, model_id)?;
        let session = self.storage.get_active_session()?;
        let resolved = session.resolved_params(&self.config.system_prompt, self.config.temperature);
        let explicit_temperature = session
            .params
            .as_ref()
            .is_some_and(|params| params.temperature.is_some());
        Some(Job {
            key: stream_key(message_id, variant_id),
            message_id: message_id.to_string(),
            variant_id: variant_id.map(str::to_string),
            request: chat_request(
                provider,
                model,
                messages,
                &resolved,
                explicit_temperature,
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
                self.persist_storage(cx);
                self.maybe_autotitle(cx);
            }
        }
        cx.notify();
    }

    fn maybe_autotitle(&mut self, cx: &mut Context<Self>) {
        let Some(session) = self.storage.get_active_session() else {
            return;
        };
        if !session.title_auto || session.messages.len() < 2 {
            return;
        }
        let session_id = session.id.clone();
        let excerpt = session
            .messages
            .iter()
            .find(|message| message.role == "user")
            .map(|message| message.content.chars().take(400).collect::<String>())
            .unwrap_or_default();
        if excerpt.is_empty() {
            return;
        }
        if let Some(session) = self
            .storage
            .sessions
            .iter_mut()
            .find(|session| session.id == session_id)
        {
            session.title_auto = false;
        }
        let provider_id = self
            .storage
            .get_active_session()
            .map(|s| s.provider_id.clone())
            .unwrap_or_default();
        let model_id = self
            .storage
            .get_active_session()
            .map(|s| s.model.clone())
            .unwrap_or_default();
        let Some(job) = self.make_job(
            "title",
            None,
            &provider_id,
            &model_id,
            vec![
                ChatMessageReq::new("system", "用不超过16个字给对话起标题，只输出标题本身，不要标点包裹。"),
                ChatMessageReq::new("user", excerpt),
            ],
        ) else {
            return;
        };
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

fn chat_request(
    provider: &ProviderConfig,
    model: &ModelConfig,
    messages: Vec<ChatMessageReq>,
    params: &ResolvedParams,
    explicit_temperature: bool,
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
        // P3-1 只打通协议层：界面还没地方让用户挂工具，所以这里先固定为空。
        // 请求体里因此不会出现 tools 字段，线上行为与改动前一致。
        tools: Vec::new(),
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
        // 消息上存的是给人看的短标签（`Vec<String>`，界面上当 chip 渲染），
        // 不是完整的调用记录。完整记录留给 P3-2 的 Agent 循环去存。
        StreamEvent::ToolCall(call) => message.tool_calls.push(tool_call_label(call)),
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
            AppLanguage::ZhCn,
        );
        assert_eq!(request.reasoning, None);
    }
}
