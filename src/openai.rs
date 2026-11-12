//! An OpenAI Chat Completions provider. The agent loop speaks Anthropic Messages, so each
//! request is converted to Chat Completions and each reply is converted back.

use crate::agent::{
    normalize_base, post_json, AgentConfig, Credential, Llm, INVALID_ARGUMENTS,
};
use serde_json::{json, Map, Value};

pub struct OpenAiLlm {
    pub config: AgentConfig,
}

impl OpenAiLlm {
    pub fn new(config: AgentConfig) -> Self {
        Self { config }
    }
}

impl Llm for OpenAiLlm {
    fn complete(&self, request: &Value) -> Result<Value, String> {
        let key = match &self.config.credential {
            Credential::ApiKey(key) | Credential::AuthToken(key) => key.clone(),
            Credential::Login => {
                return Err("an OpenAI-format provider cannot use the Anthropic login".into())
            }
        };
        let headers = [("authorization", format!("Bearer {key}"))];
        let reply = post_json(&chat_url(&self.config.base_url), &headers, &to_chat(request))?;
        Ok(from_chat(&reply))
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    fn max_tokens(&self) -> u32 {
        self.config.max_tokens
    }
}

/// `https://host`, `https://host/v1` and `https://host/v1/chat/completions` all give
/// `https://host/v1/chat/completions`.
pub fn chat_url(base_url: &str) -> String {
    let base = normalize_base(base_url);
    let base = base.strip_suffix("/chat/completions").unwrap_or(&base);
    let base = base.strip_suffix("/v1").unwrap_or(base);
    format!("{base}/v1/chat/completions")
}

/// Convert an Anthropic Messages request to a Chat Completions request.
pub fn to_chat(request: &Value) -> Value {
    let mut messages = Vec::new();
    if let Some(system) = request.get("system").and_then(Value::as_str) {
        messages.push(json!({"role": "system", "content": system}));
    }
    for message in request
        .get("messages")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let role = message.get("role").and_then(Value::as_str).unwrap_or("user");
        match message.get("content") {
            Some(Value::String(text)) => messages.push(json!({"role": role, "content": text})),
            Some(Value::Array(blocks)) if role == "assistant" => {
                messages.push(assistant_message(blocks))
            }
            Some(Value::Array(blocks)) => user_messages(blocks, &mut messages),
            _ => {}
        }
    }

    let mut chat = Map::new();
    chat.insert("model".into(), request.get("model").cloned().unwrap_or(Value::Null));
    if let Some(max) = request.get("max_tokens") {
        chat.insert("max_completion_tokens".into(), max.clone());
    }
    chat.insert("messages".into(), Value::Array(messages));
    let tools: Vec<Value> = request
        .get("tools")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|tool| {
            json!({
                "type": "function",
                "function": {
                    "name": tool.get("name").cloned().unwrap_or(Value::Null),
                    "description": tool.get("description").cloned().unwrap_or(json!("")),
                    "parameters": tool.get("input_schema").cloned().unwrap_or(json!({"type": "object"})),
                }
            })
        })
        .collect();
    if !tools.is_empty() {
        chat.insert("tools".into(), Value::Array(tools));
    }
    Value::Object(chat)
}

fn assistant_message(blocks: &[Value]) -> Value {
    let mut text = Vec::new();
    let mut calls = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("text") => {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    text.push(t.to_string());
                }
            }
            Some("tool_use") => calls.push(json!({
                "id": block.get("id").cloned().unwrap_or(Value::Null),
                "type": "function",
                "function": {
                    "name": block.get("name").cloned().unwrap_or(Value::Null),
                    "arguments": block.get("input").cloned().unwrap_or(json!({})).to_string(),
                }
            })),
            // Thinking blocks from an Anthropic provider mean nothing to OpenAI.
            _ => {}
        }
    }
    let mut message = json!({"role": "assistant"});
    message["content"] = if text.is_empty() {
        Value::Null
    } else {
        Value::String(text.join("\n"))
    };
    if !calls.is_empty() {
        message["tool_calls"] = Value::Array(calls);
    }
    message
}

fn user_messages(blocks: &[Value], messages: &mut Vec<Value>) {
    let mut text = Vec::new();
    for block in blocks {
        match block.get("type").and_then(Value::as_str) {
            Some("tool_result") => messages.push(json!({
                "role": "tool",
                "tool_call_id": block.get("tool_use_id").cloned().unwrap_or(Value::Null),
                "content": result_text(block.get("content")),
            })),
            Some("text") => {
                if let Some(t) = block.get("text").and_then(Value::as_str) {
                    text.push(t.to_string());
                }
            }
            _ => {}
        }
    }
    if !text.is_empty() {
        messages.push(json!({"role": "user", "content": text.join("\n")}));
    }
}

fn result_text(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(text)) => text.clone(),
        Some(Value::Array(blocks)) => blocks
            .iter()
            .filter_map(|block| block.get("text").and_then(Value::as_str))
            .collect::<Vec<_>>()
            .join("\n"),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// Convert a Chat Completions reply to an Anthropic Messages reply.
pub fn from_chat(reply: &Value) -> Value {
    if let Some(error) = reply.get("error") {
        return json!({"error": error});
    }
    let choice = reply
        .get("choices")
        .and_then(Value::as_array)
        .and_then(|choices| choices.first());
    let Some(message) = choice.and_then(|choice| choice.get("message")) else {
        return json!({"error": {"message": format!("the reply has no choices: {reply}")}});
    };
    let mut content = Vec::new();
    if let Some(text) = message.get("content").and_then(Value::as_str) {
        if !text.is_empty() {
            content.push(json!({"type": "text", "text": text}));
        }
    }
    for call in message
        .get("tool_calls")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        let function = call.get("function").cloned().unwrap_or(json!({}));
        let raw = function
            .get("arguments")
            .and_then(Value::as_str)
            .unwrap_or("{}");
        let input = match serde_json::from_str::<Value>(if raw.trim().is_empty() { "{}" } else { raw }) {
            Ok(value @ Value::Object(_)) => value,
            _ => json!({ INVALID_ARGUMENTS: raw }),
        };
        content.push(json!({
            "type": "tool_use",
            "id": call.get("id").cloned().unwrap_or(Value::Null),
            "name": function.get("name").cloned().unwrap_or(Value::Null),
            "input": input,
        }));
    }
    let finish = choice
        .and_then(|choice| choice.get("finish_reason"))
        .and_then(Value::as_str);
    let stop_reason = match finish {
        Some("tool_calls") => "tool_use",
        Some("length") => "max_tokens",
        _ => "end_turn",
    };
    json!({"role": "assistant", "content": content, "stop_reason": stop_reason})
}
