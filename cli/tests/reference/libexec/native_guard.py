"""Allow planning edits while a repository-native human gate is pending."""
import fnmatch
import os
import re
import shlex
import subprocess

EDIT_TOOLS = {"write", "edit", "multiedit", "apply_patch"}
COMMAND_TOOLS = {"bash", "exec_command", "exec"}
READ_PROGRAMS = {"cat", "rg", "ls", "pwd", "head", "tail", "wc", "stat"}
GIT_READS = {"status", "diff", "log", "show", "rev-parse", "ls-files", "check-ignore"}
RAMIFY_READS = {"step", "workflow", "gates", "approvals", "card", "config", "status", "dash"}
SHELLS = {"bash", "sh", "zsh"}
# Output thrown away or merged writes nothing: `2>&1`, `>/dev/null`, `2>/dev/null`.
HARMLESS_REDIRECTS = re.compile(r"(?<![<>])\d?>(?:&\d|\s*/dev/null)(?![\w/])")


def inside(path, root):
    full, base = os.path.realpath(path), os.path.realpath(root)
    return os.path.commonpath([full, base]) == base


def command_dir(tokens, cwd):
    """The directory the command runs in: the session's, moved by each leading `cd DIR`."""
    groups = shell_groups(tokens)
    for group in groups[:-1]:
        if group[0] != "cd" or len(group) != 2:
            break
        cwd = os.path.join(cwd, os.path.expanduser(group[1]))
    return cwd


def targets_root(payload, root):
    """Whether the tool call lands in the gated worktree. A gate holds its own worktree only:
    an edit or a command elsewhere (another worktree, another repository) is not its business."""
    values = tool_values(payload)
    base = payload.get("cwd") or root
    path = first_string(values, ["file_path", "path"])
    tool = payload.get("tool_name", "").split(".")[-1].lower()
    if edit_target(tool, path):
        return inside(os.path.join(base, path), root)
    command = first_string(values, ["command", "cmd"])
    if not command:
        return True
    if os.path.realpath(root) in command or root in command:
        return True
    try:
        return inside(command_dir(split_shell(command), base), root)
    except ValueError:
        return True


def split_shell(command):
    lexer = shlex.shlex(command, posix=True, punctuation_chars=";&|()")
    lexer.whitespace_split = True
    return list(lexer)


def planning_path(path, root, patterns):
    full = os.path.realpath(os.path.join(root, path))
    if os.path.commonpath([full, os.path.realpath(root)]) != os.path.realpath(root):
        return False
    relative = os.path.relpath(full, root)
    return any(fnmatch.fnmatchcase(relative, pattern) for pattern in patterns)


def staged_planning(root, patterns):
    result = subprocess.run(["git", "-C", root, "diff", "--cached", "--name-only", "-z"],
                            capture_output=True, check=True)
    return all(planning_path(p, root, patterns) for p in result.stdout.decode().split("\0") if p)


def read_command(tokens):
    if tokens[0] == "git":
        return git_read(tokens)
    if tokens[0] not in READ_PROGRAMS:
        return False
    return not any(t.startswith("--pre") for t in tokens)


def git_read(tokens):
    if len(tokens) < 2 or tokens[1] not in GIT_READS:
        return False
    return not any(t.startswith(("--output", "--ext-diff", "--textconv")) for t in tokens)


def shell_groups(tokens):
    groups, current = [], []
    for token in tokens:
        if token and set(token) <= set(";&|()"):
            if current:
                groups.append(current)
                current = []
        else:
            current.append(token)
    if current:
        groups.append(current)
    return groups


def shell_planning(command, root, patterns):
    command = HARMLESS_REDIRECTS.sub(" ", command)
    if any(part in command for part in ("$(", "`", ">", "<<")):
        return False
    try:
        groups = shell_groups(split_shell(command))
    except ValueError:
        return False
    return bool(groups) and all(shell_group(group, root, patterns) for group in groups)


def git_planning(tokens, root, patterns):
    if len(tokens) < 2:
        return False
    if tokens[1] == "commit":
        return staged_planning(root, patterns)
    if tokens[1] == "add":
        paths = [t for t in tokens[2:] if not t.startswith("-")]
        return bool(paths) and all(planning_path(p, root, patterns) for p in paths)
    return git_read(tokens)


def shell_group(tokens, root, patterns):
    executable = os.path.basename(tokens[0])
    if executable in SHELLS and len(tokens) > 1 and os.path.basename(tokens[1]) == "ramify":
        tokens, executable = tokens[1:], "ramify"  # bash /path/to/bin/ramify step ask review
    if executable == "ramify":
        return len(tokens) > 1 and tokens[1] in RAMIFY_READS
    if executable == "cd":
        return len(tokens) <= 2
    if executable == "git":
        return git_planning([executable, *tokens[1:]], root, patterns)
    return read_command([executable, *tokens[1:]])


def first_string(values, keys):
    return next((values[k] for k in keys if isinstance(values.get(k), str) and values[k]), None)


def patch_paths(patch):
    paths = re.findall(r"^\*\*\* (?:Update|Add|Delete) File: (.+)$", patch, re.MULTILINE)
    return paths + re.findall(r"^\*\*\* Move to: (.+)$", patch, re.MULTILINE)


def patch_planning(patch, root, patterns):
    paths = patch_paths(patch)
    return bool(paths) and all(planning_path(p, root, patterns) for p in paths)


def edit_target(tool, path):
    return path and tool in EDIT_TOOLS


def is_patch(patch):
    return patch and "*** Begin Patch" in patch


def tool_values(payload):
    values = payload.get("tool_input") or {}
    return values if isinstance(values, dict) else {"patch": values}


def planning_tool(payload, root, patterns):
    tool = payload.get("tool_name", "").split(".")[-1].lower()
    values = tool_values(payload)
    path = first_string(values, ["file_path", "path"])
    if edit_target(tool, path):
        return planning_path(path, root, patterns)
    patch = first_string(values, ["patch", "input"])
    if is_patch(patch):
        return patch_planning(patch, root, patterns)
    command = first_string(values, ["command", "cmd"])
    if command:
        return shell_planning(command, root, patterns)
    return tool not in EDIT_TOOLS | COMMAND_TOOLS
