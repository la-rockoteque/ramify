//! Ram Code reads AGENTS.md and CLAUDE.md into its system prompt.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ram_code::agent::{run_turn, AgentEvent, Llm, SYSTEM_PROMPT};
use ram_code::app::{App, Screen};
use ram_code::instructions::{load, FILE_LIMIT, TOTAL_LIMIT};
use ram_code::ramify::{CommandOutput, CommandSpec, Runner};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex, MutexGuard};

/// `RAM_CODE_INSTRUCTIONS_HOME` is process-wide: one test at a time.
static ENV: Mutex<()> = Mutex::new(());

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    home: PathBuf,
    repo: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let guard = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let base =
            std::env::temp_dir().join(format!("ram-instructions-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (home, repo) = (base.join("home"), base.join("repo"));
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&repo).unwrap();
        let ok = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&repo)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let home = home.canonicalize().unwrap();
        std::env::set_var("RAM_CODE_INSTRUCTIONS_HOME", &home);
        Fixture {
            _guard: guard,
            home,
            repo: repo.canonicalize().unwrap(),
        }
    }

    fn write(&self, path: &Path, text: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, text).unwrap();
    }

    /// The system text of one task that works in `root`.
    fn system_in(&self, root: &Path) -> (String, Vec<String>) {
        let llm = Recorder::default();
        let mut notes = Vec::new();
        let _ = run_turn(&llm, root, &[], "go", &mut |event| {
            if let AgentEvent::Hook(note) = event {
                notes.push(note);
            }
        });
        let requests = llm.requests.lock().unwrap();
        (requests[0]["system"].as_str().unwrap().to_string(), notes)
    }

    fn system(&self) -> String {
        self.system_in(&self.repo).0
    }
}

#[derive(Default)]
struct Recorder {
    requests: Mutex<Vec<Value>>,
}

impl Llm for Recorder {
    fn complete(&self, request: &Value) -> Result<Value, String> {
        self.requests.lock().unwrap().push(request.clone());
        Ok(json!({"content": [{"type": "text", "text": "done"}]}))
    }
    fn model(&self) -> &str {
        "test"
    }
    fn max_tokens(&self) -> u32 {
        10
    }
}

/// The places of `parts` in `text`, which must all be there, in that order.
fn in_order(text: &str, parts: &[&str]) {
    let mut from = 0;
    for part in parts {
        let at = text[from..]
            .find(part)
            .unwrap_or_else(|| panic!("{part:?} missing or out of order in:\n{text}"));
        from += at + part.len();
    }
}

#[test]
fn user_files_come_first() {
    let fixture = Fixture::new("user");
    fixture.write(&fixture.home.join(".ram/AGENTS.md"), "ram rule");
    fixture.write(&fixture.home.join(".claude/CLAUDE.md"), "claude rule");
    fixture.write(&fixture.repo.join("AGENTS.md"), "project rule");
    in_order(
        &fixture.system(),
        &[
            SYSTEM_PROMPT,
            "Instructions from ~/.ram/AGENTS.md:\n\nram rule",
            "Instructions from ~/.claude/CLAUDE.md:\n\nclaude rule",
            "Instructions from AGENTS.md:\n\nproject rule",
        ],
    );
}

#[test]
fn project_files_come_in_order() {
    let fixture = Fixture::new("project");
    for (file, text) in [
        ("CLAUDE.local.md", "four"),
        (".claude/CLAUDE.md", "three"),
        ("CLAUDE.md", "two"),
        ("AGENTS.md", "one"),
    ] {
        fixture.write(&fixture.repo.join(file), text);
    }
    in_order(
        &fixture.system(),
        &[
            "AGENTS.md:\n\none",
            "CLAUDE.md:\n\ntwo",
            ".claude/CLAUDE.md:\n\nthree",
            "CLAUDE.local.md:\n\nfour",
        ],
    );
}

#[test]
fn a_parent_folder_comes_first() {
    let fixture = Fixture::new("parent");
    fixture.write(&fixture.repo.join("AGENTS.md"), "top rule");
    fixture.write(&fixture.repo.join("sub/AGENTS.md"), "sub rule");
    let (system, _) = fixture.system_in(&fixture.repo.join("sub"));
    in_order(
        &system,
        &[
            "Instructions from AGENTS.md:\n\ntop rule",
            "Instructions from sub/AGENTS.md:\n\nsub rule",
        ],
    );
}

#[test]
fn a_linked_file_is_read_once() {
    let fixture = Fixture::new("link");
    fixture.write(&fixture.repo.join("AGENTS.md"), "shared rule");
    std::os::unix::fs::symlink("AGENTS.md", fixture.repo.join("CLAUDE.md")).unwrap();
    assert_eq!(fixture.system().matches("shared rule").count(), 1);
}

#[test]
fn an_import_brings_the_file_in() {
    let fixture = Fixture::new("import");
    fixture.write(&fixture.repo.join("docs/rules.md"), "RULES TEXT");
    fixture.write(
        &fixture.repo.join("CLAUDE.md"),
        "see @docs/rules.md.\nnot `@docs/rules.md` here\n```\n@docs/rules.md\n```\n",
    );
    let system = fixture.system();
    assert!(system.contains("see RULES TEXT."), "{system}");
    assert!(system.contains("not `@docs/rules.md` here"), "{system}");
    assert!(system.contains("```\n@docs/rules.md\n```"), "{system}");
}

#[test]
fn a_home_import_reads_from_home() {
    let fixture = Fixture::new("home-import");
    fixture.write(&fixture.home.join("shared.md"), "FROM HOME");
    fixture.write(&fixture.repo.join("CLAUDE.md"), "@~/shared.md");
    assert!(fixture.system().contains("FROM HOME"));
}

#[test]
fn an_import_loop_ends() {
    let fixture = Fixture::new("loop");
    fixture.write(&fixture.repo.join("a.md"), "TEXT A @b.md");
    fixture.write(&fixture.repo.join("b.md"), "TEXT B @a.md");
    fixture.write(&fixture.repo.join("CLAUDE.md"), "@a.md");
    let system = fixture.system();
    assert_eq!(
        (
            system.matches("TEXT A").count(),
            system.matches("TEXT B").count()
        ),
        (1, 1),
        "{system}"
    );
}

#[test]
fn imports_stop_at_depth_five() {
    let fixture = Fixture::new("depth");
    for index in 1..=6 {
        let next = if index < 6 {
            format!(" @f{}.md", index + 1)
        } else {
            String::new()
        };
        fixture.write(
            &fixture.repo.join(format!("f{index}.md")),
            &format!("TEXT{index}{next}"),
        );
    }
    fixture.write(&fixture.repo.join("CLAUDE.md"), "TEXT0 @f1.md");
    let system = fixture.system();
    for index in 0..=5 {
        assert!(
            system.contains(&format!("TEXT{index}")),
            "TEXT{index}: {system}"
        );
    }
    assert!(!system.contains("TEXT6"), "{system}");
    assert!(system.contains("@f6.md"), "{system}");
}

#[test]
fn a_missing_import_stays_as_text() {
    let fixture = Fixture::new("missing");
    fixture.write(&fixture.repo.join("CLAUDE.md"), "read @nope.md first");
    assert!(fixture.system().contains("read @nope.md first"));
}

#[test]
fn a_large_file_is_cut() {
    let fixture = Fixture::new("large");
    fixture.write(
        &fixture.repo.join("AGENTS.md"),
        &format!("{}TAIL", "x".repeat(40 * 1024)),
    );
    let system = fixture.system();
    assert!(
        system.contains(&format!("{}\n[cut at 32 KiB]", "x".repeat(FILE_LIMIT))),
        "no cut"
    );
    assert!(!system.contains("TAIL"));
}

#[test]
fn all_files_stop_at_128_kib() {
    let fixture = Fixture::new("total");
    for (index, file) in [
        "AGENTS.md",
        "CLAUDE.md",
        ".claude/CLAUDE.md",
        "CLAUDE.local.md",
        "sub/AGENTS.md",
    ]
    .iter()
    .enumerate()
    {
        fixture.write(
            &fixture.repo.join(file),
            &char::from(b'a' + index as u8).to_string().repeat(30 * 1024),
        );
    }
    let (system, _) = fixture.system_in(&fixture.repo.join("sub"));
    let part = &system[SYSTEM_PROMPT.len()..];
    assert!(part.len() <= TOTAL_LIMIT + 40, "{}", part.len());
    assert!(system.ends_with("[cut at 128 KiB]"));
}

#[test]
fn a_file_that_is_not_utf8_is_skipped() {
    let fixture = Fixture::new("binary");
    std::fs::write(fixture.repo.join("AGENTS.md"), [0xff, 0xfe, 0x00]).unwrap();
    fixture.write(&fixture.repo.join("CLAUDE.md"), "good rule");
    let (system, notes) = fixture.system_in(&fixture.repo);
    assert!(system.contains("good rule"));
    assert_eq!(notes, ["instructions: skipped AGENTS.md: not UTF-8"]);
}

#[test]
fn no_file_keeps_the_prompt() {
    let fixture = Fixture::new("none");
    assert_eq!(fixture.system(), SYSTEM_PROMPT);
}

#[test]
fn an_openai_provider_gets_the_instructions() {
    let fixture = Fixture::new("openai");
    fixture.write(&fixture.repo.join("AGENTS.md"), "openai rule");
    let request =
        json!({"model": "m", "max_tokens": 1, "system": fixture.system(), "messages": []});
    let chat = ram_code::openai::to_chat(&request);
    let first = &chat["messages"][0];
    assert_eq!(first["role"], "system");
    assert!(first["content"].as_str().unwrap().contains("openai rule"));
}

struct NoRunner;

impl Runner for NoRunner {
    fn run(&self, _spec: &CommandSpec) -> Result<CommandOutput, String> {
        Err("no runner in this test".into())
    }
}

fn shown(fixture: &Fixture) -> Vec<String> {
    let mut app = App::new(Arc::new(NoRunner), fixture.repo.clone());
    app.screen = Screen::Main;
    app.input = "/instructions".into();
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
    app.entries.iter().map(|entry| entry.text.clone()).collect()
}

#[test]
fn the_instructions_command_lists_the_files() {
    let fixture = Fixture::new("command");
    assert!(shown(&fixture)
        .iter()
        .any(|line| line == "no AGENTS.md or CLAUDE.md"));
    fixture.write(&fixture.repo.join("AGENTS.md"), "abc");
    fixture.write(&fixture.repo.join("CLAUDE.md"), "@docs/x.md");
    fixture.write(&fixture.repo.join("docs/x.md"), "x".repeat(2048).as_str());
    let lines = shown(&fixture);
    let at = lines
        .iter()
        .position(|line| line == "AGENTS.md (3 B)")
        .expect("AGENTS.md listed");
    assert_eq!(
        lines[at + 1..at + 3],
        ["CLAUDE.md (10 B)", "  ↳ docs/x.md (2.0 KiB)"]
    );
}

#[test]
fn the_ramify_repo_reads_its_agents_md_once() {
    let fixture = Fixture::new("ramify");
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let agents = std::fs::read_to_string(repo.join("AGENTS.md")).expect("the repo has AGENTS.md");
    let found = load(repo, &fixture.home);
    let system = found.system_prompt(SYSTEM_PROMPT);
    let first_line = agents.lines().find(|line| !line.trim().is_empty()).unwrap();
    assert_eq!(system.matches(first_line).count(), 1, "{system}");
    assert_eq!(
        std::fs::read_to_string(repo.join("CLAUDE.md"))
            .unwrap()
            .trim(),
        "@AGENTS.md"
    );
}
