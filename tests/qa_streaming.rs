//! The cases of `.ramify/work/ram-code-streaming/qa-plan.md` that the build settles.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ram_code::agent::{
    run_task, run_turn_cancellable, AgentConfig, AgentEvent, AnthropicLlm, Credential, Llm,
    ReplyEnd, TurnEnd,
};
use ram_code::app::{App, Role, Screen};
use ram_code::cancel::Cancel;
use ram_code::hooked::Hooked;
use ram_code::openai::{from_chat, OpenAiLlm};
use ram_code::providers::{resolve, Providers};
use ram_code::ramify::{CommandOutput, CommandSpec, Runner};
use ram_code::tools::Tools;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ── helpers ──────────────────────────────────────────────────────────────────

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramcode-stream-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// One piece of a reply: a pause, then bytes.
#[derive(Clone)]
struct Piece(u64, String);

#[derive(Clone)]
struct Reply {
    status: u16,
    content_type: &'static str,
    pieces: Vec<Piece>,
}

fn sse(events: &[(u64, Value)]) -> Reply {
    Reply {
        status: 200,
        content_type: "text/event-stream",
        pieces: events
            .iter()
            .map(|(pause, event)| {
                let kind = event["type"].as_str().unwrap_or("message");
                Piece(*pause, format!("event: {kind}\ndata: {event}\n\n"))
            })
            .collect(),
    }
}

fn chunks(events: &[(u64, Value)]) -> Reply {
    let mut pieces: Vec<Piece> = events
        .iter()
        .map(|(pause, event)| Piece(*pause, format!("data: {event}\n\n")))
        .collect();
    pieces.push(Piece(0, "data: [DONE]\n\n".into()));
    Reply {
        status: 200,
        content_type: "text/event-stream",
        pieces,
    }
}

fn whole(value: Value) -> Reply {
    Reply {
        status: 200,
        content_type: "application/json",
        pieces: vec![Piece(0, value.to_string())],
    }
}

fn error_reply(status: u16, value: Value) -> Reply {
    Reply {
        status,
        content_type: "application/json",
        pieces: vec![Piece(0, value.to_string())],
    }
}

#[derive(Debug, Clone)]
struct Seen {
    path: String,
    body: Value,
}

/// A local HTTP server: each request gets the next reply, sent piece by piece.
fn serve(replies: Vec<Reply>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    std::thread::spawn(move || {
        for (stream, reply) in listener.incoming().zip(replies) {
            let Ok(mut stream) = stream else { return };
            let record = record.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                let mut line = String::new();
                reader.read_line(&mut line).unwrap();
                let path = line.split_whitespace().nth(1).unwrap_or("").to_string();
                let mut headers = HashMap::new();
                loop {
                    let mut header = String::new();
                    reader.read_line(&mut header).unwrap();
                    let header = header.trim_end();
                    if header.is_empty() {
                        break;
                    }
                    if let Some((k, v)) = header.split_once(':') {
                        headers.insert(k.trim().to_ascii_lowercase(), v.trim().to_string());
                    }
                }
                let length: usize = headers
                    .get("content-length")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                let mut body = vec![0; length];
                reader.read_exact(&mut body).unwrap();
                record.lock().unwrap().push(Seen {
                    path,
                    body: serde_json::from_slice(&body).unwrap_or(Value::Null),
                });
                let _ = write!(
                    stream,
                    "HTTP/1.1 {} X\r\ncontent-type: {}\r\nconnection: close\r\n\r\n",
                    reply.status, reply.content_type
                );
                let _ = stream.flush();
                for Piece(pause, bytes) in reply.pieces {
                    std::thread::sleep(Duration::from_millis(pause));
                    if stream.write_all(bytes.as_bytes()).is_err() {
                        return;
                    }
                    let _ = stream.flush();
                }
            });
        }
    });
    (base, seen)
}

fn config(base: &str) -> AgentConfig {
    AgentConfig {
        base_url: base.to_string(),
        credential: Credential::ApiKey("k".into()),
        model: "m".into(),
        max_tokens: 100,
        stream: true,
        idle_timeout: 5,
    }
}

fn message_start() -> Value {
    json!({"type": "message_start", "message": {"id": "msg_1", "type": "message", "role": "assistant",
        "content": [], "model": "m", "stop_reason": null, "usage": {"input_tokens": 12, "output_tokens": 1}}})
}

fn text_block(index: u64) -> Value {
    json!({"type": "content_block_start", "index": index, "content_block": {"type": "text", "text": ""}})
}

fn text_delta(index: u64, text: &str) -> Value {
    json!({"type": "content_block_delta", "index": index, "delta": {"type": "text_delta", "text": text}})
}

fn tool_block(index: u64, id: &str, name: &str) -> Value {
    json!({"type": "content_block_start", "index": index,
        "content_block": {"type": "tool_use", "id": id, "name": name, "input": {}}})
}

fn json_delta(index: u64, part: &str) -> Value {
    json!({"type": "content_block_delta", "index": index,
        "delta": {"type": "input_json_delta", "partial_json": part}})
}

fn block_stop(index: u64) -> Value {
    json!({"type": "content_block_stop", "index": index})
}

fn message_end(stop: &str) -> Vec<Value> {
    vec![
        json!({"type": "message_delta", "delta": {"stop_reason": stop, "stop_sequence": null},
            "usage": {"output_tokens": 30}}),
        json!({"type": "message_stop"}),
    ]
}

fn anthropic_text(text: &str) -> Reply {
    let mut events = vec![(0, message_start()), (0, text_block(0)), (0, text_delta(0, text)), (0, block_stop(0))];
    events.extend(message_end("end_turn").into_iter().map(|e| (0, e)));
    sse(&events)
}

fn executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// The environment of a real worker or headless run against a fake server, with no hook,
/// instruction or key from the developer's own setup.
fn isolated(command: &mut std::process::Command, home: &Path, base: &str) {
    command
        .env_clear()
        .env("PATH", std::env::var("PATH").unwrap_or_default())
        .env("HOME", home)
        .env("RAMIFY_HOOKS_HOME", home)
        .env("RAMIFY_HOME", home)
        .env("RAMIFY_STATE_DIR", home.join("state"))
        .env("RAM_CODE_CONFIG_DIR", home.join("config"))
        .env("RAM_CODE_INSTRUCTIONS_HOME", home)
        .env("RAM_CODE_PROVIDER", "env")
        .env("ANTHROPIC_AUTH_TOKEN", "fake-token")
        .env("ANTHROPIC_BASE_URL", base);
}

fn events_of(path: &Path) -> Vec<Value> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect()
}

struct NoRunner;

impl Runner for NoRunner {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, String> {
        let stdout = if spec.args.first().map(String::as_str) == Some("dash-project") {
            json!({
                "name": "demo", "primary": spec.root, "stacks": true, "jira": null,
                "containers": [], "shared": [], "build_servers": [], "strays": [], "orphans": [],
                "worktrees": [{
                    "name": "demo", "path": spec.root, "branch": "main", "ticket": null,
                    "workflow": null, "primary": true, "dirty": false, "slot": 0,
                    "uptime": null, "errors": [], "services": []
                }],
            })
            .to_string()
        } else {
            String::new()
        };
        Ok(CommandOutput {
            status: 0,
            stdout,
            stderr: String::new(),
        })
    }
}

/// An app on `dir`, with a provider that resolves, and `worker` as the task process.
fn app(dir: &Path, worker: &Path) -> App {
    let mut app = App::new(Arc::new(NoRunner), dir.to_path_buf());
    let vars: HashMap<String, String> =
        [("ANTHROPIC_AUTH_TOKEN".to_string(), "t".to_string())].into();
    app.env = Arc::new(move |key| vars.get(key).cloned());
    app.entries.clear();
    app.load_providers(dir.join("config"));
    app.load();
    app.worker_binary = Some(worker.to_path_buf());
    app
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn send(app: &mut App, text: &str) {
    app.input = text.to_string();
    app.on_key(key(KeyCode::Enter));
}

fn poll_until(app: &mut App, limit: Duration, done: impl Fn(&App) -> bool) -> bool {
    let end = Instant::now() + limit;
    while Instant::now() < end {
        app.poll_events();
        if done(app) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

fn transcript(app: &App) -> String {
    app.entries
        .iter()
        .map(|e| e.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

// ── Q1 ───────────────────────────────────────────────────────────────────────

#[test]
fn text_deltas_reach_the_events_file_before_finished() {
    let home = tmp("q1");
    let mut events = vec![(0, message_start()), (0, text_block(0))];
    for part in ["one ", "two ", "three"] {
        events.push((300, text_delta(0, part)));
    }
    events.push((0, block_stop(0)));
    events.extend(message_end("end_turn").into_iter().map(|e| (0, e)));
    let (base, _) = serve(vec![sse(&events)]);
    let events_file = home.join("events.jsonl");
    let history = home.join("history.json");
    fs::write(&history, "[]").unwrap();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ram-code"));
    command
        .args(["--task", "count"])
        .arg("--events")
        .arg(&events_file)
        .arg("--history")
        .arg(&history)
        .current_dir(&home);
    isolated(&mut command, &home, &base);
    command.env("RAMIFY_ROOT", &home);
    let status = command.status().unwrap();
    assert!(status.success());
    let kinds: Vec<String> = events_of(&events_file)
        .iter()
        .map(|e| e["type"].as_str().unwrap_or("").to_string())
        .collect();
    let first_delta = kinds.iter().position(|k| k == "text_delta").expect("a text_delta");
    let finished = kinds.iter().position(|k| k == "finished").expect("finished");
    assert!(first_delta < finished, "{kinds:?}");
    assert_eq!(kinds.iter().filter(|k| *k == "text_delta").count(), 3, "{kinds:?}");
}

// ── Q2 ───────────────────────────────────────────────────────────────────────

#[test]
fn transcript_shows_partial_text() {
    let dir = tmp("q2");
    let script = dir.join("worker.sh");
    executable(
        &script,
        "#!/bin/sh\nevents=\"$4\"\nhistory=\"$6\"\n\
         printf '{\"type\":\"text_delta\",\"text\":\"Hel\"}\\n' >> \"$events\"\n\
         sleep 1\n\
         printf '{\"type\":\"text_delta\",\"text\":\"lo\"}\\n' >> \"$events\"\n\
         printf '{\"type\":\"text\",\"text\":\"Hello\"}\\n' >> \"$events\"\n\
         printf '[]' > \"$history\"\n\
         printf '{\"type\":\"finished\",\"answer\":\"Hello\"}\\n' >> \"$events\"\n",
    );
    let mut app = app(&dir, &script);
    send(&mut app, "greet");
    let partial = poll_until(&mut app, Duration::from_millis(900), |app| {
        app.entries.iter().any(|e| e.role == Role::Assistant && e.text == "Hel")
    });
    assert!(partial, "no partial text: {}", transcript(&app));
    assert!(app.running, "the task ended before the stream did");
    assert!(poll_until(&mut app, Duration::from_secs(5), |app| !app.running));
    let assistant: Vec<&str> = app
        .entries
        .iter()
        .filter(|e| e.role == Role::Assistant)
        .map(|e| e.text.as_str())
        .collect();
    assert_eq!(assistant, vec!["Hello"], "the text shows twice or not at all");
}

// ── Q3 ───────────────────────────────────────────────────────────────────────

fn rich_events() -> Vec<(u64, Value)> {
    let mut events = vec![
        (0, message_start()),
        (0, json!({"type": "content_block_start", "index": 0,
            "content_block": {"type": "thinking", "thinking": "", "signature": ""}})),
        (0, json!({"type": "content_block_delta", "index": 0,
            "delta": {"type": "thinking_delta", "thinking": "let me "}})),
        (0, json!({"type": "content_block_delta", "index": 0,
            "delta": {"type": "thinking_delta", "thinking": "look"}})),
        (0, json!({"type": "content_block_delta", "index": 0,
            "delta": {"type": "signature_delta", "signature": "sig-1"}})),
        (0, block_stop(0)),
        (0, text_block(1)),
        (0, text_delta(1, "Reading ")),
        (0, json!({"type": "ping"})),
        (0, text_delta(1, "the file.")),
        (0, block_stop(1)),
        (0, tool_block(2, "toolu_1", "read_file")),
        (0, json_delta(2, "{\"pa")),
        (0, json_delta(2, "th\": \"a")),
        (0, json_delta(2, ".txt\"}")),
        (0, block_stop(2)),
    ];
    events.extend(message_end("tool_use").into_iter().map(|e| (0, e)));
    events
}

fn rich_fixture() -> Value {
    json!([
        {"type": "thinking", "thinking": "let me look", "signature": "sig-1"},
        {"type": "text", "text": "Reading the file."},
        {"type": "tool_use", "id": "toolu_1", "name": "read_file", "input": {"path": "a.txt"}}
    ])
}

#[test]
fn anthropic_stream_assembles_like_a_whole_reply() {
    let (base, _) = serve(vec![sse(&rich_events())]);
    let llm = AnthropicLlm::new(config(&base));
    let mut deltas = Vec::new();
    let reply = llm.stream(&json!({"model": "m", "max_tokens": 10, "messages": []}), &Cancel::new(), &mut |t| {
        deltas.push(t.to_string())
    });
    assert_eq!(reply.end, ReplyEnd::Complete);
    assert_eq!(reply.message["content"], rich_fixture());
    assert_eq!(deltas.concat(), "Reading the file.");

    let dir = tmp("q3");
    fs::write(dir.join("a.txt"), "alpha").unwrap();
    let (base, _) = serve(vec![sse(&rich_events()), anthropic_text("done")]);
    let llm = AnthropicLlm::new(config(&base));
    let mut results = Vec::new();
    let answer = run_task(&llm, &dir, "read a.txt", &mut |event| {
        if let AgentEvent::ToolResult(text) = event {
            results.push(text);
        }
    })
    .unwrap();
    assert_eq!(results, vec!["alpha".to_string()]);
    assert!(answer.ends_with("done"), "{answer}");
}

// ── Q4 ───────────────────────────────────────────────────────────────────────

#[test]
fn anthropic_stream_keeps_usage_and_stop_reason() {
    let (base, _) = serve(vec![sse(&rich_events())]);
    let llm = AnthropicLlm::new(config(&base));
    let reply = llm.stream(&json!({"messages": []}), &Cancel::new(), &mut |_| {});
    assert_eq!(reply.message["stop_reason"], "tool_use");
    assert_eq!(reply.message["usage"]["input_tokens"], 12);
    assert_eq!(reply.message["usage"]["output_tokens"], 30);
}

// ── Q5 ───────────────────────────────────────────────────────────────────────

#[test]
fn openai_stream_assembles_split_tool_calls() {
    let delta = |d: Value| json!({"choices": [{"index": 0, "delta": d, "finish_reason": null}]});
    let events = vec![
        (0, delta(json!({"role": "assistant", "content": "Two "}))),
        (0, delta(json!({"content": "calls."}))),
        (0, delta(json!({"tool_calls": [{"index": 0, "id": "call_a", "type": "function",
            "function": {"name": "read_file", "arguments": ""}}]}))),
        (0, delta(json!({"tool_calls": [{"index": 0, "function": {"arguments": "{\"path\":"}}]}))),
        (0, delta(json!({"tool_calls": [{"index": 1, "id": "call_b", "type": "function",
            "function": {"name": "list_files", "arguments": "{}"}}]}))),
        (0, delta(json!({"tool_calls": [{"index": 0, "function": {"arguments": " \"a.txt\"}"}}]}))),
        (0, json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]})),
    ];
    let fixture = json!({"choices": [{"finish_reason": "tool_calls", "message": {
        "role": "assistant", "content": "Two calls.",
        "tool_calls": [
            {"id": "call_a", "type": "function", "function": {"name": "read_file", "arguments": "{\"path\": \"a.txt\"}"}},
            {"id": "call_b", "type": "function", "function": {"name": "list_files", "arguments": "{}"}}
        ]}}]});
    let (base, seen) = serve(vec![chunks(&events)]);
    let llm = OpenAiLlm::new(config(&base));
    let reply = llm.stream(&json!({"model": "m", "messages": []}), &Cancel::new(), &mut |_| {});
    assert_eq!(reply.end, ReplyEnd::Complete);
    let expected = from_chat(&fixture);
    assert_eq!(reply.message["content"], expected["content"]);
    assert_eq!(reply.message["stop_reason"], expected["stop_reason"]);
    assert_eq!(seen.lock().unwrap()[0].body["stream"], true);
    assert_eq!(seen.lock().unwrap()[0].path, "/v1/chat/completions");

    let dir = tmp("q5");
    fs::write(dir.join("a.txt"), "alpha").unwrap();
    let done = chunks(&[(0, json!({"choices": [{"index": 0, "delta": {"content": "ok"}, "finish_reason": "stop"}]}))]);
    let (base, _) = serve(vec![chunks(&events), done]);
    let llm = OpenAiLlm::new(config(&base));
    let mut calls = Vec::new();
    run_task(&llm, &dir, "go", &mut |event| {
        if let AgentEvent::ToolCall { name, .. } = event {
            calls.push(name);
        }
    })
    .unwrap();
    assert_eq!(calls, vec!["read_file".to_string(), "list_files".to_string()]);
}

// ── Q6 ───────────────────────────────────────────────────────────────────────

#[test]
fn bad_streamed_input_is_a_tool_error() {
    let dir = tmp("q6");
    let mut events = vec![(0, message_start()), (0, tool_block(0, "toolu_x", "list_files")),
        (0, json_delta(0, "{not ")), (0, json_delta(0, "json")), (0, block_stop(0))];
    events.extend(message_end("tool_use").into_iter().map(|e| (0, e)));
    let (base, seen) = serve(vec![sse(&events), anthropic_text("sorry")]);
    let llm = AnthropicLlm::new(config(&base));
    let mut results = Vec::new();
    let answer = run_task(&llm, &dir, "list", &mut |event| {
        if let AgentEvent::ToolResult(text) = event {
            results.push(text);
        }
    })
    .unwrap();
    assert_eq!(answer, "sorry");
    assert!(results[0].starts_with("error: the tool arguments are not valid JSON"), "{results:?}");
    assert_eq!(seen.lock().unwrap().len(), 2, "the next turn did not run");
}

// ── Q7 ───────────────────────────────────────────────────────────────────────

#[test]
fn idle_stream_times_out() {
    let events = vec![(0, message_start()), (0, text_block(0)), (0, text_delta(0, "a")),
        (3000, text_delta(0, "b"))];
    let (base, _) = serve(vec![sse(&events)]);
    let mut cfg = config(&base);
    cfg.idle_timeout = 1;
    let llm = AnthropicLlm::new(cfg);
    let started = Instant::now();
    let reply = llm.stream(&json!({"messages": []}), &Cancel::new(), &mut |_| {});
    assert!(started.elapsed() < Duration::from_millis(2500), "{:?}", started.elapsed());
    match reply.end {
        ReplyEnd::Failed(error) => assert!(error.contains("idle"), "{error}"),
        other => panic!("expected an idle timeout, got {other:?}"),
    }
    assert_eq!(reply.message["content"][0]["text"], "a", "the partial text is lost");
}

// ── Q8 ───────────────────────────────────────────────────────────────────────

#[test]
fn active_long_stream_succeeds() {
    // The whole-reply limit applies to a reply read whole. Lower it so the test is quick: a
    // stream longer than it must still succeed, and a whole reply slower than it must fail.
    let mut events = vec![(0, message_start()), (0, text_block(0))];
    for _ in 0..5 {
        events.push((500, text_delta(0, "x")));
    }
    events.push((0, block_stop(0)));
    events.extend(message_end("end_turn").into_iter().map(|e| (0, e)));
    let (base, _) = serve(vec![sse(&events)]);
    let mut cfg = config(&base);
    cfg.idle_timeout = 1;
    let mut llm = AnthropicLlm::new(cfg);
    llm.whole_timeout = Duration::from_secs(1);
    let started = Instant::now();
    let reply = llm.stream(&json!({"messages": []}), &Cancel::new(), &mut |_| {});
    assert!(started.elapsed() > Duration::from_secs(2), "the stream was not longer than the limit");
    assert_eq!(reply.end, ReplyEnd::Complete);
    assert_eq!(reply.message["content"][0]["text"], "xxxxx");

    // The same limit does stop a whole reply, so the test above shows the stream ignores it.
    let slow = Reply {
        status: 200,
        content_type: "application/json",
        pieces: vec![Piece(2000, json!({"content": [{"type": "text", "text": "late"}]}).to_string())],
    };
    let (base, _) = serve(vec![slow]);
    let mut cfg = config(&base);
    cfg.stream = false;
    let mut llm = AnthropicLlm::new(cfg);
    llm.whole_timeout = Duration::from_secs(1);
    let reply = llm.stream(&json!({"messages": []}), &Cancel::new(), &mut |_| {});
    assert!(matches!(reply.end, ReplyEnd::Failed(_)), "{:?}", reply.end);
}

// ── Q9 ───────────────────────────────────────────────────────────────────────

#[test]
fn json_reply_to_a_stream_request_is_read_whole() {
    let (base, seen) = serve(vec![whole(json!({"content": [{"type": "text", "text": "whole"}],
        "stop_reason": "end_turn"}))]);
    let llm = AnthropicLlm::new(config(&base));
    let reply = llm.stream(&json!({"messages": []}), &Cancel::new(), &mut |_| {});
    assert_eq!(reply.end, ReplyEnd::Complete);
    assert_eq!(reply.message["content"][0]["text"], "whole");
    assert_eq!(seen.lock().unwrap()[0].body["stream"], true);
    assert_eq!(seen.lock().unwrap()[0].path, "/v1/messages");
}

// ── Q10 ──────────────────────────────────────────────────────────────────────

#[test]
fn streaming_can_be_turned_off() {
    let dir = tmp("q10");
    fs::write(
        dir.join("providers.json"),
        json!({"providers": [{"name": "plain", "base_url": "http://127.0.0.1:9",
            "key_env": "K", "model": "m", "stream": false}]})
        .to_string(),
    )
    .unwrap();
    let providers = Providers::load(dir.clone()).unwrap();
    let vars: HashMap<String, String> = [("K".to_string(), "v".to_string())].into();
    let plain = resolve(&providers.find("plain").unwrap(), &|k| vars.get(k).cloned(), None).unwrap();
    assert!(!plain.config.stream);

    let mut off = vars.clone();
    off.insert("RAM_CODE_STREAM".into(), "0".into());
    off.insert("DEEPSEEK_API_KEY".into(), "d".into());
    let deepseek = resolve(&providers.find("deepseek").unwrap(), &|k| off.get(k).cloned(), None).unwrap();
    assert!(!deepseek.config.stream);

    let (base, seen) = serve(vec![whole(json!({"content": [{"type": "text", "text": "x"}]}))]);
    let mut cfg = config(&base);
    cfg.stream = false;
    let llm = AnthropicLlm::new(cfg);
    let reply = llm.stream(&json!({"messages": []}), &Cancel::new(), &mut |_| {});
    assert_eq!(reply.end, ReplyEnd::Complete);
    assert!(seen.lock().unwrap()[0].body.get("stream").is_none());
    let (base, seen) = serve(vec![whole(json!({"choices": [{"message": {"role": "assistant", "content": "x"}}]}))]);
    let mut cfg = config(&base);
    cfg.stream = false;
    let llm = OpenAiLlm::new(cfg);
    let reply = llm.stream(&json!({"messages": []}), &Cancel::new(), &mut |_| {});
    assert_eq!(reply.end, ReplyEnd::Complete);
    assert!(seen.lock().unwrap()[0].body.get("stream").is_none());
}

// ── Q11 ──────────────────────────────────────────────────────────────────────

#[test]
fn esc_stops_the_task_within_a_second() {
    let dir = tmp("q11");
    let script = dir.join("worker.sh");
    executable(
        &script,
        "#!/bin/sh\nevents=\"$4\"\nhistory=\"$6\"\n\
         trap 'printf \"{\\\"type\\\":\\\"interrupted\\\"}\\n\" >> \"$events\"; printf \"[]\" > \"$history\"; \
         printf \"{\\\"type\\\":\\\"finished\\\",\\\"answer\\\":\\\"\\\"}\\n\" >> \"$events\"; exit 0' INT\n\
         printf '{\"type\":\"text_delta\",\"text\":\"slow\"}\\n' >> \"$events\"\n\
         i=0; while [ $i -lt 300 ]; do sleep 0.1; i=$((i+1)); done\n",
    );
    let mut app = app(&dir, &script);
    send(&mut app, "go slowly");
    assert!(poll_until(&mut app, Duration::from_secs(3), |app| {
        app.entries.iter().any(|e| e.role == Role::Assistant)
    }));
    let pressed = Instant::now();
    app.on_key(key(KeyCode::Esc));
    assert!(poll_until(&mut app, Duration::from_secs(1), |app| !app.running), "still running");
    assert!(pressed.elapsed() < Duration::from_secs(1));
    assert!(transcript(&app).contains("interrupted"), "{}", transcript(&app));
    app.on_key(key(KeyCode::Char('n')));
    assert_eq!(app.input, "n", "the input does not take text");
}

// ── Q12 ──────────────────────────────────────────────────────────────────────

/// The checks a fake server applies to a request's messages.
fn anthropic_valid(messages: &[Value]) -> Result<(), String> {
    let mut last = "";
    let mut open: Vec<String> = Vec::new();
    for message in messages {
        let role = message["role"].as_str().unwrap_or("");
        if role == last {
            return Err(format!("two {role} messages in a row"));
        }
        last = role;
        let content = &message["content"];
        if content.as_str().is_some_and(str::is_empty) || content.as_array().is_some_and(Vec::is_empty) {
            return Err("empty content".into());
        }
        for block in content.as_array().into_iter().flatten() {
            match block["type"].as_str() {
                Some("tool_use") => open.push(block["id"].as_str().unwrap_or("").to_string()),
                Some("tool_result") => {
                    let id = block["tool_use_id"].as_str().unwrap_or("");
                    open.retain(|o| o != id);
                }
                Some("text") if block["text"].as_str().is_some_and(|t| t.trim().is_empty()) => {
                    return Err("empty or whitespace-only text block".into())
                }
                _ => {}
            }
        }
        if role == "user" && !open.is_empty() {
            return Err(format!("tool_use without a result: {open:?}"));
        }
    }
    Ok(())
}

fn chat_valid(messages: &[Value]) -> Result<(), String> {
    let mut open: Vec<String> = Vec::new();
    for message in messages {
        match message["role"].as_str().unwrap_or("") {
            "assistant" => {
                if !open.is_empty() {
                    return Err(format!("tool calls without results: {open:?}"));
                }
                let calls = message["tool_calls"].as_array().cloned().unwrap_or_default();
                if message["content"].is_null() && calls.is_empty() {
                    return Err("assistant with no content".into());
                }
                open = calls.iter().map(|c| c["id"].as_str().unwrap_or("").to_string()).collect();
            }
            "tool" => {
                let id = message["tool_call_id"].as_str().unwrap_or("");
                if !open.iter().any(|o| o == id) {
                    return Err(format!("a tool message for unknown call {id}"));
                }
                open.retain(|o| o != id);
            }
            _ => {
                if !open.is_empty() {
                    return Err(format!("tool calls without results: {open:?}"));
                }
            }
        }
    }
    Ok(())
}

#[test]
fn interrupted_history_is_valid_for_both_formats() {
    let dir = tmp("q12");
    let events = vec![
        (0, message_start()),
        (0, text_block(0)),
        (0, text_delta(0, "I will list ")),
        (0, block_stop(0)),
        (0, tool_block(1, "toolu_done", "list_files")),
        (0, json_delta(1, "{}")),
        (0, block_stop(1)),
        (0, tool_block(2, "toolu_half", "read_file")),
        (0, json_delta(2, "{\"pa")),
        (5000, json_delta(2, "th\": \"x\"}")),
    ];
    let (base, _) = serve(vec![sse(&events)]);
    let llm = AnthropicLlm::new(config(&base));
    let cancel = Cancel::new();
    let trigger = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(400));
        trigger.cancel();
    });
    let started = Instant::now();
    let turn = run_turn_cancellable(&llm, &dir, &[], "list", &Hooked::none(&dir), &cancel, &mut |_| {});
    assert!(started.elapsed() < Duration::from_millis(1500), "{:?}", started.elapsed());
    assert_eq!(turn.end, TurnEnd::Interrupted);
    let history = turn.history;
    let assistant = &history[1];
    assert_eq!(assistant["role"], "assistant");
    let blocks = assistant["content"].as_array().unwrap();
    assert_eq!(blocks[0]["text"], "I will list ");
    assert_eq!(blocks[1]["id"], "toolu_done");
    assert_eq!(blocks.len(), 2, "the incomplete tool_use was kept: {blocks:?}");
    let results = history[2]["content"].as_array().unwrap();
    assert_eq!(results[0]["tool_use_id"], "toolu_done");
    assert_eq!(results[0]["is_error"], true);
    assert_eq!(results[0]["content"], "interrupted by the user");

    let (base, seen) = serve(vec![anthropic_text("next")]);
    let llm = AnthropicLlm::new(config(&base));
    run_turn_cancellable(&llm, &dir, &history, "go on", &Hooked::none(&dir), &Cancel::new(), &mut |_| {});
    let body = seen.lock().unwrap()[0].body.clone();
    anthropic_valid(body["messages"].as_array().unwrap()).unwrap();

    let done = chunks(&[(0, json!({"choices": [{"index": 0, "delta": {"content": "ok"}, "finish_reason": "stop"}]}))]);
    let (base, seen) = serve(vec![done]);
    let llm = OpenAiLlm::new(config(&base));
    run_turn_cancellable(&llm, &dir, &history, "go on", &Hooked::none(&dir), &Cancel::new(), &mut |_| {});
    let body = seen.lock().unwrap()[0].body.clone();
    chat_valid(body["messages"].as_array().unwrap()).unwrap();
}

// ── Q13 ──────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn interrupt_kills_the_command_group() {
    let dir = tmp("q13");
    let cancel = Cancel::new();
    let tools = Tools::with_env(
        &dir,
        ["PATH", "HOME"]
            .iter()
            .filter_map(|k| std::env::var(k).ok().map(|v| (k.to_string(), v)))
            .collect(),
    )
    .with_cancel(cancel.clone());
    let trigger = cancel.clone();
    let pid_file = dir.join("pid");
    let pid_path = pid_file.clone();
    std::thread::spawn(move || {
        let end = Instant::now() + Duration::from_secs(10);
        while !pid_path.exists() && Instant::now() < end {
            std::thread::sleep(Duration::from_millis(20));
        }
        std::thread::sleep(Duration::from_millis(200));
        trigger.cancel();
    });
    let started = Instant::now();
    let out = tools
        .call("run_command", &json!({"command": "echo $$ > pid; sleep 60 & sleep 60"}))
        .unwrap_or_else(|e| e);
    let group: i32 = fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
    assert!(out.contains("interrupted"), "{out}");
    let deadline = Instant::now() + Duration::from_secs(1);
    let mut alive = true;
    while Instant::now() < deadline {
        // SAFETY: a signal 0 only checks that the group exists.
        alive = unsafe { libc::killpg(group, 0) } == 0;
        if !alive {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(!alive, "the group {group} still runs");
    assert!(started.elapsed() < Duration::from_secs(5));
}

// ── Q14 ──────────────────────────────────────────────────────────────────────

#[test]
fn interrupt_cancels_the_run_wait() {
    let dir = tmp("q14");
    let bin = dir.join("bin");
    fs::create_dir_all(&bin).unwrap();
    let program = bin.join("ramify");
    executable(
        &program,
        &format!(
            "#!/bin/sh\nprintf '%s\\n' \"$*\" >> '{log}'\n\
             if [ \"$1 $2\" = 'run ask' ]; then echo 'run 20261112-120000-42 waits on the human'; exit 0; fi\n\
             if [ \"$1 $2\" = 'run wait' ]; then sleep 30; echo 'ran: exit 0'; exit 0; fi\n\
             exit 3\n",
            log = dir.join("calls.log").display()
        ),
    );
    let cancel = Cancel::new();
    let tools = Tools::with_env(
        &dir,
        vec![
            ("PATH".into(), std::env::var("PATH").unwrap_or_default()),
            ("RAM_CODE_RAMIFY".into(), program.display().to_string()),
        ],
    )
    .with_cancel(cancel.clone());
    let trigger = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        trigger.cancel();
    });
    let started = Instant::now();
    let out = tools
        .call(
            "run_command",
            &json!({"command": "curl -sI https://example.com", "outside_sandbox": true, "reason": "fetch"}),
        )
        .unwrap_or_else(|e| e);
    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    assert!(out.contains("interrupted"), "{out}");
    let calls = fs::read_to_string(dir.join("calls.log")).unwrap_or_default();
    assert!(calls.contains("run wait"), "{calls}");
    assert!(!calls.contains("run go") && !calls.contains("run block"), "{calls}");
}

// ── Q15 ──────────────────────────────────────────────────────────────────────

#[test]
fn stuck_worker_is_killed_and_history_rolls_back() {
    let dir = tmp("q15");
    let script = dir.join("worker.sh");
    executable(
        &script,
        "#!/bin/sh\nevents=\"$4\"\nhistory=\"$6\"\ntrap '' INT\n\
         printf '[{\"role\":\"user\",\"content\":\"changed\"}]' > \"$history\"\n\
         printf '{\"type\":\"text_delta\",\"text\":\"stuck\"}\\n' >> \"$events\"\n\
         sleep 30\n",
    );
    let mut app = app(&dir, &script);
    let before = vec![json!({"role": "user", "content": "earlier"}),
        json!({"role": "assistant", "content": "reply"})];
    app.history = before.clone();
    send(&mut app, "hang");
    assert!(poll_until(&mut app, Duration::from_secs(3), |app| {
        app.entries.iter().any(|e| e.role == Role::Assistant)
    }));
    let pressed = Instant::now();
    app.on_key(key(KeyCode::Esc));
    assert!(poll_until(&mut app, Duration::from_secs(5), |app| !app.running));
    let took = pressed.elapsed();
    assert!(took >= Duration::from_millis(2900) && took < Duration::from_secs(5), "{took:?}");
    assert_eq!(app.history, before);
    assert!(transcript(&app).contains("before the task"), "{}", transcript(&app));
}

// ── Q16 ──────────────────────────────────────────────────────────────────────

#[test]
fn stream_error_keeps_partial_text() {
    let home = tmp("q16");
    let events = vec![
        (0, message_start()),
        (0, text_block(0)),
        (0, text_delta(0, "half ")),
        (0, text_delta(0, "a reply")),
        (0, json!({"type": "error", "error": {"type": "overloaded_error", "message": "Overloaded"}})),
    ];
    let (base, _) = serve(vec![sse(&events)]);
    let events_file = home.join("events.jsonl");
    let history = home.join("history.json");
    fs::write(&history, "[]").unwrap();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ram-code"));
    command
        .args(["--task", "talk"])
        .arg("--events")
        .arg(&events_file)
        .arg("--history")
        .arg(&history)
        .current_dir(&home);
    isolated(&mut command, &home, &base);
    command.env("RAMIFY_ROOT", &home);
    command.status().unwrap();
    let events = events_of(&events_file);
    let partial: String = events
        .iter()
        .filter(|e| e["type"] == "text_delta")
        .filter_map(|e| e["text"].as_str())
        .collect();
    assert_eq!(partial, "half a reply");
    let error = events.iter().find(|e| e["type"] == "error").expect("an error event");
    assert!(error["text"].as_str().unwrap().contains("Overloaded"), "{error}");
    assert!(events.iter().any(|e| e["type"] == "finished"), "{events:?}");
    let saved: Vec<Value> = serde_json::from_str(&fs::read_to_string(&history).unwrap()).unwrap();
    assert_eq!(saved.last().unwrap()["content"][0]["text"], "half a reply", "{saved:?}");
}

// ── Q17 ──────────────────────────────────────────────────────────────────────

#[test]
fn headless_sigint_exits_130() {
    let home = tmp("q17");
    let events = vec![(0, message_start()), (0, text_block(0)), (0, text_delta(0, "start")),
        (20000, text_delta(0, "never"))];
    let (base, seen) = serve(vec![sse(&events)]);
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ram-code"));
    command
        .args(["--task", "long"])
        .current_dir(&home)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    isolated(&mut command, &home, &base);
    command.env("RAMIFY_ROOT", &home);
    let child = command.spawn().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    while seen.lock().unwrap().is_empty() && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    std::thread::sleep(Duration::from_millis(300));
    // SAFETY: SIGINT to the child this test started.
    unsafe {
        libc::kill(child.id() as i32, libc::SIGINT);
    }
    let started = Instant::now();
    let output = child.wait_with_output().unwrap();
    assert!(started.elapsed() < Duration::from_secs(3));
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stdout.contains("interrupted") || stderr.contains("interrupted"), "{stdout}\n{stderr}");
    assert_eq!(output.status.code(), Some(130), "{stdout}\n{stderr}");
}

// ── Q18 ──────────────────────────────────────────────────────────────────────

#[test]
fn max_tokens_defaults_per_provider() {
    let dir = tmp("q18");
    let keys: HashMap<String, String> = [
        ("ANTHROPIC_API_KEY", "a"),
        ("OPENAI_API_KEY", "o"),
        ("DEEPSEEK_API_KEY", "d"),
        ("RAM_CODE_MODEL", "m"),
    ]
    .iter()
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .collect();
    let providers = Providers::load(dir.clone()).unwrap();
    let tokens = |name: &str, vars: &HashMap<String, String>, providers: &Providers| {
        resolve(&providers.find(name).unwrap(), &|k| vars.get(k).cloned(), None)
            .unwrap()
            .config
            .max_tokens
    };
    assert_eq!(tokens("claude", &keys, &providers), 64_000);
    for name in ["deepseek", "openai", "env"] {
        assert_eq!(tokens(name, &keys, &providers), 8_192, "{name}");
    }
    fs::write(
        dir.join("providers.json"),
        json!({"providers": [{"name": "deepseek", "max_tokens": 16000}]}).to_string(),
    )
    .unwrap();
    let custom = Providers::load(dir.clone()).unwrap();
    assert_eq!(tokens("deepseek", &keys, &custom), 16_000);
    let mut forced = keys.clone();
    forced.insert("RAM_CODE_MAX_TOKENS".into(), "4000".into());
    assert_eq!(tokens("claude", &forced, &custom), 4_000);
    assert_eq!(tokens("deepseek", &forced, &custom), 4_000);
}

// ── Q19 ──────────────────────────────────────────────────────────────────────

#[test]
fn esc_without_a_task_does_nothing() {
    let dir = tmp("q19");
    let mut app = app(&dir, &dir.join("none"));
    app.input = "draft".into();
    let entries = app.entries.len();
    app.on_key(key(KeyCode::Esc));
    assert_eq!(app.screen, Screen::Main);
    assert_eq!(app.input, "draft");
    assert_eq!(app.entries.len(), entries);
    assert!(!app.quit && !app.running);
    app.on_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL));
    assert!(app.quit);
}

// ── audit ────────────────────────────────────────────────────────────────────

#[test]
fn sse_parser_handles_crlf_comments_bare_data_and_split_utf8() {
    use ram_code::stream::SseParser;
    let mut parser = SseParser::default();
    let text = "é".as_bytes();
    let mut events = Vec::new();
    events.extend(parser.push(b": keep-alive\r\nevent: a\r\ndata:{\"t\":\""));
    events.extend(parser.push(&text[..1]));
    events.extend(parser.push(&text[1..]));
    events.extend(parser.push(b"\"}\r\ndata: second\r\n\r\n"));
    assert_eq!(events.len(), 1, "{events:?}");
    assert_eq!(events[0].name.as_deref(), Some("a"));
    assert_eq!(events[0].data, "{\"t\":\"é\"}\nsecond");
}

#[test]
fn openai_name_split_across_fragments_with_id_first() {
    let delta = |d: Value| json!({"choices": [{"index": 0, "delta": d, "finish_reason": null}]});
    let events = vec![
        (0, delta(json!({"tool_calls": [{"index": 1, "id": "call_b", "type": "function",
            "function": {"name": "list_", "arguments": ""}}]}))),
        (0, delta(json!({"tool_calls": [{"index": 0, "id": "call_a", "type": "function",
            "function": {"name": "read", "arguments": "{\"pa"}}]}))),
        (0, delta(json!({"tool_calls": [{"index": 1, "function": {"name": "files", "arguments": "{}"}}]}))),
        (0, delta(json!({"tool_calls": [{"index": 0, "function": {"name": "_file", "arguments": "th\":\"a\"}"}}]}))),
        (0, json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "tool_calls"}]})),
    ];
    let (base, _seen) = serve(vec![chunks(&events)]);
    let reply = OpenAiLlm::new(config(&base)).stream(&json!({"model": "m", "messages": []}), &Cancel::new(), &mut |_| {});
    assert_eq!(reply.end, ReplyEnd::Complete);
    let content = reply.message["content"].as_array().unwrap();
    assert_eq!(content[0]["id"], "call_a");
    assert_eq!(content[0]["name"], "read_file");
    assert_eq!(content[0]["input"], json!({"path": "a"}));
    assert_eq!(content[1]["id"], "call_b");
    assert_eq!(content[1]["name"], "list_files");
}

#[test]
fn openai_stream_cut_before_the_end_is_an_error() {
    let delta = |d: Value| json!({"choices": [{"index": 0, "delta": d, "finish_reason": null}]});
    let cut = Reply {
        status: 200,
        content_type: "text/event-stream",
        pieces: vec![
            Piece(0, format!("data: {}\n\n", delta(json!({"content": "half an ans"})))),
        ],
    };
    let (base, _seen) = serve(vec![cut]);
    let reply = OpenAiLlm::new(config(&base)).stream(&json!({"model": "m", "messages": []}), &Cancel::new(), &mut |_| {});
    assert!(
        matches!(reply.end, ReplyEnd::Failed(_)),
        "a stream cut before finish_reason or [DONE] passed as complete: {:?}",
        reply.end
    );
    assert_eq!(reply.message["content"][0]["text"], "half an ans");
}

struct ThinkingOnly;

impl Llm for ThinkingOnly {
    fn complete(&self, _request: &Value) -> Result<Value, String> {
        Err("not used".into())
    }

    fn stream(&self, _request: &Value, _cancel: &Cancel, _on_text: &mut dyn FnMut(&str)) -> ram_code::agent::Reply {
        ram_code::agent::Reply {
            message: json!({"role": "assistant", "content": [
                {"type": "thinking", "thinking": "", "signature": "sig"}
            ]}),
            end: ReplyEnd::Interrupted,
        }
    }
}

#[test]
fn an_interrupt_after_thinking_only_leaves_no_thinking_only_message() {
    let root = tmp("audit-think");
    let turn = run_turn_cancellable(
        &ThinkingOnly,
        &root,
        &[],
        "go",
        &Hooked::none(&root),
        &Cancel::new(),
        &mut |_| {},
    );
    assert_eq!(turn.end, TurnEnd::Interrupted);
    for message in &turn.history {
        if message["role"] == "assistant" {
            let blocks = message["content"].as_array().cloned().unwrap_or_default();
            assert!(
                blocks.iter().any(|b| b["type"] == "text" || b["type"] == "tool_use"),
                "an assistant message holds only thinking: {message}"
            );
        }
    }
}

// ── review fixes ─────────────────────────────────────────────────────────────

fn history_fixture() -> Vec<Value> {
    vec![
        json!({"role": "user", "content": "earlier"}),
        json!({"role": "assistant", "content": [{"type": "text", "text": "reply"}]}),
    ]
}

#[test]
fn failed_reply_with_nothing_kept_restores_the_history() {
    let dir = tmp("r1");
    let (base, _) = serve(vec![error_reply(
        429,
        json!({"type": "error", "error": {"type": "rate_limit_error", "message": "slow down"}}),
    )]);
    let llm = AnthropicLlm::new(config(&base));
    let before = history_fixture();
    let turn = run_turn_cancellable(&llm, &dir, &before, "again", &Hooked::none(&dir), &Cancel::new(), &mut |_| {});
    assert!(matches!(turn.end, TurnEnd::Failed(_)), "{:?}", turn.end);
    assert_eq!(turn.history, before, "a retry would send the task twice");
}

/// A pid is gone once `kill(pid, 0)` fails.
fn gone(pid: i32, within: Duration) -> bool {
    let end = Instant::now() + within;
    while Instant::now() < end {
        // SAFETY: signal 0 only checks that the process exists.
        if unsafe { libc::kill(pid, 0) } != 0 {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    false
}

#[test]
fn stuck_worker_kill_also_stops_hooks() {
    let dir = tmp("r2");
    let pid_file = dir.join("hook.pid");
    let task_dir_file = dir.join("task.dir");
    let script = dir.join("worker.sh");
    // The worker ignores SIGINT and leaves a child in its own process group, as a hook is.
    executable(
        &script,
        &format!(
            "#!/bin/sh\nevents=\"$4\"\ntrap '' INT\n\
             dirname \"$events\" > '{dirfile}'\n\
             perl -e 'setpgrp(0,0); exec \"sleep\", \"60\"' &\n\
             echo $! > '{pidfile}'\n\
             printf '{{\"type\":\"text_delta\",\"text\":\"stuck\"}}\\n' >> \"$events\"\n\
             sleep 30\n",
            dirfile = task_dir_file.display(),
            pidfile = pid_file.display(),
        ),
    );
    let mut app = app(&dir, &script);
    send(&mut app, "hang");
    assert!(poll_until(&mut app, Duration::from_secs(3), |app| {
        app.entries.iter().any(|e| e.role == Role::Assistant)
    }));
    let pid: i32 = fs::read_to_string(&pid_file).unwrap().trim().parse().unwrap();
    let task_dir = PathBuf::from(fs::read_to_string(&task_dir_file).unwrap().trim());
    app.on_key(key(KeyCode::Esc));
    assert!(poll_until(&mut app, Duration::from_secs(5), |app| !app.running));
    let survived = !gone(pid, Duration::from_secs(1));
    if survived {
        // SAFETY: clean up the process this test started.
        unsafe { libc::kill(pid, libc::SIGKILL) };
    }
    assert!(!survived, "the hook's process group outlived the killed worker");
    assert!(!task_dir.exists(), "the stuck path left {}", task_dir.display());
}

fn worker_command(home: &Path, base: &str, events: &Path, history: &Path) -> std::process::Command {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ram-code"));
    command
        .args(["--task", "talk"])
        .arg("--events")
        .arg(events)
        .arg("--history")
        .arg(history)
        .current_dir(home);
    isolated(&mut command, home, base);
    command.env("RAMIFY_ROOT", home);
    command
}

fn wait_for(child: &mut std::process::Child, within: Duration) -> Option<std::process::ExitStatus> {
    let end = Instant::now() + within;
    while Instant::now() < end {
        if let Ok(Some(status)) = child.try_wait() {
            return Some(status);
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

#[test]
fn esc_before_the_provider_resolves_is_an_interrupt() {
    let home = tmp("r3");
    let start = home.join("start");
    fs::create_dir_all(&start).unwrap();
    let events = home.join("events.jsonl");
    let history = home.join("history.json");
    fs::write(&history, "[]").unwrap();
    // No `started` marker: the worker waits for the SessionStart snapshot, before any request.
    let mut command = worker_command(&home, "http://127.0.0.1:9", &events, &history);
    command.env("RAM_CODE_START", &start);
    let mut child = command.spawn().unwrap();
    std::thread::sleep(Duration::from_millis(300));
    // SAFETY: SIGINT to the child this test started.
    unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
    let status = wait_for(&mut child, Duration::from_secs(3)).expect("the worker did not stop");
    assert!(status.success(), "the worker died on the signal: {status:?}");
    let seen = events_of(&events);
    assert!(seen.iter().any(|e| e["type"] == "interrupted"), "{seen:?}");
    assert!(seen.iter().any(|e| e["type"] == "finished"), "{seen:?}");
}

#[test]
fn real_worker_handles_sigint_in_events_mode() {
    let home = tmp("r4");
    let stream = vec![(0, message_start()), (0, text_block(0)), (0, text_delta(0, "start")),
        (20000, text_delta(0, "never"))];
    let (base, _) = serve(vec![sse(&stream)]);
    let events = home.join("events.jsonl");
    let history = home.join("history.json");
    fs::write(&history, "[]").unwrap();
    let mut child = worker_command(&home, &base, &events, &history).spawn().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    while !events_of(&events).iter().any(|e| e["type"] == "text_delta") && Instant::now() < end {
        std::thread::sleep(Duration::from_millis(20));
    }
    // SAFETY: SIGINT to the child this test started.
    unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
    let status = wait_for(&mut child, Duration::from_secs(3)).expect("the worker did not stop");
    assert!(status.success(), "{status:?}");
    let seen = events_of(&events);
    assert!(seen.iter().any(|e| e["type"] == "interrupted"), "{seen:?}");
    assert!(seen.iter().any(|e| e["type"] == "finished"), "{seen:?}");
    let saved: Vec<Value> = serde_json::from_str(&fs::read_to_string(&history).unwrap()).unwrap();
    anthropic_valid(&saved).unwrap();
    assert_eq!(saved.last().unwrap()["content"][0]["text"], "start", "{saved:?}");
}

#[test]
fn whitespace_only_partial_text_is_dropped() {
    let events = vec![(0, message_start()), (0, text_block(0)), (0, text_delta(0, "\n"))];
    let (base, _) = serve(vec![sse(&events)]);
    let llm = AnthropicLlm::new(config(&base));
    let reply = llm.stream(&json!({"messages": []}), &Cancel::new(), &mut |_| {});
    assert!(matches!(reply.end, ReplyEnd::Failed(_)), "{:?}", reply.end);
    assert_eq!(reply.message["content"], json!([]), "{}", reply.message);
}

#[test]
fn second_ctrl_c_kills_a_headless_run() {
    let home = tmp("r6");
    // A slow UserPromptSubmit hook: the first Ctrl+C cannot stop it.
    fs::create_dir_all(home.join(".ram")).unwrap();
    fs::write(
        home.join(".ram/settings.json"),
        json!({"hooks": {"UserPromptSubmit": [{"matcher": "", "hooks": [{"type": "command", "command": "sleep 20"}]}]}})
            .to_string(),
    )
    .unwrap();
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_ram-code"));
    command
        .args(["--task", "long"])
        .current_dir(&home)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    isolated(&mut command, &home, "http://127.0.0.1:9");
    command.env("RAMIFY_ROOT", &home);
    let mut child = command.spawn().unwrap();
    std::thread::sleep(Duration::from_millis(800));
    // SAFETY: SIGINT to the child this test started.
    unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
    std::thread::sleep(Duration::from_millis(300));
    unsafe { libc::kill(child.id() as i32, libc::SIGINT) };
    let status = wait_for(&mut child, Duration::from_secs(2));
    assert!(status.is_some(), "a second Ctrl+C did not end the run");
}

#[test]
fn server_tool_use_input_is_parsed() {
    let mut events = vec![
        (0, message_start()),
        (0, json!({"type": "content_block_start", "index": 0,
            "content_block": {"type": "server_tool_use", "id": "srvtoolu_1", "name": "web_search", "input": {}}})),
        (0, json_delta(0, "{\"query\":")),
        (0, json_delta(0, "\"rust\"}")),
        (0, block_stop(0)),
    ];
    events.extend(message_end("end_turn").into_iter().map(|e| (0, e)));
    let (base, _) = serve(vec![sse(&events)]);
    let llm = AnthropicLlm::new(config(&base));
    let reply = llm.stream(&json!({"messages": []}), &Cancel::new(), &mut |_| {});
    assert_eq!(reply.end, ReplyEnd::Complete);
    assert_eq!(reply.message["content"][0]["input"], json!({"query": "rust"}));
}

#[test]
fn sse_parser_is_linear_on_a_long_line() {
    let mut parser = ram_code::stream::SseParser::default();
    let chunk = vec![b'x'; 8192];
    let started = Instant::now();
    assert!(parser.push(b"data: ").is_empty());
    for _ in 0..1024 {
        assert!(parser.push(&chunk).is_empty());
    }
    let events = parser.push(b"\n\n");
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].data.len(), 8 * 1024 * 1024);
    assert!(started.elapsed() < Duration::from_millis(1500), "{:?}", started.elapsed());
}

#[test]
fn openai_idle_after_finish_is_complete() {
    let reply = Reply {
        status: 200,
        content_type: "text/event-stream",
        pieces: vec![
            Piece(0, format!("data: {}\n\n", json!({"choices": [{"index": 0, "delta": {"content": "done"}, "finish_reason": null}]}))),
            Piece(0, format!("data: {}\n\n", json!({"choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]}))),
            // The server holds the connection open with no [DONE].
            Piece(3000, String::new()),
        ],
    };
    let (base, _) = serve(vec![reply]);
    let mut cfg = config(&base);
    cfg.idle_timeout = 1;
    let llm = OpenAiLlm::new(cfg);
    let reply = llm.stream(&json!({"messages": []}), &Cancel::new(), &mut |_| {});
    assert_eq!(reply.end, ReplyEnd::Complete, "{:?}", reply.end);
    assert_eq!(reply.message["content"][0]["text"], "done");
}

#[test]
fn openai_error_null_is_not_an_error() {
    let (base, _) = serve(vec![chunks(&[
        (0, json!({"error": null, "choices": [{"index": 0, "delta": {"content": "fine"}, "finish_reason": null}]})),
        (0, json!({"error": null, "choices": [{"index": 0, "delta": {}, "finish_reason": "stop"}]})),
    ])]);
    let llm = OpenAiLlm::new(config(&base));
    let reply = llm.stream(&json!({"messages": []}), &Cancel::new(), &mut |_| {});
    assert_eq!(reply.end, ReplyEnd::Complete, "{:?}", reply.end);
    assert_eq!(reply.message["content"][0]["text"], "fine");
}

#[test]
fn max_tokens_too_large_retries_with_the_model_limit() {
    let dir = tmp("r8");
    let (base, seen) = serve(vec![
        error_reply(400, json!({"type": "error", "error": {"type": "invalid_request_error",
            "message": "max_tokens: 64000 > 8192, which is the maximum allowed number of output tokens for m"}})),
        anthropic_text("ok"),
    ]);
    let mut cfg = config(&base);
    cfg.max_tokens = 64000;
    let llm = AnthropicLlm::new(cfg);
    let mut notes = Vec::new();
    let mut limits = Vec::new();
    let turn = run_turn_cancellable(&llm, &dir, &[], "hi", &Hooked::none(&dir), &Cancel::new(), &mut |event| {
        match event {
            AgentEvent::Hook(note) => notes.push(note),
            AgentEvent::MaxTokens(limit) => limits.push(limit),
            _ => {}
        }
    });
    assert_eq!(turn.end, TurnEnd::Done, "{:?}", turn.end);
    assert_eq!(turn.answer, "ok");
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].body["max_tokens"], 64000);
    assert_eq!(seen[1].body["max_tokens"], 8192);
    assert_eq!(limits, vec![8192]);
    assert!(notes.iter().any(|n| n.contains("8192")), "{notes:?}");
}

#[test]
fn max_tokens_limit_is_kept_for_the_session() {
    let dir = tmp("r8b");
    let seen = dir.join("env.txt");
    let script = dir.join("worker.sh");
    executable(
        &script,
        &format!(
            "#!/bin/sh\nevents=\"$4\"\nhistory=\"$6\"\n\
             printf '%s\\n' \"$RAM_CODE_MAX_TOKENS\" >> '{seen}'\n\
             printf '{{\"type\":\"max_tokens\",\"value\":8192}}\\n' >> \"$events\"\n\
             printf '[]' > \"$history\"\n\
             printf '{{\"type\":\"finished\",\"answer\":\"\"}}\\n' >> \"$events\"\n",
            seen = seen.display()
        ),
    );
    let mut app = app(&dir, &script);
    for task in ["one", "two"] {
        send(&mut app, task);
        assert!(poll_until(&mut app, Duration::from_secs(3), |app| !app.running));
    }
    let lines: Vec<String> = fs::read_to_string(&seen).unwrap().lines().map(String::from).collect();
    assert_eq!(lines, vec!["".to_string(), "8192".to_string()]);
}

#[test]
fn redraw_only_when_something_changed() {
    use ram_code::app::needs_redraw;
    assert!(needs_redraw(true, Duration::ZERO, false));
    assert!(!needs_redraw(false, Duration::from_millis(100), true));
    assert!(needs_redraw(false, Duration::from_millis(300), true), "the spinner stops");
    assert!(!needs_redraw(false, Duration::from_millis(500), false));
    assert!(needs_redraw(false, Duration::from_secs(2), false));

    let dir = tmp("r9");
    let script = dir.join("worker.sh");
    executable(&script, "#!/bin/sh\nexit 0\n");
    let mut app = app(&dir, &script);
    app.take_dirty();
    app.poll_events();
    assert!(!app.take_dirty(), "a poll with nothing new asked for a redraw");
    app.on_key(key(KeyCode::Char('x')));
    assert!(app.take_dirty(), "a key press did not ask for a redraw");
}

#[test]
fn long_streaming_entry_shows_its_tail() {
    use ram_code::app::entry_lines;
    let text: String = (0..100).map(|i| format!("line {i}\n")).collect();
    let streaming = entry_lines(&text, true);
    assert_eq!(streaming.len(), 40);
    assert_eq!(streaming.last().copied(), Some("line 99"));
    let done = entry_lines(&text, false);
    assert_eq!(done.len(), 40);
    assert_eq!(done.first().copied(), Some("line 0"));
}

fn thinking_block(index: u64) -> Vec<(u64, Value)> {
    vec![
        (0, json!({"type": "content_block_start", "index": index, "content_block": {"type": "thinking", "thinking": ""}})),
        (0, json!({"type": "content_block_delta", "index": index, "delta": {"type": "thinking_delta", "thinking": "plan"}})),
        (0, json!({"type": "content_block_delta", "index": index, "delta": {"type": "signature_delta", "signature": "sig"}})),
        (0, block_stop(index)),
    ]
}

#[test]
fn stopped_turn_without_tool_use_drops_thinking() {
    let dir = tmp("rt");
    // Thinking, then text that the user stops part way.
    let mut events = vec![(0, message_start())];
    events.extend(thinking_block(0));
    events.extend([(0, text_block(1)), (0, text_delta(1, "half")), (20000, text_delta(1, "never"))]);
    let (base, _) = serve(vec![sse(&events)]);
    let llm = AnthropicLlm::new(config(&base));
    let cancel = Cancel::new();
    let stop = cancel.clone();
    let turn = run_turn_cancellable(&llm, &dir, &[], "go", &Hooked::none(&dir), &cancel, &mut |event| {
        if matches!(event, AgentEvent::TextDelta(_)) {
            stop.cancel();
        }
    });
    assert_eq!(turn.end, TurnEnd::Interrupted);
    let last = turn.history.last().unwrap();
    assert_eq!(last["content"], json!([{"type": "text", "text": "half"}]), "{last}");

    // A kept tool_use keeps its thinking, which must come before it.
    let mut events = vec![(0, message_start())];
    events.extend(thinking_block(0));
    events.extend([(0, tool_block(1, "toolu_1", "list_files")), (0, json_delta(1, "{}")), (0, block_stop(1)),
        (0, text_block(2)), (0, text_delta(2, "and")), (20000, text_delta(2, "never"))]);
    let (base, _) = serve(vec![sse(&events)]);
    let llm = AnthropicLlm::new(config(&base));
    let cancel = Cancel::new();
    let stop = cancel.clone();
    let turn = run_turn_cancellable(&llm, &dir, &[], "go", &Hooked::none(&dir), &cancel, &mut |event| {
        if matches!(event, AgentEvent::TextDelta(_)) {
            stop.cancel();
        }
    });
    assert_eq!(turn.end, TurnEnd::Interrupted);
    let assistant = &turn.history[turn.history.len() - 2];
    assert_eq!(assistant["content"][0]["type"], "thinking", "{assistant}");
    assert_eq!(assistant["content"][1]["type"], "tool_use", "{assistant}");
}
