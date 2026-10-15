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
import time
import json
import os
import re
import subprocess
import sys

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "vendor"))
import yaml  # noqa: E402  (vendored PyYAML, pure python)


SLUG = re.compile(r"[A-Za-z0-9][A-Za-z0-9._-]*")


class Fail(Exception):
    pass


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
        extra = set(d or {}) - {"skill", "gate", "does", "opens_pr"}
        if extra:
            raise Fail(f"{path}: step {s} has unknown keys {sorted(map(str, extra))} — quote a `does` that holds a comma")
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

def read_order(path):
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
        meta = yaml.safe_load(text[4:end]) or {}
    except yaml.YAMLError as e:
        raise Fail(f"{path}: {e}")
    if not isinstance(meta, dict):
        raise Fail(f"{path}: the frontmatter is not a map")
    meta["steps"] = meta.get("steps") or []
    if not isinstance(meta["steps"], list) or not all(isinstance(e, dict) for e in meta["steps"]):
        raise Fail(f"{path}: `steps:` is not a list of {{step, date}} entries")
    for key in ("workflow", "lane", "branch"):
        if not isinstance(meta.get(key), str):
            raise Fail(f"{path}: no `{key}:`")
    return meta, text[end + 5:]


def write_order(path, meta, body):
    head = yaml.safe_dump(meta, sort_keys=False, default_flow_style=None, allow_unicode=True, width=1000)
    tmp = path + ".tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        f.write(f"---\n{head}---\n{body}")
    os.replace(tmp, path)


def find_order(defs):
    branch = env("WF_BRANCH")
    for path in sorted(glob.glob(os.path.join(work_dir(defs, env("WF_ROOT")), "*", "order.md"))):
        try:
            meta, _ = read_order(path)
        except Fail:
            continue
        if meta.get("branch") == branch:
            return path
    return None


def current(defs):
    path = find_order(defs)
    if path is None:
        raise Fail(f"no work order for branch {env('WF_BRANCH')} — start one with `ramify new <slug> --workflow <name>`")
    meta, body = read_order(path)
    wf = workflow(defs, meta["workflow"])
    if meta["lane"] not in wf["lanes"]:
        raise Fail(f"{path}: lane '{meta['lane']}' is not one of {meta['workflow']}'s")
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
        "workflow": meta["workflow"], "lane": meta["lane"], "order": os.path.relpath(path, env("WF_ROOT")),
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
    print(f"{meta['workflow']} · {meta['lane']} lane · {rel}")
    by_step = {s.get("step"): s for s in meta["steps"]}
    for s in lane:
        d = defs["steps"][s]
        if s in by_step:
            r = by_step[s]
            print(f"  ✓ {s:<10} {r.get('date', '')}{'  n/a' if r.get('na') else ''}{'  ' + r['note'] if r.get('note') else ''}")
        elif s == nxt:
            gate = "  (human gate: record it only after the user's explicit OK)" if d.get("gate") == "human" else ""
            print(f"  → {s:<10} {d.get('skill', '')} — {d.get('does', '')}{gate}")
        else:
            print(f"  · {s}")
    if nxt is None:
        print("  every step is recorded")


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
    entry = {"step": step, "date": today()}
    if na:
        entry["na"] = True
    if opts.get("note"):
        entry["note"] = opts["note"][-1]
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


def cmd_ask(defs, args):
    pos, opts = options(args, values=("file",))
    if len(pos) != 1:
        raise Fail("usage: ramify step ask <step> [--file PATH]...")
    step = pos[0]
    path, meta, body, wf = current(defs)
    gate_step(defs, meta, wf, step)
    root, folder = env("WF_ROOT"), os.path.dirname(path)
    files = []
    for f in opts.get("file") or [os.path.join(folder, f"{step}.md")]:
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
    write_order(path, meta, body)
    print(f"  {step} waits on the human — the approvals window shows {', '.join(files)}")


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
    if open_ask(meta) != step:
        raise Fail(f"nothing asks for a decision on {step} — the agent puts it up with `ramify step ask {step}`")
    gate_step(defs, meta, wf, step)
    if approve:
        entry = {"step": step, "date": today()}
        if note:
            entry["note"] = note
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
            note = by_step[step].get("note")
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
    meta = read_order(path)[0] if path else {}
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
                      "workflow": meta["workflow"], "lane": meta["lane"], "branch": meta["branch"], "files": files},
                     default=str))


def cmd_lane(defs, args):
    if len(args) != 1:
        raise Fail("usage: ramify lane <lane>")
    path, meta, body, wf = current(defs)
    lane = args[0]
    if lane not in wf["lanes"]:
        raise Fail(f"no lane '{lane}' in {meta['workflow']} — one of: {', '.join(lane_names(wf))}")
    if heavier(wf, lane, meta["lane"]) != lane:
        raise Fail(f"{lane} is lighter than {meta['lane']}: a lane is raised, never lowered")
    if lane == meta["lane"]:
        print(f"  already on the {lane} lane")
        return
    meta["lane"] = lane
    write_order(path, meta, body)
    print(f"  lane raised to {lane} — next: {(pending(meta, wf) or ['nothing'])[0]}")


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


def main(argv):
    if not argv:
        print(__doc__.strip())
        return 2
    cmd, args = argv[0], argv[1:]
    try:
        defs = load_definitions()
        table = {
            "list": cmd_list, "show": cmd_show, "branch": cmd_branch, "start": cmd_start,
            "status": cmd_status, "lane": cmd_lane, "gate": cmd_gate,
            "done": lambda d, a: record(d, a[0] if a else "", False, a[1:]),
            "na": lambda d, a: record(d, a[0] if a else "", True, a[1:]),
            "ask": cmd_ask, "wait": cmd_wait, "asked": cmd_asked,
            "approve": lambda d, a: cmd_decide(d, a, True),
            "reject": lambda d, a: cmd_decide(d, a, False),
        }
        if cmd not in table:
            raise Fail(f"unknown command {cmd}")
        return table[cmd](defs, args) or 0
    except Off:
        if cmd == "status" and "--json" in args:
            print("null")
            return 0
        if cmd == "status" and "--line" in args:
            return 0
        if cmd == "gate":
            return 0
        if cmd == "asked":
            print("null")
            return 0
        if cmd in ("branch", "start", "done", "na", "lane", "ask", "approve", "reject", "wait"):
            print("ramify: workflows are off for this repo", file=sys.stderr)
            return 1
        print("workflows are off for this repo")
        return 0
    except Fail as e:
        if cmd == "gate":
            print(f"ramify: {e}", file=sys.stderr)
            return 2  # not "steps missing": the hook lets the PR through on this one
        if cmd == "status" and "--json" in args:
            print(json.dumps({"error": str(e)}))
            return 0
        if cmd == "status" and "--line" in args:
            print("workflow: broken, see `ramify step`")
            return 0
        if cmd == "asked":
            print("null")  # a broken order is the dashboard's to report; the window shows what it can
            return 0
        print(f"ramify: {e}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
