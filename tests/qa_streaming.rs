//! The cases of `.ramify/work/ram-code-streaming/qa-plan.md` that the build settles.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ram_code::agent::{
    run_task, run_turn_cancellable, AgentConfig, AgentEvent, AnthropicLlm, Credential, Llm,
    ReplyEnd, TurnEnd, WHOLE_REPLY_TIMEOUT_SECS,
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
use std::sync::atomic::Ordering;
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
    content_type: &'static str,
    pieces: Vec<Piece>,
}

fn sse(events: &[(u64, Value)]) -> Reply {
    Reply {
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
        content_type: "text/event-stream",
        pieces,
    }
}

fn whole(value: Value) -> Reply {
    Reply {
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
                    "HTTP/1.1 200 OK\r\ncontent-type: {}\r\nconnection: close\r\n\r\n",
                    reply.content_type
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
    // The whole-reply limit applies to a reply read whole. Lower it so the test is quick.
    WHOLE_REPLY_TIMEOUT_SECS.store(1, Ordering::SeqCst);
    let mut events = vec![(0, message_start()), (0, text_block(0))];
    for _ in 0..5 {
        events.push((500, text_delta(0, "x")));
    }
    events.push((0, block_stop(0)));
    events.extend(message_end("end_turn").into_iter().map(|e| (0, e)));
    let (base, _) = serve(vec![sse(&events)]);
    let mut cfg = config(&base);
    cfg.idle_timeout = 1;
    let llm = AnthropicLlm::new(cfg);
    let reply = llm.stream(&json!({"messages": []}), &Cancel::new(), &mut |_| {});
    WHOLE_REPLY_TIMEOUT_SECS.store(120, Ordering::SeqCst);
    assert_eq!(reply.end, ReplyEnd::Complete);
    assert_eq!(reply.message["content"][0]["text"], "xxxxx");
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
                Some("text") if block["text"].as_str().is_some_and(str::is_empty) => {
                    return Err("empty text block".into())
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
