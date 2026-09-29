//! 工具调用协议：内部类型、四渠道的序列化、以及流式参数的拼装。
//!
//! 这一层只做「格式翻译」——把三个渠道各不相同的写法（OpenAI `tool_calls`、
//! Claude `tool_use`、Gemini `functionCall`）统一成 `ToolCall` / `ToolResult`，
//! 反向再翻译回去。**它不知道有哪些工具存在、也不执行任何工具**：
//! 工具清单由 `ChatRequest.tools` 传进来，执行属于 Agent 循环的事。
//!
//! 从 `llm.rs` 拆出来是因为工具协议本身就过千行了，而 `llm.rs` 已经有
//! 请求构建 + 流式读取的职责（§3.3 单文件 800 行上限）。

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::llm::ChatMessageReq;

/// 模型要求调用某个工具的记录。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// 调用 id，**一定不为空、在会话里唯一**：Agent 循环靠它把结果和调用配对。
    /// Claude / OpenAI 会给；Gemini 和部分兼容接口不给，由 [`fresh_call_id`] 补一个
    /// （Gemini 回传时按函数名对应，用不到这个 id）。
    #[serde(default)]
    pub id: String,
    pub name: String,
    /// 参数已经是解析好的 JSON。模型流式吐参数时是一段段字符串，收完整了才解析；
    /// 解析失败就退化成 `{"_raw": "原字符串"}`，不丢内容也不 panic。
    pub arguments: Value,
}

/// 工具执行结果，回传时按渠道转成对应格式。
///
/// 带 `Default` 是为了让构造点只写关心的字段（测试、以及只想传正文的转换路径），
/// 将来再加执行细节字段时不必回来改每一处。
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ToolResult {
    /// 对应 `ToolCall::id`；Gemini 用不到，回传时忽略。
    #[serde(default)]
    pub id: String,
    pub name: String,
    pub content: String,
    #[serde(default)]
    pub is_error: bool,
    /// 执行花了多久（毫秒）。**只给界面看，不掺进 `content`**——
    /// content 是要发给模型的内容，混进耗时和退出码既占上下文，
    /// 又会让同一段历史在不同时刻序列化出不同结果。
    #[serde(default)]
    pub duration_ms: u64,
    /// 子进程的退出码。只有 `run_command` / `git_status` 有；
    /// 文件操作不是进程，是 `None`。
    #[serde(default)]
    pub exit_code: Option<i32>,
}

/// 一个可供模型调用的工具。`parameters` 是 JSON Schema。
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

impl ToolSpec {
    pub fn new(name: impl Into<String>, description: impl Into<String>, parameters: Value) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            parameters,
        }
    }

    /// 没有参数的工具。三个渠道都要求 `parameters` 存在，所以给一个空对象 schema。
    pub fn no_args(name: impl Into<String>, description: impl Into<String>) -> Self {
        Self::new(name, description, json!({"type": "object", "properties": {}}))
    }
}

/// 流式工具调用的中间状态。
///
/// 模型吐工具参数时是一段段 JSON 字符串碎片（OpenAI 的 `tool_calls[].function.arguments`、
/// Claude 的 `input_json_delta.partial_json`），碎到可能连一个完整的键都没有。
/// 所以按「调用的下标」攒着，等这一轮收完（`message_delta` / `[DONE]` / 流结束）
/// 再一次性解析。
#[derive(Default)]
pub(crate) struct ToolCallState {
    /// 按出现顺序累积的调用。下标就是 OpenAI / Claude 给的 `index`。
    pending: Vec<PendingToolCall>,
}

/// 一条正在攒的调用。字段对 `llm` 开放——流式解析要按渠道往里塞碎片。
pub(crate) struct PendingToolCall {
    pub(crate) id: String,
    pub(crate) name: String,
    /// 已收到的 JSON 字符串碎片
    pub(crate) raw: String,
}

impl ToolCallState {
    /// 按下标取（或新建）一条待收的调用。
    pub(crate) fn entry(&mut self, index: usize) -> &mut PendingToolCall {
        while self.pending.len() <= index {
            self.pending.push(PendingToolCall {
                id: String::new(),
                name: String::new(),
                raw: String::new(),
            });
        }
        &mut self.pending[index]
    }

    /// 收齐后统一解析并交给 `emit`，然后清空。重复调用是安全的（空的时候什么都不做）。
    ///
    /// 包个闭包而不是直接收 `tx`，是为了这个模块不用依赖 `llm::StreamEvent`——
    /// 引用方向保持单向（`llm` → `llm_tools`）。
    pub(crate) fn flush(&mut self, mut emit: impl FnMut(ToolCall)) {
        for call in self.pending.drain(..) {
            if call.name.is_empty() {
                continue;
            }
            emit(ToolCall {
                id: if call.id.is_empty() { fresh_call_id() } else { call.id },
                name: call.name,
                arguments: parse_arguments(&call.raw),
            });
        }
    }
}

/// 给没带 id 的调用补一个。
///
/// 不能留空、也不能拿函数名凑数：Agent 循环按 id 判断「这个调用回过结果没有」，
/// 同一个函数被调第二次时，id 重复就会被当成已经回过，循环直接停住。
pub(crate) fn fresh_call_id() -> String {
    format!("call_{}", uuid::Uuid::new_v4().simple())
}

/// 把攒起来的参数字符串解析成 JSON。
///
/// 解析失败**不算错误**：把原文塞进 `_raw` 里。这样模型下一轮能看到自己发过什么，
/// 还有机会自我纠正；直接丢掉的话只会看到模型反复重发同一个调用，更难查。
pub(crate) fn parse_arguments(raw: &str) -> Value {
    if raw.trim().is_empty() {
        return json!({});
    }
    serde_json::from_str(raw).unwrap_or_else(|_| json!({"_raw": raw}))
}

/// 把 `ToolCall::arguments` 编回字符串。流式期间模型吐的是字符串，收完解析成
/// JSON 值，回传时再编回去——中间多一次解码/编码，换来上层永远拿到结构化数据。
pub(crate) fn arguments_text(arguments: &Value) -> String {
    match arguments {
        Value::String(raw) => raw.clone(),
        other => other.to_string(),
    }
}

/// OpenAI 系（chat/completions 与 responses 共用声明格式）的工具声明。
pub(crate) fn openai_tools(tools: &[ToolSpec]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.name,
                    "description": tool.description,
                    "parameters": tool.parameters,
                }
            })
        })
        .collect()
}

/// OpenAI Responses 的工具声明：函数字段直接放在工具对象上。
pub(crate) fn openai_responses_tools(tools: &[ToolSpec]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "type": "function",
                "name": tool.name,
                "description": tool.description,
                "parameters": tool.parameters,
            })
        })
        .collect()
}

/// Claude 的工具声明用 `input_schema`，字段名和 OpenAI 不同。
pub(crate) fn claude_tools(tools: &[ToolSpec]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                "input_schema": tool.parameters,
            })
        })
        .collect()
}

/// Gemini 的工具声明：外层再包一层 `functionDeclarations`。
pub(crate) fn gemini_tools(tools: &[ToolSpec]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool.name,
                "description": tool.description,
                // 只有这一条路径要裁 schema：OpenAI 和 Claude 能读完整 JSON Schema，
                // 为了迁就 Gemini 把表达力削掉，对另外两个渠道是净损失。
                "parameters": schema_for_gemini(&tool.parameters),
            })
        })
        .collect()
}

/// Gemini 的 `functionDeclarations.parameters` 认识的键。
///
/// 取自 Gemini API 的 `Schema` 消息（OpenAPI 3.0 Schema 的一个子集）。
/// `type` / `properties` / `items` / `anyOf` 也在这个集合里，但它们要递归处理，
/// 所以在 [`schema_for_gemini`] 里单独分支，不列进来。
const GEMINI_SCHEMA_KEYS: [&str; 18] = [
    "format",
    "title",
    "description",
    "nullable",
    "default",
    "minItems",
    "maxItems",
    "enum",
    "required",
    "minProperties",
    "maxProperties",
    "minimum",
    "maximum",
    "minLength",
    "maxLength",
    "pattern",
    "example",
    "propertyOrdering",
];

/// 把一份 JSON Schema 裁成 Gemini 认的那部分。
///
/// 用**白名单**而不是黑名单：JSON Schema 的关键字有几十个，黑名单永远补不完，
/// 漏掉一个就是一个 400，而报错只说 "invalid schema"，根本定位不到是哪个键。
/// MCP 服务器给的 schema 尤其杂——TypeScript 那套 `zod → JSON Schema` 出来的
/// 东西必带 `$schema` 和 `additionalProperties`，这两个都会让 Gemini 直接拒收。
///
/// 代价是丢掉了一些约束（比如 `additionalProperties: false` 表达的"不许传额外字段"），
/// 但模型多传一个字段，服务器一般会忽略，比整个请求失败好。
pub(crate) fn schema_for_gemini(schema: &Value) -> Value {
    let Value::Object(map) = schema else {
        // 不是对象就不是合法 schema。退回"任意对象"，至少别让请求挂掉。
        return json!({"type": "object"});
    };
    let mut out = serde_json::Map::new();
    for (key, value) in map {
        match key.as_str() {
            // `properties` 的**键**是用户自己的字段名（完全可能就叫 `$schema`），
            // 只有值是 schema。所以这一层不能整棵子树按关键字过滤，得逐个子树递归。
            "properties" => {
                let Value::Object(props) = value else { continue };
                let cleaned: serde_json::Map<String, Value> = props
                    .iter()
                    .map(|(name, sub)| (name.clone(), schema_for_gemini(sub)))
                    .collect();
                out.insert(key.clone(), Value::Object(cleaned));
            }
            "items" => {
                out.insert(key.clone(), schema_for_gemini(value));
            }
            "anyOf" => {
                let Value::Array(branches) = value else { continue };
                out.insert(
                    key.clone(),
                    Value::Array(branches.iter().map(schema_for_gemini).collect()),
                );
            }
            "type" => {
                if let Some((kind, nullable)) = gemini_type(value) {
                    out.insert("type".to_string(), Value::String(kind));
                    if nullable {
                        out.insert("nullable".to_string(), Value::Bool(true));
                    }
                }
            }
            _ if GEMINI_SCHEMA_KEYS.contains(&key.as_str()) => {
                out.insert(key.clone(), value.clone());
            }
            _ => {}
        }
    }
    // 有 `properties` 却没写 `type` 的 schema 很常见（手写的时候容易漏）。
    // 对象的结构靠 `properties` 才认得出来，补上比让 Gemini 猜好。
    if out.contains_key("properties") && !out.contains_key("type") {
        out.insert("type".to_string(), Value::String("object".to_string()));
    }
    Value::Object(out)
}

/// Gemini 的 `type` 是单个枚举值，而 JSON Schema 允许写成数组——
/// `zod` 生成的 `.nullable()` 字段经常长成 `["string", "null"]`。
///
/// 数组时取第一个非 `null` 的类型，同时把 `nullable` 置上；多种非空类型
/// （`["string", "number"]`）只留第一种，其余信息会丢——Gemini 表达不了。
fn gemini_type(value: &Value) -> Option<(String, bool)> {
    match value {
        Value::String(kind) => Some((kind.clone(), false)),
        Value::Array(kinds) => {
            let nullable = kinds.iter().any(|kind| kind.as_str() == Some("null"));
            let first = kinds
                .iter()
                .filter_map(|kind| kind.as_str())
                .find(|kind| *kind != "null")?;
            Some((first.to_string(), nullable))
        }
        _ => None,
    }
}

pub(crate) fn openai_tool_calls(msg: &ChatMessageReq) -> Vec<Value> {
    msg.tool_calls
        .iter()
        .map(|call| {
            json!({
                "id": call.id,
                "type": "function",
                "function": {
                    "name": call.name,
                    "arguments": arguments_text(&call.arguments),
                }
            })
        })
        .collect()
}

/// 一条消息是否需要走 `claude_message` 的特殊转换。
pub(crate) fn needs_claude_special_case(msg: &ChatMessageReq) -> bool {
    msg.role == "tool" || !msg.tool_calls.is_empty()
}

/// Claude 要求带工具调用的助手消息必须以 `tool_use` 内容块表达，
/// 回传的 `tool_result` 也必须是 user 消息里的一个块，不能像 OpenAI 那样
/// 用独立的 `tool` 角色。这里统一做转换，调用方照旧只准备普通消息。
pub(crate) fn claude_message(msg: &ChatMessageReq) -> Value {
    if msg.role == "tool" {
        return json!({
            "role": "user",
            "content": [{
                "type": "tool_result",
                "tool_use_id": msg.tool_call_id,
                "content": msg.content,
            }]
        });
    }
    let mut blocks: Vec<Value> = Vec::new();
    if !msg.content.trim().is_empty() {
        blocks.push(json!({"type": "text", "text": msg.content}));
    }
    for call in &msg.tool_calls {
        blocks.push(json!({
            "type": "tool_use",
            "id": call.id,
            "name": call.name,
            "input": call.arguments,
        }));
    }
    json!({"role": msg.role, "content": blocks})
}

/// Gemini 的工具结果：是个 user 消息里的 `functionResponse` part。
/// Gemini 不认调用 id，只认函数名，所以用 `tool_name` 而不是 `tool_call_id`。
///
/// 结果必须放在 `response` 对象里——`functionResponse` 没有 `content` 这个字段，
/// 写成 `content` 会被接口以"未知字段"拒绝，循环第二轮就断了。
pub(crate) fn gemini_tool_response(msg: &ChatMessageReq) -> Value {
    json!({
        "role": "user",
        "parts": [{
            "functionResponse": {
                "name": msg.tool_name,
                "response": { "result": msg.content },
            }
        }]
    })
}

/// Gemini 的模型侧工具调用：`parts[].functionCall`。
pub(crate) fn gemini_function_call_parts(msg: &ChatMessageReq) -> Vec<Value> {
    msg.tool_calls
        .iter()
        .map(|call| {
            json!({
                "functionCall": {
                    "name": call.name,
                    "args": call.arguments,
                }
            })
        })
        .collect()
}

/// 工具调用在消息上显示成的短标签（`读取文件(path=a.rs)` 这种）。
///
/// 参数可能是任意 JSON，直接 `to_string()` 出来会很丑，所以压成一行键值对。
pub(crate) fn tool_call_label(call: &ToolCall) -> String {
    let args = match &call.arguments {
        // 空对象与 null 都当成"没有参数"。这里必须显式匹配空对象——
        // 落进下面的 `other` 分支会渲染成字面量 `{}`，很难看。
        Value::Object(map) if map.is_empty() => String::new(),
        Value::Object(map) => {
            let pairs: Vec<String> = map
                .iter()
                .map(|(key, value)| {
                    let rendered = match value {
                        Value::String(text) => text.clone(),
                        other => other.to_string(),
                    };
                    format!("{key}={rendered}")
                })
                .collect();
            pairs.join(", ")
        }
        Value::Null => String::new(),
        other => other.to_string(),
    };
    if args.is_empty() {
        call.name.clone()
    } else {
        format!("{}({args})", call.name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_tool_spec_has_a_valid_empty_schema() {
        let spec = ToolSpec::no_args("list_sessions", "列出会话");
        assert_eq!(spec.parameters["type"], "object");
        assert!(spec.parameters["properties"].as_object().unwrap().is_empty());
    }

    #[test]
    fn broken_json_is_preserved_under_raw() {
        let parsed = parse_arguments("{\"path\":");
        assert_eq!(parsed["_raw"], "{\"path\":");
        // 空串是合法情况（无参数工具），不该变成 `_raw`
        assert_eq!(parse_arguments("  "), json!({}));
    }

    #[test]
    fn arguments_round_trip_as_strings() {
        assert_eq!(arguments_text(&json!({"path": "a.rs"})), "{\"path\":\"a.rs\"}");
        // 已经是字符串的（模型中途就发字符串）原样保留，别多套一层引号
        assert_eq!(arguments_text(&json!("{\"a\":1}")), "{\"a\":1}");
    }

    #[test]
    fn flush_skips_calls_without_a_name() {
        let mut state = ToolCallState::default();
        state.entry(0); // 只有下标、没有名字——收了半截就断了
        state.entry(1).name = "read_file".into();
        state.entry(1).raw = "{\"path\":\"a.rs\"}".into();
        let mut collected = Vec::new();
        state.flush(|call| collected.push(call));
        assert_eq!(collected.len(), 1, "没有函数名的半截调用不该发出去");
        assert_eq!(collected[0].name, "read_file");
        // flush 之后状态清空，重复调用不会重复发
        let mut again = Vec::new();
        state.flush(|call| again.push(call));
        assert!(again.is_empty());
    }

    #[test]
    fn tool_call_label_is_compact() {
        let call = ToolCall {
            id: "c1".into(),
            name: "read_file".into(),
            arguments: json!({"path": "a.rs", "line": 3}),
        };
        let label = tool_call_label(&call);
        assert!(label.starts_with("read_file("), "得到 {label}");
        assert!(label.contains("path=a.rs"));
        assert!(!label.contains('\n'));

        let no_args = ToolCall {
            id: "c2".into(),
            name: "list_sessions".into(),
            arguments: json!({}),
        };
        assert_eq!(tool_call_label(&no_args), "list_sessions");
    }

    #[test]
    fn gemini_schema_drops_keys_it_does_not_know() {
        // 这是 MCP 服务器（zod 那套）最典型的输出，两个多余键都会让 Gemini 400
        let schema = json!({
            "$schema": "http://json-schema.org/draft-07/schema#",
            "type": "object",
            "additionalProperties": false,
            "properties": { "path": { "type": "string", "description": "路径" } },
            "required": ["path"]
        });
        let cleaned = schema_for_gemini(&schema);
        assert!(cleaned.get("$schema").is_none(), "得到 {cleaned}");
        assert!(cleaned.get("additionalProperties").is_none(), "得到 {cleaned}");
        assert_eq!(cleaned["type"], "object");
        assert_eq!(cleaned["properties"]["path"]["type"], "string");
        assert_eq!(cleaned["properties"]["path"]["description"], "路径");
        assert_eq!(cleaned["required"][0], "path");
    }

    #[test]
    fn gemini_schema_keeps_property_names_even_if_they_look_like_keywords() {
        // 字段名是用户自己的，可能就叫 `$schema`——只过滤关键字，不能连名字一起动
        let schema = json!({
            "type": "object",
            "properties": { "$schema": { "type": "string" } }
        });
        let cleaned = schema_for_gemini(&schema);
        assert_eq!(cleaned["properties"]["$schema"]["type"], "string");
    }

    #[test]
    fn gemini_schema_flattens_nullable_type_arrays() {
        let schema = json!({
            "type": "object",
            "properties": {
                "path": { "type": ["string", "null"] },
                "count": { "type": ["null", "integer"] }
            }
        });
        let cleaned = schema_for_gemini(&schema);
        assert_eq!(cleaned["properties"]["path"]["type"], "string");
        assert_eq!(cleaned["properties"]["path"]["nullable"], true);
        // 顺序反过来也要挑到那个非 null 的
        assert_eq!(cleaned["properties"]["count"]["type"], "integer");
    }

    #[test]
    fn gemini_schema_recurses_into_items_and_any_of() {
        let schema = json!({
            "type": "array",
            "items": { "type": "object", "additionalProperties": true, "properties": {} },
            "anyOf": [{ "type": "string", "$schema": "x" }]
        });
        let cleaned = schema_for_gemini(&schema);
        assert!(cleaned["items"].get("additionalProperties").is_none());
        assert!(cleaned["anyOf"][0].get("$schema").is_none());
        assert_eq!(cleaned["anyOf"][0]["type"], "string");
    }

    #[test]
    fn gemini_schema_adds_a_missing_object_type() {
        // 只写了 properties、忘了 type 的 schema 很常见，补上对象类型
        let cleaned = schema_for_gemini(&json!({"properties": {"a": {"type": "string"}}}));
        assert_eq!(cleaned["type"], "object");
    }

    #[test]
    fn a_non_object_schema_becomes_an_empty_object_schema() {
        // 服务器给了个乱七八糟的东西时，宁可退化成"任意对象"，也别让整个请求挂掉
        assert_eq!(schema_for_gemini(&json!("nope")), json!({"type": "object"}));
    }

    #[test]
    fn only_gemini_gets_the_trimmed_schema() {
        let spec = ToolSpec::new(
            "mcp__srv__read",
            "读点什么",
            json!({"type": "object", "$schema": "x", "properties": {}}),
        );
        // OpenAI / Claude 拿到的还是完整 schema，只有 Gemini 那份被裁过
        assert!(
            openai_tools(std::slice::from_ref(&spec))[0]["function"]["parameters"]
                .get("$schema")
                .is_some()
        );
        assert!(
            claude_tools(std::slice::from_ref(&spec))[0]["input_schema"]
                .get("$schema")
                .is_some()
        );
        assert!(
            gemini_tools(std::slice::from_ref(&spec))[0]["parameters"]
                .get("$schema")
                .is_none()
        );
    }
}
