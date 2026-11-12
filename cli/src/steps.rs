//! The step commands: record a step, gate on earlier ones, and run a human gate from the ask to
//! the decision. A port of `libexec/workflow.py`; the work order's frontmatter is the record, so
//! the window, the CLI and the chat all end in the same file.

use crate::config::Config;
use crate::workflow::{find_order, workflows_file, Definitions, OrderMeta, WorkflowDef};
use serde_json::{json, Value as Json};
use serde_yaml_ng::{Mapping, Value as Yaml};
use std::path::{Component, Path, PathBuf};

struct Fail(String);

impl<T: Into<String>> From<T> for Fail {
    fn from(text: T) -> Self {
        Fail(text.into())
    }
}

enum Loaded {
    Off,
    On(Box<Context>),
}

struct Context {
    config: Config,
    definitions: Definitions,
    branch: String,
}

struct Order {
    path: PathBuf,
    meta: OrderMeta,
}

/// `ramify step <done|na|gate|ask|wait|guard|ensure> …`
pub fn step(args: &[String]) -> i32 {
    let Some(command) = args.first().map(String::as_str) else {
        return usage();
    };
    let rest = &args[1..];
    let result = match command {
        "done" => with_context(command, rest, |context| record(context, rest, false)),
        "na" => with_context(command, rest, |context| record(context, rest, true)),
        "gate" => with_context(command, rest, |context| {
            if rest == ["--human"] {
                human_gate(context)
            } else {
                gate(context, rest)
            }
        }),
        "ask" => with_context(command, rest, |context| ask(context, rest)),
        "wait" => with_context(command, rest, |context| wait(context, rest)),
        "guard" => with_context(command, rest, guard),
        "ensure" => with_context(command, rest, ensure),
        _ => return usage(),
    };
    let opened = matches!(command, "ask" | "ensure") && result.0 == 0 && result.1;
    if opened && std::env::var("RAMIFY_POPUP").as_deref() != Ok("0") {
        open_approvals(command == "ask");
    }
    result.0
}

/// `ramify approve <step>` and `ramify reject <step> --note TEXT`: the human's decision.
pub fn decide(command: &str, args: &[String]) -> i32 {
    with_context(command, args, |context| decide_step(context, args, command == "approve")).0
}

fn usage() -> i32 {
    eprintln!(
        "ramify: usage: ramify step [done <step> | na <step> | gate <step>|--pr | ask <step> | wait <step>]"
    );
    1
}

fn open_approvals(asked: bool) {
    if crate::launch::open("approvals").is_err() {
        if asked {
            println!("  the approvals window did not open — run 'ramify approvals', or decide with 'ramify approve|reject'");
        } else {
            println!("  the approvals window did not open — run 'ramify approvals'");
        }
    }
}

/// Runs one command and maps its failure the way `workflow.py` did. The `bool` says whether a
/// request went up, so the caller opens the window.
fn with_context<F>(command: &str, args: &[String], run: F) -> (i32, bool)
where
    F: FnOnce(&Context) -> Result<(i32, bool), Fail>,
{
    let outcome = load().and_then(|loaded| match loaded {
        Loaded::Off => Ok(None),
        Loaded::On(context) => run(&context).map(Some),
    });
    match outcome {
        Ok(Some(done)) => done,
        Ok(None) => (off_output(command, args), false),
        Err(Fail(error)) => (fail_output(command, &error), false),
    }
}

fn off_output(command: &str, _args: &[String]) -> i32 {
    match command {
        "ensure" | "guard" | "gate" => 0,
        _ => {
            eprintln!("ramify: workflows are off for this repo");
            1
        }
    }
}

fn fail_output(command: &str, error: &str) -> i32 {
    if command == "guard" {
        // A gate that cannot be read cannot be opened either: say so, and hold nothing.
        eprintln!("ramify: cannot verify the configured human gate: {error}");
        return 0;
    }
    eprintln!("ramify: {error}");
    if command == "gate" {
        2
    } else {
        1
    }
}

fn load() -> Result<Loaded, Fail> {
    let source = std::env::var("RAMIFY_WORKFLOW_SOURCE").unwrap_or_default();
    let config = Config::resolve().map_err(Fail)?;
    let source = if source.is_empty() { config.workflow_source.clone() } else { source };
    if !["", "auto", "ramify", "repo", "select"].contains(&source.as_str()) {
        return Err("RAMIFY_WORKFLOW_SOURCE must be auto, ramify, or repo".into());
    }
    if std::env::var("RAMIFY_WORKFLOWS").as_deref() == Ok("0") {
        return Ok(Loaded::Off);
    }
    let path = workflows_file(&config).ok_or("no workflows are defined")?;
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let raw: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    // YAML 1.1 read a bare `off` as false.
    match raw.get("workflows") {
        Some(serde_yaml_ng::Value::Bool(false)) => return Ok(Loaded::Off),
        Some(serde_yaml_ng::Value::String(word)) if word == "off" => return Ok(Loaded::Off),
        _ => {}
    }
    let definitions: Definitions = serde_yaml_ng::from_value(raw)
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let branch = std::env::var("WF_BRANCH")
        .ok()
        .or_else(|| crate::config::git(&["rev-parse", "--abbrev-ref", "HEAD"], &config.repo_root).ok())
        .map(|text| text.trim().to_string())
        .unwrap_or_default();
    Ok(Loaded::On(Box::new(Context { config, definitions, branch })))
}

impl Context {
    fn root(&self) -> &Path {
        &self.config.repo_root
    }

    fn work_dir(&self) -> String {
        self.definitions.work_dir.clone().unwrap_or_else(|| ".ramify/work".into())
    }

    fn find(&self) -> Option<Order> {
        find_order(self.root(), &self.work_dir(), &self.branch, &self.definitions)
            .map(|(path, meta)| Order { path, meta })
    }

    fn current(&self) -> Result<(Order, &WorkflowDef), Fail> {
        let order = self.find().ok_or_else(|| {
            format!(
                "no work order for branch {} — start one with `ramify new <slug> --workflow <name>`",
                self.branch
            )
        })?;
        let workflow = self.definitions.workflows.get(&order.meta.workflow).ok_or_else(|| {
            format!(
                "no workflow '{}' — one of: {}",
                order.meta.workflow,
                self.definitions.workflows.keys().cloned().collect::<Vec<_>>().join(", ")
            )
        })?;
        if !workflow.lanes.contains_key(&order.meta.lane) {
            return Err(format!(
                "{}: lane '{}' is not one of {}'s",
                order.path.display(),
                order.meta.lane,
                order.meta.workflow
            )
            .into());
        }
        Ok((order, workflow))
    }

    fn gate_of(&self, step: &str) -> &str {
        self.definitions
            .steps
            .get(step)
            .and_then(|definition| definition.gate.as_deref())
            .unwrap_or("auto")
    }

    fn files_of(&self, step: &str) -> Vec<String> {
        self.definitions
            .steps
            .get(step)
            .and_then(|definition| definition.files.clone())
            .unwrap_or_else(|| vec![format!("{step}.md")])
    }

    fn date_field(&self) -> String {
        self.definitions
            .record
            .as_ref()
            .and_then(|record| record.date_field.clone())
            .unwrap_or_else(|| "date".into())
    }

    fn note_field(&self) -> String {
        self.definitions
            .record
            .as_ref()
            .and_then(|record| record.note_field.clone())
            .unwrap_or_else(|| "note".into())
    }

    fn relative(&self, path: &Path) -> String {
        relpath(path, self.root())
    }

    fn auto_ask(&self) -> bool {
        self.definitions.auto_ask == Some(true)
    }
}

fn lane<'a>(workflow: &'a WorkflowDef, meta: &OrderMeta) -> &'a Vec<String> {
    &workflow.lanes[&meta.lane]
}

fn recorded(meta: &OrderMeta) -> Vec<&str> {
    meta.steps.iter().map(|entry| entry.step.as_str()).collect()
}

fn pending(meta: &OrderMeta, workflow: &WorkflowDef) -> Vec<String> {
    let done = recorded(meta);
    lane(workflow, meta)
        .iter()
        .filter(|step| !done.contains(&step.as_str()))
        .cloned()
        .collect()
}

/// The step that waits on the human. A rejected request is answered, not open.
fn open_ask(meta: &OrderMeta) -> Option<&str> {
    meta.ask
        .as_ref()
        .filter(|ask| ask.decision.is_none())
        .map(|ask| ask.step.as_str())
}

struct Options {
    positional: Vec<String>,
    flags: Vec<String>,
    values: Vec<(String, String)>,
}

impl Options {
    fn parse(args: &[String], flags: &[&str], values: &[&str]) -> Result<Self, Fail> {
        let mut options = Options { positional: Vec::new(), flags: Vec::new(), values: Vec::new() };
        let mut index = 0;
        while index < args.len() {
            let arg = &args[index];
            let name = arg.strip_prefix("--");
            match name {
                Some(name) if flags.contains(&name) => options.flags.push(name.into()),
                Some(name) if values.contains(&name) => {
                    let value = args.get(index + 1).ok_or_else(|| format!("{arg} needs a value"))?;
                    options.values.push((name.into(), value.clone()));
                    index += 1;
                }
                Some(_) => return Err(format!("unknown option {arg}").into()),
                None => options.positional.push(arg.clone()),
            }
            index += 1;
        }
        Ok(options)
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|flag| flag == name)
    }

    fn all(&self, name: &str) -> Vec<String> {
        self.values.iter().filter(|(key, _)| key == name).map(|(_, value)| value.clone()).collect()
    }

    fn last(&self, name: &str) -> Option<String> {
        self.all(name).pop()
    }
}

fn record(context: &Context, args: &[String], na: bool) -> Result<(i32, bool), Fail> {
    let step = args.first().map(String::as_str).unwrap_or("");
    if step.is_empty() || step.starts_with("--") {
        return Err(format!(
            "usage: ramify step {} <step> [--note TEXT]",
            if na { "na" } else { "done" }
        )
        .into());
    }
    let options = Options::parse(&args[1..], &["approved"], &["note"])?;
    let (order, workflow) = context.current()?;
    let todo = pending(&order.meta, workflow);
    if recorded(&order.meta).contains(&step) {
        return Err(format!("{step} is already recorded").into());
    }
    if !lane(workflow, &order.meta).iter().any(|name| name == step) {
        return Err(format!("{step} is not a step of the {} lane", order.meta.lane).into());
    }
    if step != todo[0] {
        return Err(format!("the next step is {}, not {step}", todo[0]).into());
    }
    if !na && context.gate_of(step) == "human" && !options.flag("approved") {
        return Err(format!(
            "{step} is a human gate: record it with --approved, and only after the user's explicit OK"
        )
        .into());
    }
    let mut entry = step_entry(context, step);
    if na {
        entry.insert(Yaml::from("na"), Yaml::Bool(true));
    }
    if let Some(note) = options.last("note").filter(|note| !note.is_empty()) {
        entry.insert(Yaml::from(context.note_field()), Yaml::from(note));
    }
    let mut head = Head::read(&order.path)?;
    head.append_step(entry);
    // An OK given in the chat answers the window's request too.
    if order.meta.ask.as_ref().is_some_and(|ask| ask.step == step) {
        head.set("ask", None);
    }
    head.write(context)?;
    let next = todo.get(1).cloned().unwrap_or_else(|| "nothing, every step is recorded".into());
    println!("  recorded {step}{} — next: {next}", if na { " (n/a)" } else { "" });
    Ok((0, false))
}

/// `gate --pr` gates on the lane's first `opens_pr` step; a lane without one opens PRs freely.
fn gate(context: &Context, args: &[String]) -> Result<(i32, bool), Fail> {
    if args.len() != 1 {
        return Err("usage: workflow.py gate <step> | --pr".into());
    }
    if context.find().is_none() {
        return Ok((0, false));
    }
    let (order, workflow) = context.current()?;
    let steps = lane(workflow, &order.meta);
    let target = if args[0] == "--pr" {
        steps
            .iter()
            .find(|step| {
                context.definitions.steps.get(*step).and_then(|definition| definition.opens_pr)
                    == Some(true)
            })
            .cloned()
    } else {
        Some(args[0].clone())
    };
    let Some(target) = target else {
        return Ok((0, false));
    };
    let Some(position) = steps.iter().position(|step| step == &target) else {
        return Ok((0, false));
    };
    let done = recorded(&order.meta);
    let missing: Vec<&String> = steps[..position]
        .iter()
        .filter(|step| !done.contains(&step.as_str()))
        .collect();
    let Some(first) = missing.first() else {
        return Ok((0, false));
    };
    let skill = context
        .definitions
        .steps
        .get(*first)
        .and_then(|definition| definition.skill.clone())
        .map(|skill| format!(" — invoke {skill}"))
        .unwrap_or_default();
    let human = if context.gate_of(first) == "human" { ", a human gate" } else { "" };
    println!(
        "{target} waits on: {} ({}). Next: {first}{skill}{human}.",
        missing.iter().map(|step| step.as_str()).collect::<Vec<_>>().join(", "),
        context.relative(&order.path)
    );
    Ok((1, false))
}

fn human_gate(context: &Context) -> Result<(i32, bool), Fail> {
    if context.find().is_none() || !context.auto_ask() {
        return Ok((0, false));
    }
    let (order, workflow) = context.current()?;
    let todo = pending(&order.meta, workflow);
    if let Some(next) = todo.first().filter(|step| context.gate_of(step) == "human") {
        println!(
            "{next} needs human approval in Ramify before implementation ({})",
            context.relative(&order.path)
        );
        return Ok((1, false));
    }
    Ok((0, false))
}

fn gate_step(context: &Context, meta: &OrderMeta, workflow: &WorkflowDef, step: &str) -> Result<(), Fail> {
    let todo = pending(meta, workflow);
    if !lane(workflow, meta).iter().any(|name| name == step) {
        return Err(format!("{step} is not a step of the {} lane", meta.lane).into());
    }
    if recorded(meta).contains(&step) {
        return Err(format!("{step} is already recorded").into());
    }
    if step != todo[0] {
        return Err(format!("the next step is {}, not {step}", todo[0]).into());
    }
    if context.gate_of(step) != "human" {
        return Err(format!("{step} is not a human gate: record it with `ramify step done {step}`").into());
    }
    Ok(())
}

fn ask(context: &Context, args: &[String]) -> Result<(i32, bool), Fail> {
    let options = Options::parse(args, &[], &["file"])?;
    if options.positional.len() != 1 {
        return Err("usage: ramify step ask <step> [--file PATH]...".into());
    }
    ask_step(context, &options.positional[0], options.all("file"))
}

fn ask_step(context: &Context, step: &str, named: Vec<String>) -> Result<(i32, bool), Fail> {
    let (order, workflow) = context.current()?;
    gate_step(context, &order.meta, workflow, step)?;
    let root = context.root();
    let real_root = realpath(root);
    let folder = order.path.parent().unwrap_or(root).to_path_buf();
    let requested = if named.is_empty() {
        context
            .files_of(step)
            .iter()
            .map(|name| folder.join(name).to_string_lossy().into_owned())
            .collect()
    } else {
        named
    };
    let mut files = Vec::new();
    for name in requested {
        let full = realpath(&root.join(&name));
        if !full.is_file() {
            return Err(format!("{name}: no such file — write it before you ask, or name it with --file").into());
        }
        if !full.starts_with(&real_root) {
            return Err(format!("{name} is outside the worktree").into());
        }
        files.push(relpath(&full, &real_root));
    }
    let order_file = relpath(&realpath(&order.path), &real_root);
    if !files.contains(&order_file) {
        files.push(order_file);
    }
    let mut fields = new_ask(step, &files);
    if context.auto_ask() {
        let mut map = Mapping::new();
        for (file, digest) in digests(context, &order.path, &files)? {
            map.insert(Yaml::from(file), Yaml::from(digest));
        }
        fields.insert(Yaml::from("digests"), Yaml::Mapping(map));
    }
    let mut head = Head::read(&order.path)?;
    head.set("ask", Some(Yaml::Mapping(fields)));
    head.write(context)?;
    println!("  {step} waits on the human — the approvals window shows {}", files.join(", "));
    Ok((0, true))
}

fn decide_step(context: &Context, args: &[String], approve: bool) -> Result<(i32, bool), Fail> {
    let options = Options::parse(args, &[], &["note"])?;
    let verb = if approve { "approve" } else { "reject" };
    if options.positional.len() != 1 {
        return Err(format!(
            "usage: ramify {verb} <step> {}",
            if approve { "[--note TEXT]" } else { "--note TEXT" }
        )
        .into());
    }
    let note = options.last("note").unwrap_or_default().trim().to_string();
    if !approve && note.is_empty() {
        return Err("a rejection needs --note: say what to change".into());
    }
    let step = options.positional[0].as_str();
    let (order, workflow) = context.current()?;
    gate_step(context, &order.meta, workflow, step)?;
    // The human decides the next gate whether or not the agent asked: a missing request is the
    // agent's slip, and it must never leave the human unable to answer.
    let mut head = Head::read(&order.path)?;
    let mut fields = match order.meta.ask.as_ref().filter(|_| open_ask(&order.meta) == Some(step)) {
        Some(asked) => {
            if let Some(stored) = &asked.digests {
                let now = digests(context, &order.path, &asked.files)?;
                if yaml_digests(stored) != now {
                    return Err("approval files changed; refresh the request before deciding".into());
                }
            }
            match head.get("ask") {
                Some(Yaml::Mapping(map)) => map,
                _ => new_ask(step, &[]),
            }
        }
        None => new_ask(step, &[]),
    };
    if approve {
        let mut entry = step_entry(context, step);
        if !note.is_empty() {
            entry.insert(Yaml::from(context.note_field()), Yaml::from(note.clone()));
        }
        head.append_step(entry);
        head.set("ask", None);
    } else {
        fields.insert(Yaml::from("decision"), Yaml::from("rejected"));
        fields.insert(Yaml::from("decided"), Yaml::from(now()));
        fields.insert(Yaml::from("note"), Yaml::from(note.clone()));
        head.set("ask", Some(Yaml::Mapping(fields)));
    }
    head.write(context)?;
    let shown = if note.is_empty() { String::new() } else { format!(": {note}") };
    println!("  {step} {}{shown}", if approve { "approved" } else { "rejected" });
    Ok((0, false))
}

/// Exit 0 with `approved` or `rejected: <note>`; 1 on timeout. It polls the work order: the
/// decision can come from the window, the CLI or the chat, and they all end there.
fn wait(context: &Context, args: &[String]) -> Result<(i32, bool), Fail> {
    let options = Options::parse(args, &[], &["timeout"])?;
    if options.positional.len() != 1 {
        return Err("usage: ramify step wait <step> [--timeout SECONDS]".into());
    }
    let step = options.positional[0].as_str();
    let timeout: f64 = match options.last("timeout") {
        Some(text) => text.parse().map_err(|_| format!("could not convert string to float: '{text}'"))?,
        None => 86400.0,
    };
    let poll: f64 = std::env::var("WF_POLL").ok().and_then(|text| text.parse().ok()).unwrap_or(2.0);
    // `inf` or a huge timeout waits for ever, as it did in Python.
    let deadline = std::time::Duration::try_from_secs_f64(timeout.max(0.0))
        .ok()
        .and_then(|timeout| std::time::Instant::now().checked_add(timeout));
    loop {
        let (order, _) = context.current()?;
        if let Some(entry) = order.meta.steps.iter().find(|entry| entry.step == step) {
            let note = entry.note.as_ref().map(|note| format!(": {note}")).unwrap_or_default();
            println!("approved{note}");
            return Ok((0, false));
        }
        match &order.meta.ask {
            Some(ask) if ask.step == step && ask.decision.as_deref() == Some("rejected") => {
                println!("rejected: {}", ask.note.clone().unwrap_or_default());
                return Ok((0, false));
            }
            Some(ask) if ask.step == step => {}
            _ => {
                return Err(format!(
                    "nothing asks for a decision on {step} — run `ramify step ask {step}` first"
                )
                .into())
            }
        }
        if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
            println!("no decision on {step} yet");
            return Ok((1, false));
        }
        std::thread::sleep(std::time::Duration::from_secs_f64(poll.max(0.05)));
    }
}

/// Holds the gated worktree while a human gate is next. Reads, planning edits and anything aimed
/// at another directory pass, so a gate never stops the agent from opening itself.
fn guard(context: &Context) -> Result<(i32, bool), Fail> {
    if !context.auto_ask() {
        return Ok((0, false));
    }
    let mut input = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
    let payload: Json = serde_json::from_str(&input).map_err(|error| error.to_string())?;
    let root = context.root().to_string_lossy().into_owned();
    let patterns = context
        .definitions
        .planning_paths
        .clone()
        .filter(|patterns| !patterns.is_empty())
        .unwrap_or_else(|| vec!["docs/**".into(), ".ramify/**".into(), ".claude/**".into()]);
    if !crate::guard::targets_root(&payload, &root) || crate::guard::planning_tool(&payload, &root, &patterns) {
        return Ok((0, false));
    }
    if context.find().is_none() {
        return Ok((0, false));
    }
    let (order, workflow) = match context.current() {
        Ok(found) => found,
        Err(Fail(error)) => {
            deny_tool(&format!(
                "Cannot verify the configured human gate: {error}. Fix the story record; reads, planning edits and other worktrees still run."
            ));
            return Ok((0, false));
        }
    };
    let todo = pending(&order.meta, workflow);
    if let Some(next) = todo.first().filter(|step| context.gate_of(step) == "human") {
        deny_tool(&gate_reason(context, &order, next));
    }
    Ok((0, false))
}

/// What the agent reads when the gate refuses a tool call: always a next move, never a dead end.
fn gate_reason(context: &Context, order: &Order, step: &str) -> String {
    let root = context.root().display();
    let scope = format!(
        "Until then only reads and planning edits run in {root}; other worktrees and repositories are not held. The user can also give the OK in the chat: record it with `ramify step done {step} --approved`."
    );
    if open_ask(&order.meta) == Some(step) {
        return format!(
            "{step} waits on the human: approve it in the approvals window, or run `ramify approve {step}` in {root}. {scope}"
        );
    }
    let missing = missing_files(context, order, step);
    if !missing.is_empty() {
        return format!(
            "{step} is a human gate, and Ramify asks for it once {} exist. Write {} now (planning edits are allowed): the request then opens by itself. {scope}",
            missing.join(", "),
            if missing.len() == 1 { "it" } else { "them" }
        );
    }
    format!("{step} is a human gate that nobody asked for yet: run `ramify step ask {step}`. {scope}")
}

fn missing_files(context: &Context, order: &Order, step: &str) -> Vec<String> {
    let folder = order.path.parent().unwrap_or(context.root());
    context
        .files_of(step)
        .iter()
        .filter(|name| !folder.join(name).is_file())
        .map(|name| context.relative(&folder.join(name)))
        .collect()
}

fn deny_tool(reason: &str) {
    let value = json!({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}});
    println!("{}", ascii(&crate::pyjson::to_string(&value)));
}

/// Native pipelines put their ready human gate up once; later hook calls leave it as it is.
fn ensure(context: &Context) -> Result<(i32, bool), Fail> {
    if !context.auto_ask() || context.find().is_none() {
        return Ok((0, false));
    }
    let (order, workflow) = context.current()?;
    let todo = pending(&order.meta, workflow);
    let Some(step) = todo.first().filter(|step| context.gate_of(step) == "human") else {
        return Ok((0, false));
    };
    let folder = order.path.parent().unwrap_or(context.root());
    if !context.files_of(step).iter().all(|name| folder.join(name).is_file()) {
        return Ok((0, false));
    }
    let same = match order.meta.ask.as_ref() {
        Some(ask) if ask.step == *step => {
            let now = digests(context, &order.path, &ask.files)?;
            ask.digests.as_ref().map(yaml_digests) == Some(now)
        }
        _ => false,
    };
    if same {
        return Ok((0, false));
    }
    ask_step(context, step, Vec::new())
}

/// A digest per approval file, so a decision on changed files is refused. The work order itself
/// is hashed without its `ask` and `steps`, which the decision writes.
fn digests(context: &Context, order: &Path, files: &[String]) -> Result<Vec<(String, String)>, Fail> {
    use sha2::{Digest, Sha256};
    let root = realpath(context.root());
    let mut result = Vec::new();
    for relative in files {
        let path = realpath(&context.root().join(relative));
        if !path.starts_with(&root) {
            return Err("approval file is outside the worktree".into());
        }
        let data = if path == realpath(order) {
            let text = std::fs::read_to_string(&path).map_err(|error| error.to_string())?;
            let (head, body) = split_order(&text, &path)?;
            let mut meta: serde_yaml_ng::Value = serde_yaml_ng::from_str(&head).unwrap_or_default();
            if let Some(map) = meta.as_mapping_mut() {
                map.remove("ask");
                map.remove("steps");
            }
            let json: Json = serde_json::to_value(&meta).unwrap_or(Json::Null);
            format!("{}{body}", ascii(&crate::pyjson::to_string(&sorted(json)))).into_bytes()
        } else {
            std::fs::read(&path).map_err(|error| error.to_string())?
        };
        let digest = Sha256::digest(&data);
        result.push((relative.clone(), digest.iter().map(|byte| format!("{byte:02x}")).collect()));
    }
    Ok(result)
}

fn yaml_digests(value: &serde_yaml_ng::Value) -> Vec<(String, String)> {
    value
        .as_mapping()
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| Some((key.as_str()?.to_string(), value.as_str()?.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn sorted(value: Json) -> Json {
    match value {
        Json::Object(map) => {
            let mut entries: Vec<(String, Json)> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Json::Object(entries.into_iter().map(|(key, value)| (key, sorted(value))).collect())
        }
        Json::Array(items) => Json::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// Python's `json.dumps` escapes every non-ASCII character.
fn ascii(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_ascii() {
            out.push(c);
        } else {
            let mut units = [0u16; 2];
            for unit in c.encode_utf16(&mut units) {
                out.push_str(&format!("\\u{unit:04x}"));
            }
        }
    }
    out
}

// ── The frontmatter ── PyYAML wrote it with `default_flow_style=None`: a collection of scalars
// is one flow line, anything else is a block. Each command changes one key, so the other lines
// stay byte for byte.

fn split_order(text: &str, path: &Path) -> Result<(String, String), Fail> {
    let mut text = text.replace("\r\n", "\n");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    if !text.starts_with("---\n") {
        return Err(format!("{}: no frontmatter", path.display()).into());
    }
    let end = text[3..]
        .find("\n---\n")
        .map(|index| index + 3)
        .ok_or_else(|| format!("{}: the frontmatter is not closed", path.display()))?;
    Ok((text[4..end + 1].to_string(), text[end + 5..].to_string()))
}

struct Head {
    path: PathBuf,
    /// Each top-level key with its lines, the key line first.
    keys: Vec<(String, Vec<String>)>,
    /// The frontmatter, parsed: a changed key is rewritten from it, the others stay as written.
    meta: Mapping,
    body: String,
}

impl Head {
    fn read(path: &Path) -> Result<Self, Fail> {
        let text = std::fs::read_to_string(path).map_err(|error| format!("{}: {error}", path.display()))?;
        let (head, body) = split_order(&text, path)?;
        let meta = match serde_yaml_ng::from_str::<Yaml>(&head) {
            Ok(Yaml::Mapping(map)) => map,
            Ok(Yaml::Null) => Mapping::new(),
            Ok(_) => return Err(format!("{}: the frontmatter is not a map", path.display()).into()),
            Err(error) => return Err(format!("{}: {error}", path.display()).into()),
        };
        let mut keys: Vec<(String, Vec<String>)> = Vec::new();
        for line in head.lines() {
            let top = !line.starts_with([' ', '\t', '-', '#']) && !line.is_empty();
            match (top, keys.last_mut()) {
                (false, Some((_, lines))) => lines.push(line.to_string()),
                _ => {
                    let key = line.split(':').next().unwrap_or("").trim().to_string();
                    keys.push((key, vec![line.to_string()]));
                }
            }
        }
        // A key the line scan cannot find is written in a way it does not know: refuse rather
        // than write a record that no longer parses.
        for key in meta.keys() {
            let name = key.as_str().unwrap_or("");
            if !keys.iter().any(|(found, _)| found == name) {
                return Err(format!("{}: cannot edit the frontmatter key {name:?} in place", path.display()).into());
            }
        }
        Ok(Head { path: path.to_path_buf(), keys, meta, body })
    }

    fn get(&self, key: &str) -> Option<Yaml> {
        self.meta.get(key).cloned()
    }

    fn set(&mut self, key: &str, value: Option<Yaml>) {
        let position = self.keys.iter().position(|(name, _)| name == key);
        match value {
            Some(value) => {
                let lines = emit_key(key, &value);
                self.meta.insert(Yaml::from(key), value);
                match position {
                    Some(index) => self.keys[index].1 = lines,
                    None => self.keys.push((key.to_string(), lines)),
                }
            }
            None => {
                self.meta.remove(key);
                if let Some(index) = position {
                    self.keys.remove(index);
                }
            }
        }
    }

    fn append_step(&mut self, entry: Mapping) {
        let mut steps = match self.get("steps") {
            Some(Yaml::Sequence(steps)) => steps,
            _ => Vec::new(),
        };
        steps.push(Yaml::Mapping(entry));
        self.set("steps", Some(Yaml::Sequence(steps)));
    }

    fn write(&self, context: &Context) -> Result<(), Fail> {
        let mut text = String::from("---\n");
        for (_, lines) in &self.keys {
            for line in lines {
                text.push_str(line);
                text.push('\n');
            }
        }
        text.push_str("---\n");
        text.push_str(&self.body);
        let temporary = self.path.with_extension("md.tmp");
        std::fs::write(&temporary, text).map_err(|error| format!("{}: {error}", temporary.display()))?;
        std::fs::rename(&temporary, &self.path).map_err(|error| format!("{}: {error}", self.path.display()))?;
        // `ramify gates` reads an index of the worktrees with an open request.
        let state = std::env::var("RAMIFY_STATE_DIR").unwrap_or_else(|_| "/tmp/ramify".into());
        crate::commands::check(&Path::new(&state).join("gates"), context.root());
        Ok(())
    }
}

fn step_entry(context: &Context, step: &str) -> Mapping {
    let mut entry = Mapping::new();
    entry.insert(Yaml::from("step"), Yaml::from(step));
    entry.insert(Yaml::from(context.date_field()), Yaml::from(today()));
    entry
}

fn new_ask(step: &str, files: &[String]) -> Mapping {
    let mut ask = Mapping::new();
    ask.insert(Yaml::from("step"), Yaml::from(step));
    ask.insert(Yaml::from("asked"), Yaml::from(now()));
    ask.insert(
        Yaml::from("files"),
        Yaml::Sequence(files.iter().map(|file| Yaml::from(file.as_str())).collect()),
    );
    ask
}

fn is_scalar(value: &Yaml) -> bool {
    !matches!(value, Yaml::Sequence(_) | Yaml::Mapping(_) | Yaml::Tagged(_))
}

/// One top-level key the way PyYAML writes it with `default_flow_style=None`: a collection of
/// scalars on one flow line, any other collection as a block.
fn emit_key(key: &str, value: &Yaml) -> Vec<String> {
    match value {
        Yaml::Sequence(items) if !items.iter().all(is_scalar) => {
            let mut lines = vec![format!("{key}:")];
            lines.extend(items.iter().map(|item| format!("- {}", emit_flow(item))));
            lines
        }
        Yaml::Mapping(map) if !map.values().all(is_scalar) => {
            let mut lines = vec![format!("{key}:")];
            lines.extend(map.iter().map(|(name, item)| format!("  {}: {}", emit_flow(name), emit_inline(item))));
            lines
        }
        _ => vec![format!("{key}: {}", emit_inline(value))],
    }
}

/// A value after `key: ` in block context.
fn emit_inline(value: &Yaml) -> String {
    match value {
        Yaml::String(text) => yaml_scalar(text, false),
        other => emit_flow(other),
    }
}

/// A value inside `[…]` or `{…}`.
fn emit_flow(value: &Yaml) -> String {
    match value {
        Yaml::Null => "null".into(),
        Yaml::Bool(flag) => flag.to_string(),
        Yaml::Number(number) => number.to_string(),
        Yaml::String(text) => yaml_scalar(text, true),
        Yaml::Sequence(items) => format!("[{}]", items.iter().map(emit_flow).collect::<Vec<_>>().join(", ")),
        Yaml::Mapping(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(name, item)| format!("{}: {}", emit_flow(name), emit_flow(item)))
                .collect::<Vec<_>>()
                .join(", ")
        ),
        Yaml::Tagged(tagged) => emit_flow(&tagged.value),
    }
}

/// A string as PyYAML's safe dumper writes it: plain when it reads back as the same string,
/// else single-quoted. `flow` is true inside `[…]` or `{…}`, where `,[]{}` need quotes too.
/// Text with a line break or a control character is double-quoted with JSON escapes, so it
/// stays on one line: a note from the approvals window can hold `---` on a line of its own.
fn yaml_scalar(text: &str, flow: bool) -> String {
    if text.chars().any(|c| c.is_control() || c == '\u{feff}' || c == '\u{2028}' || c == '\u{2029}') {
        return serde_json::to_string(text).unwrap_or_else(|_| "''".into());
    }
    if plain(text, flow) {
        text.to_string()
    } else {
        format!("'{}'", text.replace('\'', "''"))
    }
}

fn plain(text: &str, flow: bool) -> bool {
    if text.is_empty() || text.starts_with(' ') || text.ends_with(' ') || implicit(text) {
        return false;
    }
    let chars: Vec<char> = text.chars().collect();
    let first = chars[0];
    if "#,[]{}&*!|>'\"%@`".contains(first) {
        return false;
    }
    if "?:-".contains(first) && chars.get(1).is_none_or(|next| *next == ' ' || (flow && ",[]{}".contains(*next))) {
        return false;
    }
    for (index, c) in chars.iter().enumerate() {
        if c.is_control() || *c == '\u{feff}' {
            return false;
        }
        if flow && ",?[]{}".contains(*c) {
            return false;
        }
        let next = chars.get(index + 1);
        if *c == ':' && next.is_none_or(|next| *next == ' ' || (flow && ",[]{}".contains(*next))) {
            return false;
        }
        if *c == '#' && index > 0 && chars[index - 1] == ' ' {
            return false;
        }
    }
    true
}

/// Whether YAML 1.1 reads the text as something other than a string.
fn implicit(text: &str) -> bool {
    const WORDS: &[&str] = &[
        "yes", "Yes", "YES", "no", "No", "NO", "true", "True", "TRUE", "false", "False", "FALSE",
        "on", "On", "ON", "off", "Off", "OFF", "y", "Y", "n", "N", "null", "Null", "NULL", "~", "<<", "=",
    ];
    if WORDS.contains(&text) {
        return true;
    }
    // This CLI reads YAML 1.2: `1e3` and `0o17` are numbers there and not in YAML 1.1.
    if !matches!(serde_yaml_ng::from_str::<Yaml>(text), Ok(Yaml::String(_))) {
        return true;
    }
    let patterns = [
        r"^[-+]?0b[0-1_]+$",
        r"^[-+]?0[0-7_]+$",
        r"^[-+]?(?:0|[1-9][0-9_]*)$",
        r"^[-+]?0x[0-9a-fA-F_]+$",
        r"^[-+]?[1-9][0-9_]*(?::[0-5]?[0-9])+$",
        r"^[-+]?(?:[0-9][0-9_]*)\.[0-9_]*(?:[eE][-+][0-9]+)?$",
        r"^\.[0-9_]+(?:[eE][-+][0-9]+)?$",
        r"^[-+]?[0-9][0-9_]*(?::[0-5]?[0-9])+\.[0-9_]*$",
        r"^[-+]?\.(?:inf|Inf|INF)$",
        r"^\.(?:nan|NaN|NAN)$",
        r"^[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]$",
        r"^[0-9][0-9][0-9][0-9]-[0-9][0-9]?-[0-9][0-9]?(?:[Tt]|[ \t]+)[0-9][0-9]?:[0-9][0-9]:[0-9][0-9](?:\.[0-9]*)?(?:[ \t]*(?:Z|[-+][0-9][0-9]?(?::[0-9][0-9])?))?$",
    ];
    patterns.iter().any(|pattern| regex::Regex::new(pattern).is_ok_and(|re| re.is_match(text)))
}

fn today() -> String {
    clock("+%F")
}

fn now() -> String {
    clock("+%Y-%m-%dT%H:%M:%S")
}

fn clock(format: &str) -> String {
    std::process::Command::new("date")
        .arg(format)
        .output()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}

// ── Paths ── Python's `os.path.realpath` and `relpath`, which take paths that do not exist.

pub(crate) fn realpath(path: &Path) -> PathBuf {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir().unwrap_or_default().join(path)
    };
    let mut resolved = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::ParentDir => {
                resolved.pop();
            }
            Component::CurDir => {}
            other => {
                resolved.push(other);
                if let Ok(real) = resolved.canonicalize() {
                    resolved = real;
                }
            }
        }
    }
    resolved
}

pub(crate) fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                out.pop();
            }
            Component::CurDir => {}
            other => out.push(other),
        }
    }
    out
}

pub(crate) fn relpath(path: &Path, start: &Path) -> String {
    let path = normalize(path);
    let start = normalize(start);
    let a: Vec<_> = path.components().collect();
    let b: Vec<_> = start.components().collect();
    let common = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let mut parts: Vec<String> = vec!["..".into(); b.len() - common];
    parts.extend(a[common..].iter().map(|part| part.as_os_str().to_string_lossy().into_owned()));
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quotes_what_yaml_would_read_back_as_another_type() {
        assert_eq!(yaml_scalar("2026-11-12", true), "'2026-11-12'");
        assert_eq!(yaml_scalar("2026-11-12T10:20:30", false), "'2026-11-12T10:20:30'");
        assert_eq!(yaml_scalar("name the marks", true), "name the marks");
        assert_eq!(yaml_scalar("a, b", true), "'a, b'");
        assert_eq!(yaml_scalar("a, b", false), "a, b");
        assert_eq!(yaml_scalar("yes", false), "'yes'");
        assert_eq!(yaml_scalar(".ramify/work/demo/spec.md", true), ".ramify/work/demo/spec.md");
        assert_eq!(yaml_scalar("it's", false), "it's");
        assert_eq!(yaml_scalar("note: x", false), "'note: x'");
    }

    #[test]
    fn relpath_climbs_out_of_the_start() {
        assert_eq!(relpath(Path::new("/a/b/c"), Path::new("/a")), "b/c");
        assert_eq!(relpath(Path::new("/a/x"), Path::new("/a/b")), "../x");
        assert_eq!(relpath(Path::new("/a"), Path::new("/a")), ".");
    }
}
