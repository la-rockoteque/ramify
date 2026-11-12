//! The cases of `.ramify/work/ram-code-providers/qa-plan.md` that the build settles.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ram_code::agent::{auth_headers, messages_url, run_task, AgentConfig, Credential};
use ram_code::app::{App, MessageKind, Role, Screen};
use ram_code::agent::strip_thinking;
use ram_code::openai::{chat_url, from_chat, to_chat};
use ram_code::providers::{resolve, Format, Providers};
use ram_code::ramify::{CommandOutput, CommandSpec, Runner};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

// ── helpers ──────────────────────────────────────────────────────────────────

fn tmp(name: &str) -> PathBuf {
    // A task reads the user's AGENTS.md and CLAUDE.md: point it at an empty home. Every test
    // sets the same value, so the tests may run in parallel.
    let home = std::env::temp_dir().join(format!("ramcode-prov-{}-no-home", std::process::id()));
    std::env::set_var("RAM_CODE_INSTRUCTIONS_HOME", home);
    let dir = std::env::temp_dir().join(format!("ramcode-prov-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

type Env = HashMap<String, String>;

fn env(pairs: &[(&str, &str)]) -> Env {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn getter(map: &Env) -> impl Fn(&str) -> Option<String> + '_ {
    move |key| map.get(key).cloned()
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

/// An app whose environment is `vars` and whose `providers.json` lives in `dir`.
fn app(dir: &Path, vars: Env) -> App {
    let mut app = App::new(Arc::new(NoRunner), dir.to_path_buf());
    app.env = Arc::new(move |key| vars.get(key).cloned());
    app.entries.clear();
    app.load_providers(dir.to_path_buf());
    app.load();
    app
}

fn command(app: &mut App, text: &str) {
    app.input = text.to_string();
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
}

fn last_entry(app: &App) -> String {
    app.entries.last().map(|e| e.text.clone()).unwrap_or_default()
}

fn write_providers(dir: &Path, value: Value) {
    fs::write(dir.join("providers.json"), value.to_string()).unwrap();
}

fn saved(dir: &Path) -> Value {
    serde_json::from_str(&fs::read_to_string(dir.join("providers.json")).unwrap()).unwrap()
}

#[derive(Debug, Clone)]
struct Seen {
    path: String,
    headers: HashMap<String, String>,
    body: Value,
}

/// A local HTTP server that answers each request with the next reply and records it.
fn serve(replies: Vec<Value>) -> (String, Arc<Mutex<Vec<Seen>>>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let record = seen.clone();
    std::thread::spawn(move || {
        for (stream, reply) in listener.incoming().zip(replies) {
            let mut stream = stream.unwrap();
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
                headers,
                body: serde_json::from_slice(&body).unwrap_or(Value::Null),
            });
            let text = reply.to_string();
            let _ = write!(
                stream,
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{text}",
                text.len()
            );
        }
    });
    (base, seen)
}

// ── Q1 ───────────────────────────────────────────────────────────────────────

#[test]
fn no_providers_file_keeps_env_behaviour() {
    let dir = tmp("q1");
    let vars = env(&[
        ("ANTHROPIC_AUTH_TOKEN", "proxy-token"),
        ("ANTHROPIC_BASE_URL", "http://127.0.0.1:15721"),
    ]);
    let app = app(&dir, vars);
    assert_eq!(app.provider, "env");
    let config = app.resolve_config().expect("config");
    // The values #53 sends for a proxy token.
    assert_eq!(config.base_url, "http://127.0.0.1:15721");
    assert_eq!(config.credential, Credential::AuthToken("proxy-token".into()));
    assert_eq!(config.model, "deepseek-v4.1-flash");
    assert_eq!(config.max_tokens, 8192);
    assert_eq!(
        auth_headers(&config.credential).unwrap(),
        vec![
            ("authorization", "Bearer proxy-token".to_string()),
            ("x-api-key", "proxy-token".to_string()),
        ]
    );
}

// ── Q3 ───────────────────────────────────────────────────────────────────────

#[test]
fn provider_list_marks_active_and_missing_keys() {
    let dir = tmp("q3");
    write_providers(
        &dir,
        json!({"providers": [{
            "name": "litellm", "format": "openai", "base_url": "http://llm.local",
            "key_env": "LITELLM_KEY", "model": "gpt-x"
        }]}),
    );
    let mut app = app(&dir, env(&[("DEEPSEEK_API_KEY", "ds")]));
    command(&mut app, "/provider");
    let list = last_entry(&app);
    for name in ["claude", "openai", "deepseek", "env", "litellm"] {
        assert!(list.contains(name), "{name} is not listed:\n{list}");
    }
    let line = |name: &str| {
        list.lines()
            .find(|line| line.split_whitespace().any(|word| word == name))
            .unwrap_or_default()
            .to_string()
    };
    assert!(line("env").starts_with('●'), "env is not marked active:\n{list}");
    assert!(line("openai").contains("no key: OPENAI_API_KEY"), "{list}");
    assert!(line("litellm").contains("no key: LITELLM_KEY"), "{list}");
    assert!(!line("deepseek").contains("no key"), "{list}");
}

// ── Q4 ───────────────────────────────────────────────────────────────────────

#[test]
fn switching_to_deepseek_targets_its_endpoint() {
    let dir = tmp("q4");
    let mut app = app(&dir, env(&[("DEEPSEEK_API_KEY", "ds-key")]));
    command(&mut app, "/provider deepseek");
    assert_eq!(app.provider, "deepseek");
    let config = app.resolve_config().expect("config");
    assert_eq!(
        messages_url(&config.base_url),
        "https://api.deepseek.com/anthropic/v1/messages"
    );
    assert_eq!(
        auth_headers(&config.credential).unwrap(),
        vec![("x-api-key", "ds-key".to_string())]
    );
    assert_eq!(config.model, "deepseek-v4.1-flash");
}

// ── Q5 ───────────────────────────────────────────────────────────────────────

#[test]
fn switch_is_saved_for_the_next_session() {
    let dir = tmp("q5");
    let vars = env(&[("DEEPSEEK_API_KEY", "ds")]);
    let mut first = app(&dir, vars.clone());
    command(&mut first, "/provider deepseek");
    assert_eq!(saved(&dir)["active"], "deepseek");
    let second = app(&dir, vars);
    assert_eq!(second.provider, "deepseek");
}

// ── Q6 ───────────────────────────────────────────────────────────────────────

#[test]
fn custom_provider_uses_its_url_key_and_model() {
    let dir = tmp("q6");
    let (base, seen) = serve(vec![json!({
        "content": [{"type": "text", "text": "hi from litellm"}],
        "stop_reason": "end_turn"
    })]);
    write_providers(
        &dir,
        json!({"active": "litellm", "providers": [{
            "name": "litellm", "base_url": format!("{base}/proxy"),
            "key_env": "LITELLM_KEY", "model": "team-model"
        }]}),
    );
    let vars = env(&[("LITELLM_KEY", "lk")]);
    let providers = Providers::load(dir.clone()).unwrap();
    let provider = providers.find("litellm").unwrap();
    assert_eq!(provider.format, Format::Anthropic);
    let resolved = resolve(&provider, &getter(&vars), None).unwrap();
    let llm = resolved.llm();
    let answer = run_task(llm.as_ref(), &dir, "hello", &mut |_| {}).unwrap();
    assert_eq!(answer, "hi from litellm");
    let seen = seen.lock().unwrap();
    assert_eq!(seen[0].path, "/proxy/v1/messages");
    assert_eq!(seen[0].headers.get("x-api-key").map(String::as_str), Some("lk"));
    assert_eq!(seen[0].body["model"], "team-model");
}

// ── Q7 ───────────────────────────────────────────────────────────────────────

#[test]
fn custom_entry_overrides_a_preset() {
    let dir = tmp("q7");
    write_providers(
        &dir,
        json!({"providers": [{
            "name": "deepseek", "base_url": "http://deepseek.mirror",
            "key_env": "DEEPSEEK_API_KEY", "model": "deepseek-v4.1-flash"
        }]}),
    );
    let providers = Providers::load(dir).unwrap();
    let all = providers.all();
    let deepseek: Vec<_> = all.iter().filter(|p| p.name == "deepseek").collect();
    assert_eq!(deepseek.len(), 1, "deepseek is listed twice");
    assert_eq!(deepseek[0].base_url.as_deref(), Some("http://deepseek.mirror"));
}

// ── Q8 ───────────────────────────────────────────────────────────────────────

#[test]
fn openai_request_maps_tools_to_functions() {
    let request = json!({
        "model": "gpt-x",
        "max_tokens": 100,
        "system": "be brief",
        "tools": [{"name": "read_file", "description": "read", "input_schema": {"type": "object"}}],
        "messages": [
            {"role": "user", "content": "read a.txt"},
            {"role": "assistant", "content": [
                {"type": "thinking", "thinking": "", "signature": "s"},
                {"type": "text", "text": "reading"},
                {"type": "tool_use", "id": "call_1", "name": "read_file", "input": {"path": "a.txt"}}
            ]},
            {"role": "user", "content": [
                {"type": "tool_result", "tool_use_id": "call_1", "content": "file body"}
            ]}
        ]
    });
    let chat = to_chat(&request);
    assert_eq!(chat["model"], "gpt-x");
    assert_eq!(chat["max_completion_tokens"], 100);
    let messages = chat["messages"].as_array().unwrap();
    assert_eq!(messages[0], json!({"role": "system", "content": "be brief"}));
    assert_eq!(messages[1], json!({"role": "user", "content": "read a.txt"}));
    assert_eq!(messages[2]["role"], "assistant");
    assert_eq!(messages[2]["content"], "reading");
    let call = &messages[2]["tool_calls"][0];
    assert_eq!(call["id"], "call_1");
    assert_eq!(call["type"], "function");
    assert_eq!(call["function"]["name"], "read_file");
    let arguments: Value =
        serde_json::from_str(call["function"]["arguments"].as_str().unwrap()).unwrap();
    assert_eq!(arguments, json!({"path": "a.txt"}));
    assert_eq!(
        messages[3],
        json!({"role": "tool", "tool_call_id": "call_1", "content": "file body"})
    );
    assert_eq!(
        chat["tools"][0],
        json!({"type": "function", "function": {
            "name": "read_file", "description": "read", "parameters": {"type": "object"}
        }})
    );
}

// ── Q9 ───────────────────────────────────────────────────────────────────────

#[test]
fn openai_reply_maps_back_to_content_blocks() {
    let reply = json!({"choices": [{
        "finish_reason": "tool_calls",
        "message": {
            "role": "assistant",
            "content": "two calls",
            "tool_calls": [
                {"id": "c1", "type": "function", "function": {"name": "read_file", "arguments": "{\"path\":\"a\"}"}},
                {"id": "c2", "type": "function", "function": {"name": "list_files", "arguments": "{}"}}
            ]
        }
    }]});
    let message = from_chat(&reply);
    assert_eq!(message["stop_reason"], "tool_use");
    let content = message["content"].as_array().unwrap();
    assert_eq!(content.len(), 3);
    assert_eq!(content[0], json!({"type": "text", "text": "two calls"}));
    assert_eq!(
        content[1],
        json!({"type": "tool_use", "id": "c1", "name": "read_file", "input": {"path": "a"}})
    );
    assert_eq!(
        content[2],
        json!({"type": "tool_use", "id": "c2", "name": "list_files", "input": {}})
    );
}

// ── Q10 ──────────────────────────────────────────────────────────────────────

#[test]
fn openai_task_runs_a_tool_end_to_end() {
    let dir = tmp("q10");
    fs::write(dir.join("a.txt"), "alpha").unwrap();
    let (base, seen) = serve(vec![
        json!({"choices": [{"finish_reason": "tool_calls", "message": {
            "role": "assistant", "content": null,
            "tool_calls": [{"id": "call_9", "type": "function",
                "function": {"name": "read_file", "arguments": "{\"path\":\"a.txt\"}"}}]
        }}]}),
        json!({"choices": [{"finish_reason": "stop", "message": {
            "role": "assistant", "content": "a.txt says alpha"
        }}]}),
    ]);
    write_providers(
        &dir,
        json!({"providers": [{
            "name": "gpt", "format": "openai", "base_url": base,
            "key_env": "GPT_KEY", "model": "gpt-x"
        }]}),
    );
    let vars = env(&[("GPT_KEY", "sk-gpt")]);
    let providers = Providers::load(dir.clone()).unwrap();
    let resolved = resolve(&providers.find("gpt").unwrap(), &getter(&vars), None).unwrap();
    let llm = resolved.llm();
    let mut results = Vec::new();
    let answer = run_task(llm.as_ref(), &dir, "what does a.txt say", &mut |event| {
        if let ram_code::agent::AgentEvent::ToolResult(text) = event {
            results.push(text);
        }
    })
    .unwrap();
    assert_eq!(answer, "a.txt says alpha");
    assert_eq!(results, vec!["alpha".to_string()]);
    let seen = seen.lock().unwrap();
    assert_eq!(seen.len(), 2);
    assert_eq!(seen[0].path, "/v1/chat/completions");
    assert_eq!(
        seen[0].headers.get("authorization").map(String::as_str),
        Some("Bearer sk-gpt")
    );
    let second = seen[1].body["messages"].as_array().unwrap();
    let tool = second.iter().find(|m| m["role"] == "tool").expect("a tool message");
    assert_eq!(tool["tool_call_id"], "call_9");
    assert_eq!(tool["content"], "alpha");
}

// ── Q11 ──────────────────────────────────────────────────────────────────────

#[test]
fn openai_bad_arguments_become_a_tool_error() {
    let dir = tmp("q11");
    let (base, _seen) = serve(vec![
        json!({"choices": [{"finish_reason": "tool_calls", "message": {
            "role": "assistant", "content": null,
            "tool_calls": [{"id": "call_1", "type": "function",
                "function": {"name": "list_files", "arguments": "{not json"}}]
        }}]}),
        json!({"choices": [{"finish_reason": "stop", "message": {"role": "assistant", "content": "sorry"}}]}),
    ]);
    let config = AgentConfig {
        base_url: base,
        credential: Credential::ApiKey("k".into()),
        model: "gpt-x".into(),
        max_tokens: 10,
    };
    let llm = ram_code::openai::OpenAiLlm::new(config);
    let mut results = Vec::new();
    let answer = run_task(&llm, &dir, "list", &mut |event| {
        if let ram_code::agent::AgentEvent::ToolResult(text) = event {
            results.push(text);
        }
    })
    .unwrap();
    assert_eq!(answer, "sorry");
    assert_eq!(results.len(), 1);
    assert!(
        results[0].starts_with("error: the tool arguments are not valid JSON"),
        "{results:?}"
    );
}

// ── Q12 ──────────────────────────────────────────────────────────────────────

#[test]
fn unknown_provider_keeps_the_active_one() {
    let dir = tmp("q12");
    write_providers(&dir, json!({"active": "claude"}));
    let mut app = app(&dir, env(&[]));
    command(&mut app, "/provider nope");
    assert_eq!(app.provider, "claude");
    assert_eq!(saved(&dir)["active"], "claude");
    let entry = app.entries.last().unwrap();
    assert_eq!(entry.role, Role::Error);
    assert!(entry.text.contains("nope"), "{}", entry.text);
    for name in ["claude", "openai", "deepseek", "env"] {
        assert!(entry.text.contains(name), "{name} missing: {}", entry.text);
    }
}

// ── Q13 ──────────────────────────────────────────────────────────────────────

#[test]
fn missing_key_names_the_variable() {
    let dir = tmp("q13");
    write_providers(&dir, json!({"active": "openai"}));
    let mut app = app(&dir, env(&[]));
    command(&mut app, "do something");
    assert_eq!(app.screen, Screen::Message);
    assert_eq!(app.message_kind, MessageKind::Failure);
    assert!(app.message.contains("OPENAI_API_KEY"), "{}", app.message);
    assert!(!app.running);
    app.back();
    assert_eq!(app.screen, Screen::Main);
}

// ── Q14 ──────────────────────────────────────────────────────────────────────

#[test]
fn unreadable_providers_file_is_an_error_not_a_crash() {
    let dir = tmp("q14");
    fs::write(dir.join("providers.json"), "{not json").unwrap();
    let app = app(&dir, env(&[]));
    assert_eq!(app.provider, "env");
    assert!(
        app.entries
            .iter()
            .any(|e| e.role == Role::Error && e.text.contains("providers.json")),
        "no error names the file: {:?}",
        app.entries.iter().map(|e| &e.text).collect::<Vec<_>>()
    );
}

// ── Q15 ──────────────────────────────────────────────────────────────────────

#[test]
fn env_var_overrides_saved_provider_for_one_run() {
    let dir = tmp("q15");
    write_providers(&dir, json!({"active": "deepseek"}));
    let app = app(&dir, env(&[("RAM_CODE_PROVIDER", "openai")]));
    assert_eq!(app.provider, "openai");
    assert_eq!(saved(&dir)["active"], "deepseek");
}

// ── Q16 ──────────────────────────────────────────────────────────────────────

#[test]
fn model_command_sets_the_session_model() {
    let dir = tmp("q16");
    let mut app = app(&dir, env(&[("DEEPSEEK_API_KEY", "ds")]));
    command(&mut app, "/provider deepseek");
    let before = fs::read_to_string(dir.join("providers.json")).unwrap();
    command(&mut app, "/model deepseek-v4.1-pro");
    assert_eq!(app.resolve_config().unwrap().model, "deepseek-v4.1-pro");
    assert_eq!(fs::read_to_string(dir.join("providers.json")).unwrap(), before);
}

// ── Q17 ──────────────────────────────────────────────────────────────────────

#[test]
fn providers_file_never_holds_a_key() {
    let dir = tmp("q17");
    let keys = [
        ("ANTHROPIC_API_KEY", "secret-anthropic"),
        ("OPENAI_API_KEY", "secret-openai"),
        ("DEEPSEEK_API_KEY", "secret-deepseek"),
    ];
    let mut app = app(&dir, env(&keys));
    for name in ["claude", "openai", "deepseek", "env"] {
        command(&mut app, &format!("/provider {name}"));
        let text = fs::read_to_string(dir.join("providers.json")).unwrap();
        for (_, secret) in keys {
            assert!(!text.contains(secret), "{secret} written after /provider {name}");
        }
    }
}

// ── review fixes ─────────────────────────────────────────────────────────────

#[test]
fn partial_override_merges_with_the_preset() {
    let dir = tmp("r1");
    write_providers(&dir, json!({"providers": [{"name": "openai", "model": "gpt-team"}]}));
    let vars = env(&[("OPENAI_API_KEY", "sk-o"), ("ANTHROPIC_API_KEY", "sk-ant")]);
    let providers = Providers::load(dir).unwrap();
    let provider = providers.find("openai").unwrap();
    assert_eq!(provider.format, Format::OpenAi);
    let resolved = resolve(&provider, &getter(&vars), None).unwrap();
    assert_eq!(resolved.config.base_url, "https://api.openai.com/v1");
    assert_eq!(resolved.config.credential, Credential::ApiKey("sk-o".into()));
    assert_eq!(resolved.config.model, "gpt-team");
}

#[test]
fn a_provider_without_key_env_never_gets_the_anthropic_credential() {
    let dir = tmp("r2");
    write_providers(
        &dir,
        json!({"providers": [
            {"name": "elsewhere", "base_url": "https://llm.example.com", "model": "m"},
            {"name": "claude", "base_url": "https://llm.example.com"}
        ]}),
    );
    let vars = env(&[("ANTHROPIC_API_KEY", "sk-ant")]);
    let providers = Providers::load(dir).unwrap();
    for name in ["elsewhere", "claude"] {
        let error = resolve(&providers.find(name).unwrap(), &getter(&vars), None)
            .expect_err("an Anthropic key must not go to another host");
        assert!(error.contains("key_env"), "{name}: {error}");
    }
}

#[test]
fn a_custom_base_url_must_be_https() {
    let dir = tmp("r3");
    write_providers(
        &dir,
        json!({"providers": [{"name": "plain", "base_url": "http://llm.example.com",
            "key_env": "K", "model": "m"}]}),
    );
    let vars = env(&[("K", "secret")]);
    let providers = Providers::load(dir).unwrap();
    let error = resolve(&providers.find("plain").unwrap(), &getter(&vars), None).unwrap_err();
    assert!(error.contains("https"), "{error}");
}

#[test]
fn worker_gets_the_active_provider_and_model() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tmp("r4");
    let seen = dir.join("worker-env.txt");
    let script = dir.join("worker.sh");
    fs::write(
        &script,
        format!(
            "#!/bin/sh\nprintf '%s|%s|%s' \"$RAM_CODE_PROVIDER\" \"$RAM_CODE_CONFIG_DIR\" \"$RAM_CODE_MODEL\" > '{}'\n\
             printf '[]' > \"$6\"\nprintf '{{\"type\":\"finished\",\"answer\":\"ok\"}}\\n' >> \"$4\"\n",
            seen.display()
        ),
    )
    .unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    let mut app = app(&dir, env(&[("DEEPSEEK_API_KEY", "ds")]));
    app.worker_binary = Some(script);
    command(&mut app, "/provider deepseek");
    command(&mut app, "/model deepseek-v4.1-pro");
    command(&mut app, "do it");
    for _ in 0..300 {
        app.poll_events();
        if !app.running {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(!app.running, "the worker never finished");
    assert_eq!(
        fs::read_to_string(&seen).unwrap(),
        format!("deepseek|{}|deepseek-v4.1-pro", dir.display())
    );
}

#[test]
fn openai_empty_reply_is_an_error() {
    let reply = json!({"choices": [{"finish_reason": "content_filter",
        "message": {"role": "assistant", "content": ""}}]});
    let message = from_chat(&reply);
    let error = message["error"]["message"].as_str().expect("an error");
    assert!(error.contains("content_filter"), "{error}");
}

#[test]
fn openai_refusal_becomes_text() {
    let reply = json!({"choices": [{"finish_reason": "stop",
        "message": {"role": "assistant", "content": null, "refusal": "not allowed"}}]});
    let message = from_chat(&reply);
    assert_eq!(message["content"][0]["type"], "text");
    assert!(message["content"][0]["text"].as_str().unwrap().contains("not allowed"));
}

#[test]
fn missing_tool_call_id_gets_one() {
    let reply = json!({"choices": [{"finish_reason": "tool_calls", "message": {
        "role": "assistant", "content": null,
        "tool_calls": [{"type": "function", "function": {"name": "list_files", "arguments": "{}"}}]
    }}]});
    let message = from_chat(&reply);
    let id = message["content"][0]["id"].as_str().expect("an id");
    assert!(id.starts_with("call_"), "{id}");
}

#[test]
fn chat_url_keeps_a_base_path() {
    assert_eq!(chat_url("http://127.0.0.1:4000"), "http://127.0.0.1:4000/v1/chat/completions");
    assert_eq!(chat_url("https://api.openai.com/v1"), "https://api.openai.com/v1/chat/completions");
    assert_eq!(
        chat_url("https://generativelanguage.googleapis.com/v1beta/openai"),
        "https://generativelanguage.googleapis.com/v1beta/openai/chat/completions"
    );
    assert_eq!(
        chat_url("https://api.openai.com/v1/chat/completions"),
        "https://api.openai.com/v1/chat/completions"
    );
}

#[test]
fn an_assistant_turn_with_only_text_never_sends_null_content() {
    let chat = to_chat(&json!({"model": "m", "messages": [
        {"role": "user", "content": "hi"},
        {"role": "assistant", "content": [{"type": "thinking", "thinking": "", "signature": "s"}]},
    ]}));
    assert_eq!(chat["messages"][1]["content"], "");
}

#[test]
fn switching_provider_strips_thinking_from_history() {
    let dir = tmp("r5");
    let mut app = app(&dir, env(&[("DEEPSEEK_API_KEY", "ds")]));
    app.history = vec![
        json!({"role": "user", "content": "hi"}),
        json!({"role": "assistant", "content": [
            {"type": "thinking", "thinking": "", "signature": "sig"},
            {"type": "redacted_thinking", "data": "x"},
            {"type": "text", "text": "hello"}
        ]}),
    ];
    command(&mut app, "/provider deepseek");
    assert_eq!(app.history[1]["content"], json!([{"type": "text", "text": "hello"}]));
    let mut same = app.history.clone();
    strip_thinking(&mut same);
    assert_eq!(same, app.history);
}

#[test]
fn save_keeps_a_file_it_cannot_read() {
    let dir = tmp("r6");
    let bytes = [0xff, 0xfe, 0x7b];
    fs::write(dir.join("providers.json"), bytes).unwrap();
    let mut providers = Providers::empty(dir.clone());
    assert!(providers.save_active("deepseek").is_err());
    assert_eq!(fs::read(dir.join("providers.json")).unwrap(), bytes);
}
