#!/usr/bin/env python3
"""ramify workflow: the work order of a branch, its lane and its steps.

Usage:  workflow.py list                          the workflows and their lanes
        workflow.py show <workflow>               each lane's steps, with skill and gate
        workflow.py branch <workflow> <slug>      the branch name the workflow gives a slug
        workflow.py start <workflow> <slug> [--lane L] [--tag T]...   write the work order
        workflow.py status [--json | --line]      this branch's progress and next step
        workflow.py done <step> [--approved] [--note TEXT]
        workflow.py na <step> [--note TEXT]       record the step as not applicable
        workflow.py lane <lane>                   raise the lane; never lowers it
        workflow.py gate <step> | --pr            exit 0 when every step before <step> is recorded,
                                                  1 while some are not, 2 when the order is broken
        workflow.py ask <step> [--file F]...      put a human gate up for a decision in the approvals window
        workflow.py approve <step> [--note TEXT]  the human's OK: records the step --approved
        workflow.py reject <step> --note TEXT     the human's no, with what to change
        workflow.py wait <step> [--timeout S]     block until the human decides; print the decision
        workflow.py asked --json                  the open request with its files' text, or null

Context comes from the environment: WF_ROOT (the worktree), WF_PRIMARY (the primary checkout),
WF_BRANCH (the current branch), WF_HOME (ramify's install, default this script's),
WF_BASE (the remote main, to refuse a slug whose work order is already merged), RAMIFY_WORKFLOWS=0 to turn it off.

Definitions: WF_ROOT/.ramify/workflows.yml, else WF_PRIMARY/.ramify/workflows.yml, else
WF_HOME/workflows.yml. A file that says `workflows: off` turns workflows off for the repo.

A work order is <work_dir>/<slug>/order.md: YAML frontmatter, then free prose. The branch in the
frontmatter is what ties it to a worktree, so a renamed folder still resolves.
"""
import datetime
import glob
import hashlib
import time
import json
import os
import re
import runpy
import subprocess
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "vendor"))
import yaml  # noqa: E402  (vendored PyYAML, pure python)
from native_records import (Fail, NativeRecordLoader, record_option, order_workflow, step_entry,
                            record_loader, check_order_keys, check_native_record, check_step_files,
                            check_native_start, requested_files)


SLUG = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*")


class Off(Exception):
    pass


def env(name):
    return os.environ.get(name, "")


def definitions_file():
    for base in (env("WF_ROOT"), env("WF_PRIMARY")):
        if base and os.path.isfile(os.path.join(base, ".ramify", "workflows.yml")):
            return os.path.join(base, ".ramify", "workflows.yml")
    home = env("WF_HOME") or os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    return os.path.join(home, "workflows.yml")


def load_definitions():
    if env("RAMIFY_WORKFLOWS") == "0":
        raise Off()
    path = definitions_file()
    try:
        with open(path, encoding="utf-8") as f:
            defs = yaml.safe_load(f) or {}
    except (OSError, yaml.YAMLError) as e:
        raise Fail(f"{path}: {e}")
    # YAML 1.1 reads a bare `off` as false.
    if defs.get("workflows") in (False, "off"):
        raise Off()
    check_definitions(defs, path)
    return defs


def check_definitions(defs, path):
    """A typo in a step name would otherwise surface as a lane that can never finish."""
    if not isinstance(defs, dict):
        raise Fail(f"{path}: not a map")
    check_native_record(defs, path)
    flows, steps = defs.get("workflows"), defs.get("steps") or {}
    if not isinstance(flows, dict) or not flows:
        raise Fail(f"{path}: no `workflows:` map")
    if not isinstance(steps, dict):
        raise Fail(f"{path}: `steps:` is not a map")
    # YAML 1.1 reads on/off/yes/no as booleans: a step called `on` would never match.
    names = list(steps) + list(flows)
    for name, wf in flows.items():
        if not isinstance(wf, dict) or not isinstance(wf.get("lanes"), dict):
            raise Fail(f"{path}: workflow {name} has no lanes")
        names += list(wf["lanes"]) + list(wf.get("min_lane") or {})
        names += [v for seq in wf["lanes"].values() if isinstance(seq, list) for v in seq]
    for n in names:
        if not isinstance(n, str):
            raise Fail(f"{path}: name {n!r} is not a string — quote it")
    for s, d in steps.items():
        if d is not None and not isinstance(d, dict):
            raise Fail(f"{path}: step {s} is not a map")
        extra = set(d or {}) - {"skill", "gate", "does", "opens_pr", "files"}
        if extra:
            raise Fail(f"{path}: step {s} has unknown keys {sorted(map(str, extra))} — quote a `does` that holds a comma")
        check_step_files(d, path, s)
        if (d or {}).get("gate", "auto") not in ("auto", "human"):
            raise Fail(f"{path}: step {s} gate must be auto or human")
    for name, wf in flows.items():
        lanes = (wf or {}).get("lanes")
        if not isinstance(lanes, dict) or not lanes:
            raise Fail(f"{path}: workflow {name} has no lanes")
        for lane, seq in lanes.items():
            if not isinstance(seq, list) or not seq:
                raise Fail(f"{path}: {name}.{lane} is not a list of steps")
            for s in seq:
                if s not in steps:
                    raise Fail(f"{path}: {name}.{lane} names step '{s}', which `steps:` does not define")
            if len(set(seq)) != len(seq):
                raise Fail(f"{path}: {name}.{lane} lists a step twice")
        if "default_lane" in wf and wf["default_lane"] not in lanes:
            raise Fail(f"{path}: {name}.default_lane '{wf['default_lane']}' is not one of its lanes")
        for tag, lane in (wf.get("min_lane") or {}).items():
            if lane not in lanes:
                raise Fail(f"{path}: {name}.min_lane.{tag} '{lane}' is not one of its lanes")
        if "{slug}" not in wf.get("branch", "{slug}"):
            raise Fail(f"{path}: {name}.branch must contain {{slug}}")


def workflow(defs, name):
    wf = defs["workflows"].get(name)
    if wf is None:
        raise Fail(f"no workflow '{name}' — one of: {', '.join(defs['workflows'])}")
    return wf


def lane_names(wf):
    return list(wf["lanes"])


def default_lane(wf):
    return wf.get("default_lane") or lane_names(wf)[0]


def heavier(wf, a, b):
    order = lane_names(wf)
    return a if order.index(a) >= order.index(b) else b


def work_dir(defs, root):
    return os.path.join(root, defs.get("work_dir") or ".ramify/work")


# ── Work orders ──────────────────────────────────────────────────────────────

def order_branch(defs, meta):
    if isinstance(meta.get("branch"), str):
        return meta["branch"]
    slug = meta.get(record_option(defs, "slug_field", "story"))
    if not isinstance(slug, str) or not SLUG.fullmatch(slug):
        raise Fail("native record needs a valid story slug or an explicit branch")
    return workflow(defs, order_workflow(defs, meta)).get("branch", "{slug}").replace("{slug}", slug)


def read_order(path, defs=None):
    with open(path, encoding="utf-8") as f:
        text = f.read().replace("\r\n", "\n")
    if not text.endswith("\n"):
        text += "\n"
    if not text.startswith("---\n"):
        raise Fail(f"{path}: no frontmatter")
    end = text.find("\n---\n", 3)
    if end < 0:
        raise Fail(f"{path}: the frontmatter is not closed")
    try:
        meta = yaml.load(text[4:end], Loader=record_loader(defs)) or {}
    except yaml.YAMLError as e:
        raise Fail(f"{path}: {e}")
    if not isinstance(meta, dict):
        raise Fail(f"{path}: the frontmatter is not a map")
    meta["steps"] = meta.get("steps") or []
    if not isinstance(meta["steps"], list) or not all(isinstance(e, dict) for e in meta["steps"]):
        raise Fail(f"{path}: `steps:` is not a list of {{step, date}} entries")
    check_order_keys(meta, defs, path)
    return meta, text[end + 5:]


def write_order(path, meta, body):
    head = yaml.safe_dump(meta, sort_keys=False, default_flow_style=None, allow_unicode=True, width=1000)
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        f.write(f"---\n{head}---\n{body}")
    os.replace(tmp, path)


def matching_order(defs, path, branch):
    slug = os.path.basename(os.path.dirname(path))
    candidates = [wf.get("branch", "{slug}").replace("{slug}", slug) for wf in defs["workflows"].values()]
    try:
        meta, _ = read_order(path, defs)
        return order_branch(defs, meta) == branch
    except Fail:
        if defs.get("record") and branch in candidates:
            raise
        return False


def find_order(defs):
    pattern = os.path.join(work_dir(defs, env("WF_ROOT")), "*", record_option(defs, "filename", "order.md"))
    matches = [path for path in sorted(glob.glob(pattern)) if matching_order(defs, path, env("WF_BRANCH"))]
    if len(matches) > 1:
        raise Fail(f"several records match branch {env('WF_BRANCH')}; keep one authoritative story record")
    return matches[0] if matches else None


def current(defs):
    path = find_order(defs)
    if path is None:
        raise Fail(f"no work order for branch {env('WF_BRANCH')} — start one with `ramify new <slug> --workflow <name>`")
    meta, body = read_order(path, defs)
    wf = workflow(defs, order_workflow(defs, meta))
    if meta["lane"] not in wf["lanes"]:
        raise Fail(f"{path}: lane '{meta['lane']}' is not one of {order_workflow(defs, meta)}'s")
    return path, meta, body, wf


def recorded(meta):
    return [s.get("step") for s in meta["steps"]]


def pending(meta, wf):
    done = set(recorded(meta))
    return [s for s in wf["lanes"][meta["lane"]] if s not in done]


def today():
    return datetime.date.today()


# ── Commands ─────────────────────────────────────────────────────────────────

def cmd_list(defs, _args):
    for name, wf in defs["workflows"].items():
        lanes = " ".join(f"[{l}]" if l == default_lane(wf) else l for l in lane_names(wf))
        print(f"  {name:<10} {lanes:<28} {wf.get('does', '')}")


def cmd_show(defs, args):
    if not args:
        raise Fail("usage: ramify workflow show <workflow>")
    wf = workflow(defs, args[0])
    print(f"{args[0]} — {wf.get('does', '')}   branch {wf.get('branch', '{slug}')}")
    for lane, seq in wf["lanes"].items():
        print(f"\n  {lane}{' (default)' if lane == default_lane(wf) else ''}")
        for s in seq:
            d = defs["steps"][s]
            gate = "human gate" if d.get("gate") == "human" else ""
            print(f"    {s:<10} {d.get('skill', ''):<18} {gate:<10} {d.get('does', '')}")
    for tag, lane in (wf.get("min_lane") or {}).items():
        print(f"  tag {tag} → at least {lane}")


def cmd_branch(defs, args):
    """Also checks the lane, so `ramify new` refuses a bad one before it cuts the worktree."""
    pos, opts = options(args, values=("lane", "tag"))
    if len(pos) != 2:
        raise Fail("usage: workflow.py branch <workflow> <slug> [--lane L]")
    check_slug(defs, pos[1])
    wf = workflow(defs, pos[0])
    for lane in opts.get("lane", []):
        if lane not in wf["lanes"]:
            raise Fail(f"no lane '{lane}' in {pos[0]} — one of: {', '.join(lane_names(wf))}")
    print(wf.get("branch", "{slug}").replace("{slug}", pos[1]))


def check_slug(defs, slug):
    if not SLUG.fullmatch(slug):
        raise Fail(f"'{slug}' is not a slug: letters, digits, '.', '_' and '-' only")
    # A merged work order stays on main: reusing its slug would cut a branch with no order.
    base, rel = env("WF_BASE"), os.path.join(defs.get("work_dir") or ".ramify/work", slug, "order.md")
    if base and env("WF_PRIMARY") and subprocess.run(
            ["git", "-C", env("WF_PRIMARY"), "cat-file", "-e", f"{base}:{rel}"],
            capture_output=True).returncode == 0:
        raise Fail(f"{base} already has {rel} — pick another slug")


def options(args, flags=(), values=()):
    """--flag and --key VALUE (repeatable); returns (positional, {key: [values] or True})."""
    pos, opts, i = [], {}, 0
    while i < len(args):
        a = args[i]
        if a.startswith("--") and a[2:] in flags:
            opts[a[2:]] = True
        elif a.startswith("--") and a[2:] in values:
            if i + 1 >= len(args):
                raise Fail(f"{a} needs a value")
            opts.setdefault(a[2:], []).append(args[i + 1])
            i += 1
        elif a.startswith("--"):
            raise Fail(f"unknown option {a}")
        else:
            pos.append(a)
        i += 1
    return pos, opts


def cmd_start(defs, args):
    check_native_start(defs)
    pos, opts = options(args, values=("lane", "tag"))
    if len(pos) != 2:
        raise Fail("usage: workflow.py start <workflow> <slug> [--lane L] [--tag T]...")
    name, slug = pos
    if not SLUG.fullmatch(slug):
        raise Fail(f"'{slug}' is not a slug: letters, digits, '.', '_' and '-' only")
    wf = workflow(defs, name)
    tags = [t.lower() for t in opts.get("tag", [])]
    known = wf.get("min_lane") or {}
    for t in tags:
        if t not in known:
            print(f"  tag             '{t}' sets no lane floor in {name} (those that do: {', '.join(known) or 'none'})")
    lane = (opts.get("lane") or [default_lane(wf)])[-1]
    if lane not in wf["lanes"]:
        raise Fail(f"no lane '{lane}' in {name} — one of: {', '.join(lane_names(wf))}")
    asked = lane
    for t in tags:
        floor = (wf.get("min_lane") or {}).get(t)
        if floor:
            lane = heavier(wf, lane, floor)
    folder = os.path.join(work_dir(defs, env("WF_ROOT")), slug)
    path = os.path.join(folder, "order.md")
    if os.path.exists(path):
        raise Fail(f"{os.path.relpath(path, env('WF_ROOT'))} already exists — pick another slug")
    os.makedirs(folder, exist_ok=True)
    meta = {"workflow": name, "lane": lane, "branch": env("WF_BRANCH"), "tags": tags, "created": today(), "steps": []}
    write_order(path, meta, f"\n# {slug}\n\nWhat this changes and why.\n")
    print(f"  work order      {os.path.relpath(path, env('WF_ROOT'))} ({name}, {lane} lane)")
    if lane != asked:
        print(f"  lane            raised from {asked} to {lane} by its tags")


def progress(defs):
    """This branch's work order as data, or None when it has none."""
    path = find_order(defs)
    if path is None:
        return None
    path, meta, _, wf = current(defs)
    lane = wf["lanes"][meta["lane"]]
    todo = pending(meta, wf)
    nxt = todo[0] if todo else None
    by_step = {s.get("step"): s for s in meta["steps"]}
    step = defs["steps"][nxt] if nxt else {}
    return {
        "workflow": order_workflow(defs, meta), "lane": meta["lane"], "order": os.path.relpath(path, env("WF_ROOT")),
        "done": len(lane) - len(todo), "total": len(lane),
        "next": {"step": nxt, "skill": step.get("skill"), "gate": step.get("gate", "auto")} if nxt else None,
        "asked": open_ask(meta),
        "steps": [{"step": s, "state": ("na" if by_step[s].get("na") else "done") if s in by_step
                   else "next" if s == nxt else "todo"} for s in lane],
    }


def line(p):
    nxt = p["next"]
    where = f"→ {nxt['step']}{' (human)' if nxt['gate'] == 'human' else ''}" if nxt else "✓ done"
    if p.get("asked"):
        where = f"⏳ {p['asked']} waits on you"
    return f"{p['workflow']}/{p['lane']} {p['done']}/{p['total']} {where}"


def cmd_status(defs, args):
    if "--json" in args or "--line" in args:
        # The dashboard and the status line read this: a broken order is data, not a crash.
        try:
            p = progress(defs)
        except Fail as e:
            p = {"error": str(e)}
        if "--json" in args:
            print(json.dumps(p))
        elif p:
            # The card and the status line are narrow: the reason is in `ramify step`.
            print("workflow: broken, see `ramify step`" if "error" in p else line(p))
        return
    path = find_order(defs)
    if path is None:
        raise Fail(f"no work order for branch {env('WF_BRANCH')}")
    path, meta, _, wf = current(defs)
    lane = wf["lanes"][meta["lane"]]
    todo = pending(meta, wf)
    nxt = todo[0] if todo else None
    rel = os.path.relpath(path, env("WF_ROOT"))
    print(f"{'repository ' if defs.get('record') else ''}{order_workflow(defs, meta)} · {meta['lane']} lane · {rel}")
    by_step = {s.get("step"): s for s in meta["steps"]}
    for s in lane:
        d = defs["steps"][s]
        if s in by_step:
            r = by_step[s]
            date = r.get(record_option(defs, "date_field", "date"), "")
            note = r.get(record_option(defs, "note_field", "note"), "")
            print(f"  ✓ {s:<10} {date}{'  n/a' if r.get('na') else ''}{'  ' + note if note else ''}")
        elif s == nxt:
            gate = human_gate_note(defs, path, meta, s) if d.get("gate") == "human" else ""
            print(f"  → {s:<10} {d.get('skill', '')} — {d.get('does', '')}{gate}")
        else:
            print(f"  · {s}")
    if nxt is None:
        print("  every step is recorded")


def human_gate_note(defs, path, meta, step):
    if open_ask(meta) == step:
        return f"  (human gate: asked, waits on `ramify approve {step}` or the approvals window)"
    missing = missing_gate_files(defs, path, step)
    if missing:
        return f"  (human gate: write {', '.join(missing)}, then Ramify asks)"
    return "  (human gate: record it only after the user's explicit OK)"


def record(defs, step, na, args):
    if not step or step.startswith("--"):
        raise Fail(f"usage: ramify step {'na' if na else 'done'} <step> [--note TEXT]")
    _, opts = options(args, flags=("approved",), values=("note",))
    path, meta, body, wf = current(defs)
    todo = pending(meta, wf)
    if step in recorded(meta):
        raise Fail(f"{step} is already recorded")
    if step not in wf["lanes"][meta["lane"]]:
        raise Fail(f"{step} is not a step of the {meta['lane']} lane")
    if step != todo[0]:
        raise Fail(f"the next step is {todo[0]}, not {step}")
    if not na and defs["steps"][step].get("gate") == "human" and not opts.get("approved"):
        raise Fail(f"{step} is a human gate: record it with --approved, and only after the user's explicit OK")
    entry = step_entry(defs, step)
    if na:
        entry["na"] = True
    if opts.get("note"):
        entry[record_option(defs, "note_field", "note")] = opts["note"][-1]
    meta["steps"].append(entry)
    if (meta.get("ask") or {}).get("step") == step:
        del meta["ask"]  # an OK given in the chat answers the window's request too
    write_order(path, meta, body)
    rest = todo[1:]
    print(f"  recorded {step}{' (n/a)' if na else ''} — next: {rest[0] if rest else 'nothing, every step is recorded'}")


# ── Approvals ── the agent asks, the human decides in the approvals window (or the CLI), the
# agent waits. The request lives in the work order's frontmatter as `ask:`, so it travels with
# the branch and every reader sees the same state.

def open_ask(meta):
    """The step that waits on the human, or None. A rejected request is answered, not open."""
    ask = meta.get("ask")
    return ask.get("step") if isinstance(ask, dict) and not ask.get("decision") else None


def gate_step(defs, meta, wf, step):
    todo = pending(meta, wf)
    if step not in wf["lanes"][meta["lane"]]:
        raise Fail(f"{step} is not a step of the {meta['lane']} lane")
    if step in recorded(meta):
        raise Fail(f"{step} is already recorded")
    if step != todo[0]:
        raise Fail(f"the next step is {todo[0]}, not {step}")
    if defs["steps"][step].get("gate") != "human":
        raise Fail(f"{step} is not a human gate: record it with `ramify step done {step}`")


def now():
    return datetime.datetime.now().replace(microsecond=0).isoformat()


def approval_digests(defs, order, files):
    result = {}
    for rel in files:
        path = os.path.realpath(os.path.join(env("WF_ROOT"), rel))
        if os.path.commonpath([path, os.path.realpath(env("WF_ROOT"))]) != os.path.realpath(env("WF_ROOT")):
            raise Fail("approval file is outside the worktree")
        if path == os.path.realpath(order):
            meta, body = read_order(path, defs)
            meta.pop("ask", None)
            meta.pop("steps", None)
            data = (json.dumps(meta, sort_keys=True, default=str) + body).encode()
        else:
            with open(path, "rb") as f:
                data = f.read()
        result[rel] = hashlib.sha256(data).hexdigest()
    return result


def deny_tool(reason):
    print(json.dumps({"hookSpecificOutput": {"hookEventName": "PreToolUse", "permissionDecision": "deny", "permissionDecisionReason": reason}}))


def missing_gate_files(defs, path, step):
    folder = os.path.dirname(path)
    names = defs["steps"][step].get("files", [f"{step}.md"])
    return [os.path.relpath(os.path.join(folder, f), env("WF_ROOT")) for f in names if not os.path.isfile(os.path.join(folder, f))]


def gate_reason(defs, path, meta, step):
    """What the agent reads when the gate refuses a tool call: always a next move, never a dead end."""
    root = env("WF_ROOT")
    scope = (f"Until then only reads and planning edits run in {root}; other worktrees and repositories are not held. "
             "The user can also give the OK in the chat: record it with "
             f"`ramify step done {step} --approved`.")
    if open_ask(meta) == step:
        return (f"{step} waits on the human: approve it in the approvals window, or run "
                f"`ramify approve {step}` in {root}. {scope}")
    missing = missing_gate_files(defs, path, step)
    if missing:
        return (f"{step} is a human gate, and Ramify asks for it once {', '.join(missing)} exist. "
                f"Write {'it' if len(missing) == 1 else 'them'} now (planning edits are allowed): the request "
                f"then opens by itself. {scope}")
    return f"{step} is a human gate that nobody asked for yet: run `ramify step ask {step}`. {scope}"


def cmd_guard(defs, _args):
    """Hold the gated worktree while a human gate is next. Reads, planning edits and anything
    aimed at another directory pass, so a gate never stops the agent from opening itself."""
    if not defs.get("auto_ask"):
        return
    from native_guard import planning_tool, targets_root
    payload = json.load(sys.stdin)
    root = env("WF_ROOT")
    patterns = defs.get("planning_paths") or ["docs/**", ".ramify/**", ".claude/**"]
    if not targets_root(payload, root) or planning_tool(payload, root, patterns):
        return
    try:
        if find_order(defs) is None:
            return
        path, meta, _, wf = current(defs)
    except Fail as error:
        deny_tool(f"Cannot verify the configured human gate: {error}. Fix the story record; "
                  "reads, planning edits and other worktrees still run.")
        return
    todo = pending(meta, wf)
    if todo and defs["steps"][todo[0]].get("gate") == "human":
        deny_tool(gate_reason(defs, path, meta, todo[0]))


def ready_human_step(defs, path, meta, wf):
    todo = pending(meta, wf)
    if not todo or defs["steps"][todo[0]].get("gate") != "human":
        return None
    names = defs["steps"][todo[0]].get("files", [f"{todo[0]}.md"])
    return todo[0] if all(os.path.isfile(os.path.join(os.path.dirname(path), f)) for f in names) else None


def same_request(defs, path, ask, step):
    if ask.get("step") != step:
        return False
    return ask.get("digests") == approval_digests(defs, path, ask.get("files") or [])


def cmd_ensure(defs, _args):
    if not defs.get("auto_ask") or find_order(defs) is None:
        return
    path, meta, _, wf = current(defs)
    step = ready_human_step(defs, path, meta, wf)
    if step is None:
        return
    if not same_request(defs, path, meta.get("ask") or {}, step):
        cmd_ask(defs, [step])


def bind_approval(defs, path, meta, files):
    if defs.get("auto_ask"):
        meta["ask"]["digests"] = approval_digests(defs, path, files)


def cmd_ask(defs, args):
    pos, opts = options(args, values=("file",))
    if len(pos) != 1:
        raise Fail("usage: ramify step ask <step> [--file PATH]...")
    step = pos[0]
    path, meta, body, wf = current(defs)
    gate_step(defs, meta, wf, step)
    root, folder = env("WF_ROOT"), os.path.dirname(path)
    files = []
    for f in requested_files(defs, step, folder, opts):
        full = os.path.realpath(f if os.path.isabs(f) else os.path.join(root, f))
        if not os.path.isfile(full):
            raise Fail(f"{f}: no such file — write it before you ask, or name it with --file")
        if os.path.commonpath([full, os.path.realpath(root)]) != os.path.realpath(root):
            raise Fail(f"{f} is outside the worktree")
        files.append(os.path.relpath(full, os.path.realpath(root)))
    order = os.path.relpath(os.path.realpath(path), os.path.realpath(root))
    if order not in files:
        files.append(order)
    meta["ask"] = {"step": step, "asked": now(), "files": files}
    bind_approval(defs, path, meta, files)
    write_order(path, meta, body)
    print(f"  {step} waits on the human — the approvals window shows {', '.join(files)}")


def check_approval_content(defs, path, meta):
    digests = meta["ask"].get("digests")
    if not digests:
        return
    if digests != approval_digests(defs, path, meta["ask"].get("files") or []):
        raise Fail("approval files changed; refresh the request before deciding")


def cmd_decide(defs, args, approve):
    pos, opts = options(args, values=("note",))
    verb = "approve" if approve else "reject"
    if len(pos) != 1:
        raise Fail(f"usage: ramify {verb} <step> {'[--note TEXT]' if approve else '--note TEXT'}")
    note = (opts.get("note") or [""])[-1].strip()
    if not approve and not note:
        raise Fail("a rejection needs --note: say what to change")
    step = pos[0]
    path, meta, body, wf = current(defs)
    gate_step(defs, meta, wf, step)
    # The human decides the next gate whether or not the agent asked: a missing request is the
    # agent's slip, and it must never leave the human unable to answer.
    if open_ask(meta) == step:
        check_approval_content(defs, path, meta)
    else:
        meta["ask"] = {"step": step, "asked": now(), "files": []}
    if approve:
        entry = step_entry(defs, step)
        if note:
            entry[record_option(defs, "note_field", "note")] = note
        meta["steps"].append(entry)
        del meta["ask"]
    else:
        meta["ask"].update({"decision": "rejected", "decided": now(), "note": note})
    write_order(path, meta, body)
    print(f"  {step} {'approved' if approve else 'rejected'}{': ' + note if note else ''}")


def cmd_wait(defs, args):
    """Exit 0 with `approved` or `rejected: <note>` on stdout; 1 on timeout. Polls the file: the
    decision can come from the window, the CLI or the chat, and they all end in the work order."""
    pos, opts = options(args, values=("timeout",))
    if len(pos) != 1:
        raise Fail("usage: ramify step wait <step> [--timeout SECONDS]")
    step = pos[0]
    deadline = time.monotonic() + float((opts.get("timeout") or [86400])[-1])
    while True:
        path, meta, _, wf = current(defs)
        by_step = {s.get("step"): s for s in meta["steps"]}
        if step in by_step:
            note = by_step[step].get(record_option(defs, "note_field", "note"))
            print(f"approved{': ' + note if note else ''}")
            return 0
        ask = meta.get("ask") or {}
        if ask.get("step") == step and ask.get("decision") == "rejected":
            print(f"rejected: {ask.get('note', '')}")
            return 0
        if ask.get("step") != step:
            raise Fail(f"nothing asks for a decision on {step} — run `ramify step ask {step}` first")
        if time.monotonic() >= deadline:
            print(f"no decision on {step} yet")
            return 1
        time.sleep(float(env("WF_POLL") or 2))


def cmd_asked(defs, args):
    """The approvals window reads this: the open request and the text of its files."""
    path = find_order(defs)
    meta = read_order(path, defs)[0] if path else {}
    step = open_ask(meta)
    if not step:
        print("null")
        return
    root = os.path.realpath(env("WF_ROOT"))
    files = []
    for rel in meta["ask"].get("files") or []:
        full = os.path.realpath(os.path.join(root, rel))
        try:
            if os.path.commonpath([full, root]) != root:
                raise OSError("outside the worktree")
            with open(full, encoding="utf-8", errors="replace") as f:
                text = f.read(512 << 10)
        except OSError as e:
            text = f"(cannot read {rel}: {e})"
        files.append({"path": rel, "text": text})
    print(json.dumps({"step": step, "does": (defs["steps"].get(step) or {}).get("does"), "asked": meta["ask"].get("asked"),
                      "workflow": order_workflow(defs, meta), "lane": meta["lane"], "branch": order_branch(defs, meta), "files": files},
                     default=str))


def cmd_lane(defs, args):
    if len(args) != 1:
        raise Fail("usage: ramify lane <lane>")
    path, meta, body, wf = current(defs)
    lane = args[0]
    if lane not in wf["lanes"]:
        raise Fail(f"no lane '{lane}' in {order_workflow(defs, meta)} — one of: {', '.join(lane_names(wf))}")
    if heavier(wf, lane, meta["lane"]) != lane:
        raise Fail(f"{lane} is lighter than {meta['lane']}: a lane is raised, never lowered")
    if lane == meta["lane"]:
        print(f"  already on the {lane} lane")
        return
    meta["lane"] = lane
    write_order(path, meta, body)
    print(f"  lane raised to {lane} — next: {(pending(meta, wf) or ['nothing'])[0]}")


def cmd_human_gate(defs):
    if find_order(defs) is None or not defs.get("auto_ask"):
        return 0
    path, meta, _, wf = current(defs)
    todo = pending(meta, wf)
    if todo and defs["steps"][todo[0]].get("gate") == "human":
        print(f"{todo[0]} needs human approval in Ramify before implementation ({os.path.relpath(path, env('WF_ROOT'))})")
        return 1
    return 0


def cmd_gate(defs, args):
    """`gate --pr` gates on the lane's first `opens_pr` step; a lane without one opens PRs freely."""
    if len(args) != 1:
        raise Fail("usage: workflow.py gate <step> | --pr")
    if find_order(defs) is None:
        return 0  # a branch without a work order is not in a workflow
    path, meta, _, wf = current(defs)
    lane = wf["lanes"][meta["lane"]]
    target = args[0]
    if target == "--pr":
        target = next((s for s in lane if (defs["steps"][s] or {}).get("opens_pr")), None)
    if target not in lane:
        return 0
    missing = [s for s in lane[:lane.index(target)] if s not in recorded(meta)]
    if missing:
        d = defs["steps"][missing[0]] or {}
        print(f"{target} waits on: {', '.join(missing)} ({os.path.relpath(path, env('WF_ROOT'))}). "
              f"Next: {missing[0]}{' — invoke ' + d['skill'] if d.get('skill') else ''}"
              f"{', a human gate' if d.get('gate') == 'human' else ''}.")
        return 1
    return 0


def off_status(args):
    if "--json" in args:
        print("null")
    elif "--line" not in args:
        print("workflows are off for this repo")
    return 0


def off_output(cmd, args):
    if cmd == "status":
        return off_status(args)
    if cmd == "asked":
        print("null")
        return 0
    if cmd in {"ensure", "guard", "gate"}:
        return 0
    if cmd in {"branch", "start", "done", "na", "lane", "ask", "approve", "reject", "wait"}:
        print("ramify: workflows are off for this repo", file=sys.stderr)
        return 1
    print("workflows are off for this repo")
    return 0


def deny_gate_error(error):
    # The workflow definitions themselves are broken: say so, but hold nothing. A gate that cannot
    # be read cannot be opened either, and a hold that cannot be lifted is the deadlock to avoid.
    print(f"ramify: cannot verify the configured human gate: {error}", file=sys.stderr)
    return 0


def status_error(args, error):
    if "--json" in args:
        print(json.dumps({"error": str(error)}))
        return 0
    if "--line" in args:
        print("workflow: broken, see `ramify step`")
        return 0
    print(f"ramify: {error}", file=sys.stderr)
    return 1


def fail_output(cmd, args, error):
    if cmd == "guard":
        return deny_gate_error(error)
    if cmd == "status":
        return status_error(args, error)
    if cmd == "asked":
        print("null")
        return 0
    print(f"ramify: {error}", file=sys.stderr)
    return 2 if cmd == "gate" else 1


def main(argv):
    if not argv:
        print(__doc__.strip())
        return 2
    cmd, args = argv[0], argv[1:]
    try:
        if env('RAMIFY_WORKFLOW_SOURCE') not in ('', 'auto', 'ramify', 'repo', 'select'):
            raise Fail('RAMIFY_WORKFLOW_SOURCE must be auto, ramify, or repo')
        defs = load_definitions()
        explicit = any(os.path.isfile(os.path.join(base, '.ramify', 'workflows.yml'))
                       for base in (env('WF_ROOT'), env('WF_PRIMARY')) if base)
        if env('RAMIFY_WORKFLOW_SOURCE') in ('repo', 'select') and not explicit and not find_order(defs):
            native = runpy.run_path(os.path.join(os.path.dirname(__file__), 'repo-workflows.py'))
            try:
                return native['route'](cmd, args)
            except native['Fail'] as exc:
                raise Fail(str(exc))
        table = {
            "list": cmd_list, "show": cmd_show, "branch": cmd_branch, "start": cmd_start,
            "status": cmd_status, "lane": cmd_lane, "gate": lambda d, a: cmd_human_gate(d) if a == ["--human"] else cmd_gate(d, a),
            "done": lambda d, a: record(d, a[0] if a else "", False, a[1:]),
            "na": lambda d, a: record(d, a[0] if a else "", True, a[1:]),
            "ask": cmd_ask, "guard": cmd_guard, "ensure": cmd_ensure, "wait": cmd_wait, "asked": cmd_asked,
            "approve": lambda d, a: cmd_decide(d, a, True),
            "reject": lambda d, a: cmd_decide(d, a, False),
        }
        if cmd not in table:
            raise Fail(f"unknown command {cmd}")
        return table[cmd](defs, args) or 0
    except Off:
        return off_output(cmd, args)
    except Fail as e:
        return fail_output(cmd, args, e)



if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
