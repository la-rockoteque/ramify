//! Streamed model replies: the HTTP request on a reader thread, the server-sent events parser,
//! and the assemblers that build the same message a whole reply gives.

use crate::agent::INVALID_ARGUMENTS;
use crate::cancel::Cancel;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::io::Read;
use std::sync::mpsc;
use std::time::{Duration, Instant};

/// How often the stream checks the cancel flag while it waits for bytes.
const TICK: Duration = Duration::from_millis(50);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// How a streamed request ended.
#[derive(Debug, Clone, PartialEq)]
pub enum Ended {
    /// The server answered with a whole JSON reply, not a stream.
    Whole(Value),
    /// The stream closed, or an event said it is over.
    Done,
    Interrupted,
    Failed(String),
}

/// What an event handler tells the reader.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Flow {
    Continue,
    Stop,
}

/// One server-sent event: its `event:` name, if any, and its `data:` lines joined.
#[derive(Debug, Clone, PartialEq)]
pub struct Event {
    pub name: Option<String>,
    pub data: String,
}

/// Splits bytes into events. Bytes may arrive cut anywhere, even inside a UTF-8 character.
#[derive(Debug, Default)]
pub struct SseParser {
    pending: Vec<u8>,
    name: Option<String>,
    data: Vec<String>,
}

impl SseParser {
    pub fn push(&mut self, bytes: &[u8]) -> Vec<Event> {
        self.pending.extend_from_slice(bytes);
        let mut events = Vec::new();
        while let Some(end) = self.pending.iter().position(|b| *b == b'\n') {
            let raw: Vec<u8> = self.pending.drain(..=end).collect();
            let line = String::from_utf8_lossy(&raw[..raw.len() - 1]);
            let line = line.strip_suffix('\r').unwrap_or(&line);
            if line.is_empty() {
                events.extend(self.dispatch());
            } else if line.starts_with(':') {
                // A comment, which servers send to keep the connection open.
            } else {
                let (field, value) = line.split_once(':').unwrap_or((line, ""));
                let value = value.strip_prefix(' ').unwrap_or(value);
                match field {
                    "event" => self.name = Some(value.to_string()),
                    "data" => self.data.push(value.to_string()),
                    _ => {}
                }
            }
        }
        events
    }

    /// The last event, when the stream ends without a blank line after it.
    pub fn finish(&mut self) -> Option<Event> {
        if !self.pending.is_empty() {
            let rest = std::mem::take(&mut self.pending);
            let mut events = self.push(&rest);
            events.extend(self.push(b"\n"));
            if let Some(event) = events.pop() {
                return Some(event);
            }
        }
        self.dispatch()
    }

    fn dispatch(&mut self) -> Option<Event> {
        let name = self.name.take();
        if self.data.is_empty() {
            return None;
        }
        Some(Event {
            name,
            data: std::mem::take(&mut self.data).join("\n"),
        })
    }
}

/// What the reader thread sends.
enum Msg {
    Bytes(Vec<u8>),
    Whole(String),
    End,
    Error(String),
}

/// POST `body` and read the reply as a stream. A reply that is not `text/event-stream` comes back
/// whole. The stream fails when no byte arrives for `idle`, and stops when `cancel` is set.
pub fn post_stream(
    url: &str,
    headers: &[(&str, String)],
    body: &Value,
    idle: Duration,
    cancel: &Cancel,
    on_event: &mut dyn FnMut(Event) -> Flow,
) -> Ended {
    let text = match serde_json::to_string(body) {
        Ok(text) => text,
        Err(error) => return Ended::Failed(format!("could not encode the model request: {error}")),
    };
    let url = url.to_string();
    let headers: Vec<(String, String)> = headers
        .iter()
        .map(|(name, value)| (name.to_string(), value.clone()))
        .collect();
    let (sender, receiver) = mpsc::channel();
    // The thread owns the connection. On a cancel or a timeout the loop below stops listening,
    // and the thread ends when the server closes or the process exits.
    std::thread::spawn(move || read_reply(&url, &headers, &text, &sender));

    let mut parser = SseParser::default();
    let mut last = Instant::now();
    loop {
        if cancel.is_set() {
            return Ended::Interrupted;
        }
        match receiver.recv_timeout(TICK) {
            Ok(Msg::Bytes(bytes)) => {
                last = Instant::now();
                for event in parser.push(&bytes) {
                    if on_event(event) == Flow::Stop {
                        return Ended::Done;
                    }
                }
            }
            Ok(Msg::Whole(text)) => {
                return match serde_json::from_str(&text) {
                    Ok(value) => Ended::Whole(value),
                    Err(error) => Ended::Failed(format!("model API returned invalid JSON: {error}")),
                }
            }
            Ok(Msg::End) => {
                if let Some(event) = parser.finish() {
                    on_event(event);
                }
                return Ended::Done;
            }
            Ok(Msg::Error(error)) => return Ended::Failed(error),
            Err(mpsc::RecvTimeoutError::Timeout) => {
                if last.elapsed() >= idle {
                    return Ended::Failed(format!(
                        "the model stream sent nothing for {} s (the idle timeout, RAM_CODE_IDLE_TIMEOUT)",
                        idle.as_secs()
                    ));
                }
            }
            Err(mpsc::RecvTimeoutError::Disconnected) => return Ended::Done,
        }
    }
}

fn read_reply(url: &str, headers: &[(String, String)], body: &str, sender: &mpsc::Sender<Msg>) {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_connect(Some(CONNECT_TIMEOUT))
            .build(),
    );
    let mut post = agent.post(url);
    for (name, value) in headers {
        post = post.header(name.as_str(), value.as_str());
    }
    let mut response = match post.header("content-type", "application/json").send(body) {
        Ok(response) => response,
        Err(error) => {
            let _ = sender.send(Msg::Error(format!("model API request to {url} failed: {error}")));
            return;
        }
    };
    let status = response.status().as_u16();
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("")
        .to_ascii_lowercase();
    if status >= 400 || !content_type.contains("text/event-stream") {
        let text = response.body_mut().read_to_string().unwrap_or_default();
        let _ = if status >= 400 {
            sender.send(Msg::Error(format!(
                "model API returned {status}: {}",
                shorten(&text, 2000)
            )))
        } else {
            sender.send(Msg::Whole(text))
        };
        return;
    }
    let mut reader = response.into_body().into_reader();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk) {
            Ok(0) => {
                let _ = sender.send(Msg::End);
                return;
            }
            Ok(read) => {
                if sender.send(Msg::Bytes(chunk[..read].to_vec())).is_err() {
                    return;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
            Err(error) => {
                let _ = sender.send(Msg::Error(format!("the model stream broke: {error}")));
                return;
            }
        }
    }
}

fn shorten(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut cut = limit;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &text[..cut])
}

/// Parse a streamed tool input. Input that is not a JSON object goes through the
/// `INVALID_ARGUMENTS` path, as an OpenAI reply does.
fn parse_input(raw: &str) -> Value {
    let text = if raw.trim().is_empty() { "{}" } else { raw };
    match serde_json::from_str::<Value>(text) {
        Ok(value @ Value::Object(_)) => value,
        _ => json!({ INVALID_ARGUMENTS: raw }),
    }
}

/// One content block as it arrives.
#[derive(Debug, Clone)]
struct Block {
    value: Value,
    input: String,
    complete: bool,
}

/// Builds an Anthropic Messages reply from its stream events.
#[derive(Debug, Default)]
pub struct AnthropicAssembler {
    blocks: BTreeMap<u64, Block>,
    stop_reason: Option<Value>,
    usage: Map<String, Value>,
    pub finished: bool,
    pub error: Option<String>,
}

impl AnthropicAssembler {
    pub fn event(&mut self, event: &Event, on_text: &mut dyn FnMut(&str)) -> Flow {
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            return Flow::Continue;
        };
        let index = data.get("index").and_then(Value::as_u64).unwrap_or(0);
        match data.get("type").and_then(Value::as_str).unwrap_or("") {
            "message_start" => {
                if let Some(usage) = data.pointer("/message/usage").and_then(Value::as_object) {
                    self.usage.extend(usage.clone());
                }
            }
            "content_block_start" => {
                let value = data.get("content_block").cloned().unwrap_or(json!({}));
                // A redacted thinking block arrives whole in its start event.
                let complete = value.get("type").and_then(Value::as_str) == Some("redacted_thinking");
                self.blocks.insert(
                    index,
                    Block {
                        value,
                        input: String::new(),
                        complete,
                    },
                );
            }
            "content_block_delta" => {
                let Some(block) = self.blocks.get_mut(&index) else {
                    return Flow::Continue;
                };
                let delta = data.get("delta").cloned().unwrap_or(json!({}));
                match delta.get("type").and_then(Value::as_str).unwrap_or("") {
                    "text_delta" => {
                        let text = delta.get("text").and_then(Value::as_str).unwrap_or("");
                        append(&mut block.value, "text", text);
                        on_text(text);
                    }
                    "input_json_delta" => {
                        block
                            .input
                            .push_str(delta.get("partial_json").and_then(Value::as_str).unwrap_or(""));
                    }
                    "thinking_delta" => {
                        let text = delta.get("thinking").and_then(Value::as_str).unwrap_or("");
                        append(&mut block.value, "thinking", text);
                    }
                    "signature_delta" => {
                        let text = delta.get("signature").and_then(Value::as_str).unwrap_or("");
                        append(&mut block.value, "signature", text);
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some(block) = self.blocks.get_mut(&index) {
                    if block.value.get("type").and_then(Value::as_str) == Some("tool_use") {
                        block.value["input"] = parse_input(&block.input);
                    }
                    block.complete = true;
                }
            }
            "message_delta" => {
                if let Some(reason) = data.pointer("/delta/stop_reason") {
                    if !reason.is_null() {
                        self.stop_reason = Some(reason.clone());
                    }
                }
                if let Some(usage) = data.get("usage").and_then(Value::as_object) {
                    self.usage.extend(usage.clone());
                }
            }
            "message_stop" => {
                self.finished = true;
                return Flow::Stop;
            }
            "error" => {
                let message = data
                    .pointer("/error/message")
                    .and_then(Value::as_str)
                    .unwrap_or("the model stream sent an error");
                self.error = Some(format!("model API error: {message}"));
                return Flow::Stop;
            }
            _ => {}
        }
        Flow::Continue
    }

    /// The reply so far. A complete block stays. Of an incomplete block only text stays, since a
    /// tool input or a thinking signature is useless when cut.
    pub fn message(&self) -> Value {
        let content: Vec<Value> = self
            .blocks
            .values()
            .filter(|block| {
                let kind = block.value.get("type").and_then(Value::as_str);
                kind == Some("text") || block.complete
            })
            .map(|block| block.value.clone())
            .filter(|value| {
                value.get("type").and_then(Value::as_str) != Some("text")
                    || value.get("text").and_then(Value::as_str).is_some_and(|t| !t.is_empty())
            })
            .collect();
        let mut message = json!({"role": "assistant", "content": content});
        if let Some(reason) = &self.stop_reason {
            message["stop_reason"] = reason.clone();
        }
        if !self.usage.is_empty() {
            message["usage"] = Value::Object(self.usage.clone());
        }
        message
    }
}

fn append(value: &mut Value, key: &str, text: &str) {
    let mut current = value.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    current.push_str(text);
    value[key] = Value::String(current);
}

#[derive(Debug, Default, Clone)]
struct Call {
    id: String,
    name: String,
    arguments: String,
}

/// Builds an OpenAI Chat Completions reply from its chunks, then converts it as `from_chat` does.
#[derive(Debug, Default)]
pub struct OpenAiAssembler {
    text: String,
    refusal: String,
    calls: BTreeMap<u64, Call>,
    finish: Option<String>,
    pub finished: bool,
    pub error: Option<String>,
}

impl OpenAiAssembler {
    pub fn event(&mut self, event: &Event, on_text: &mut dyn FnMut(&str)) -> Flow {
        if event.data.trim() == "[DONE]" {
            self.finished = true;
            return Flow::Stop;
        }
        let Ok(data) = serde_json::from_str::<Value>(&event.data) else {
            return Flow::Continue;
        };
        if let Some(error) = data.get("error") {
            let message = error.get("message").and_then(Value::as_str).unwrap_or("unknown error");
            self.error = Some(format!("model API error: {message}"));
            return Flow::Stop;
        }
        let Some(choice) = data.pointer("/choices/0") else {
            return Flow::Continue;
        };
        let delta = choice.get("delta").cloned().unwrap_or(json!({}));
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            self.text.push_str(text);
            on_text(text);
        }
        if let Some(text) = delta.get("refusal").and_then(Value::as_str) {
            self.refusal.push_str(text);
        }
        for part in delta.get("tool_calls").and_then(Value::as_array).into_iter().flatten() {
            let index = part.get("index").and_then(Value::as_u64).unwrap_or(0);
            let call = self.calls.entry(index).or_default();
            if let Some(id) = part.get("id").and_then(Value::as_str) {
                call.id = id.to_string();
            }
            if let Some(name) = part.pointer("/function/name").and_then(Value::as_str) {
                call.name.push_str(name);
            }
            if let Some(arguments) = part.pointer("/function/arguments").and_then(Value::as_str) {
                call.arguments.push_str(arguments);
            }
        }
        if let Some(reason) = choice.get("finish_reason").and_then(Value::as_str) {
            self.finish = Some(reason.to_string());
        }
        Flow::Continue
    }

    /// True once the stream said it is over: `[DONE]` or a `finish_reason`.
    pub fn over(&self) -> bool {
        self.finished || self.finish.is_some()
    }

    /// The reply so far, in the Anthropic shape. Before the stream ends, the last tool call may
    /// be cut, so only the calls before it stay.
    pub fn message(&self) -> Value {
        let over = self.over();
        let last = self.calls.keys().next_back().copied();
        let calls: Vec<Value> = self
            .calls
            .iter()
            .filter(|(index, _)| over || Some(**index) != last)
            .map(|(_, call)| {
                json!({"id": call.id, "type": "function",
                    "function": {"name": call.name, "arguments": call.arguments}})
            })
            .collect();
        let mut message = json!({"role": "assistant"});
        message["content"] = if self.text.is_empty() {
            Value::Null
        } else {
            Value::String(self.text.clone())
        };
        if !self.refusal.is_empty() {
            message["refusal"] = Value::String(self.refusal.clone());
        }
        if !calls.is_empty() {
            message["tool_calls"] = Value::Array(calls);
        }
        let reply = json!({"choices": [{"finish_reason": self.finish, "message": message}]});
        let converted = crate::openai::from_chat(&reply);
        if converted.get("error").is_some() {
            return json!({"role": "assistant", "content": []});
        }
        converted
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parser_joins_data_lines_and_handles_split_reads() {
        let mut parser = SseParser::default();
        let mut events = parser.push(b"event: a\ndata: one\nda");
        assert!(events.is_empty());
        events.extend(parser.push(b"ta: two\r\n\r\n: comment\n\ndata: \xc3"));
        events.extend(parser.push(b"\xa9\n\n"));
        assert_eq!(
            events,
            vec![
                Event { name: Some("a".into()), data: "one\ntwo".into() },
                Event { name: None, data: "é".into() },
            ]
        );
    }

    #[test]
    fn parser_keeps_a_last_event_without_a_blank_line() {
        let mut parser = SseParser::default();
        assert!(parser.push(b"data: [DONE]").is_empty());
        assert_eq!(parser.finish().map(|e| e.data), Some("[DONE]".into()));
    }

    #[test]
    fn text_is_not_kept_twice() {
        let mut asm = AnthropicAssembler::default();
        let mut seen = String::new();
        for data in [
            json!({"type": "content_block_start", "index": 0, "content_block": {"type": "text", "text": ""}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "ab"}}),
            json!({"type": "content_block_delta", "index": 0, "delta": {"type": "text_delta", "text": "c"}}),
            json!({"type": "content_block_stop", "index": 0}),
        ] {
            asm.event(&Event { name: None, data: data.to_string() }, &mut |t| seen.push_str(t));
        }
        assert_eq!(seen, "abc");
        assert_eq!(asm.message()["content"], json!([{"type": "text", "text": "abc"}]));
    }
}
