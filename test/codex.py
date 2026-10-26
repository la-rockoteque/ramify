#!/usr/bin/env python3
"""Exercise Codex hook payloads against real Git worktrees and work orders."""
import json
import os
from pathlib import Path
import subprocess
import tempfile
import sys
import unittest

HOME = Path(__file__).resolve().parents[1]


class CodexHandoff(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.primary = self.root / "repo"
        self.tree = self.root / "handoff"
        self.primary.mkdir()
        self.env = {**os.environ,
                    "PATH": str(Path(sys.executable).parent) + os.pathsep + os.environ["PATH"],
                    "RAMIFY_STATE_DIR": str(self.root / "state"),
                    "RAMIFY_REGISTRY": str(self.root / "registry"), "RAMIFY_POPUP": "0",
                    "RAMIFY_AUTOSTART": "0"}
        for key in ("RAMIFY_ROOT", "CLAUDE_PROJECT_DIR", "RAMIFY_AGENT"):
            self.env.pop(key, None)
        self.git(self.primary, "init", "-q", "-b", "main")
        self.git(self.primary, "config", "user.email", "test@example.com")
        self.git(self.primary, "config", "user.name", "Test")
        (self.primary / ".ramify.conf").write_text("RAMIFY_SERVICES=''\n")
        (self.primary / "feature.txt").write_text("before\n")
        self.git(self.primary, "add", ".")
        self.git(self.primary, "commit", "-qm", "initial")
        self.git(self.primary, "worktree", "add", "-qb", "feat/handoff", str(self.tree))
        self.wf("start", "feature", "handoff")
        self.order = self.tree / ".ramify/work/handoff/order.md"

    def git(self, root, *args):
        return subprocess.run(["git", "-C", str(root), *args], env=self.env,
                              check=True, capture_output=True, text=True).stdout

    def wf(self, *args):
        env = {**self.env, "WF_ROOT": str(self.tree), "WF_PRIMARY": str(self.primary),
               "WF_BRANCH": "feat/handoff", "WF_HOME": str(HOME)}
        return subprocess.run([sys.executable, str(HOME / "libexec/workflow.py"), *args],
                              env=env, check=True, capture_output=True, text=True).stdout

    def hook(self, action, **payload):
        data = {"session_id": "codex-resume", "cwd": str(self.tree), "source": "resume", **payload}
        result = subprocess.run([str(HOME / "hooks/ramify-hook"), action],
                                input=json.dumps(data), env={**self.env, "RAMIFY_AGENT": "codex"},
                                cwd=self.primary, check=True, capture_output=True, text=True)
        return json.loads(result.stdout) if result.stdout.strip() else None

    def test_resume_preserves_unfinished_build(self):
        self.wf("done", "spec", "--approved")
        self.wf("done", "qa-plan", "--approved")
        (self.tree / "feature.txt").write_text("Claude unfinished change\n")
        (self.tree / "new.txt").write_text("Claude untracked change\n")
        before = self.order.read_bytes()
        status = self.git(self.tree, "status", "--porcelain")
        result = self.hook("session-start")["hookSpecificOutput"]
        self.assertIn("→ build", result["additionalContext"])
        self.assertIn("ramify:resume", result["additionalContext"])
        self.assertNotIn("sessionTitle", result)
        self.assertFalse((self.root / "state").exists())
        self.assertFalse((self.root / "registry").exists())
        self.assertEqual(before, self.order.read_bytes())
        self.assertEqual(status, self.git(self.tree, "status", "--porcelain"))
        self.assertEqual("Claude unfinished change\n", (self.tree / "feature.txt").read_text())
        self.assertEqual("Claude untracked change\n", (self.tree / "new.txt").read_text())

    def test_resume_preserves_pending_approval(self):
        (self.order.parent / "spec.md").write_text("# Spec\n")
        self.wf("ask", "spec")
        before = self.order.read_bytes()
        pending = self.wf("asked", "--json")
        result = self.hook("session-start")["hookSpecificOutput"]
        self.assertIn("→ spec", result["additionalContext"])
        self.assertIn("pending", result["additionalContext"])
        self.assertEqual(before, self.order.read_bytes())
        self.assertEqual(pending, self.wf("asked", "--json"))
        self.assertIn("waits on you", self.wf("status", "--line"))
        self.wf("reject", "spec", "--note", "add the missing failure state")
        rejected = self.order.read_bytes()
        self.hook("session-start")
        self.assertEqual(rejected, self.order.read_bytes())
        self.assertEqual("rejected: add the missing failure state\n",
                         self.wf("wait", "spec", "--timeout", "0"))

    def test_codex_package(self):
        manifest = json.loads((HOME / ".codex-plugin/plugin.json").read_text())
        self.assertEqual("./skills/", manifest["skills"])
        hooks = json.loads((HOME / manifest["hooks"]).read_text())["hooks"]
        self.assertEqual({"SessionStart", "SessionEnd", "PreToolUse", "PostToolUse", "Stop"}, set(hooks))
        for groups in hooks.values():
            for group in groups:
                for hook in group["hooks"]:
                    self.assertIn("RAMIFY_AGENT=codex", hook["command"])
                    self.assertIn("${PLUGIN_ROOT}", hook["command"])
        catalog = json.loads((HOME / ".agents/plugins/marketplace.json").read_text())
        self.assertEqual("./", catalog["plugins"][0]["source"]["path"])
        resume = (HOME / "skills/resume/SKILL.md").read_text()
        for command in ("git status --short --branch", "git diff", "ramify step asked --json"):
            self.assertIn(command, resume)

    def test_no_order(self):
        self.assertIsNone(self.hook("session-start", cwd=str(self.primary)))

    def test_codex_decision_guard(self):
        for command in ("ramify approve spec", "ramify run go 123"):
            result = self.hook("decide-guard", tool_input={"command": command})
            self.assertEqual("deny", result["hookSpecificOutput"]["permissionDecision"])

    def test_codex_worktree_output(self):
        marker = f"ramify: worktree {self.tree}\n"
        for output in (marker, {"output": marker}, {"stdout": marker}):
            with self.subTest(output=output):
                result = self.hook("worktree-add", tool_input={"command": "ramify new handoff"},
                                   tool_response=output)
                self.assertIsNotNone(result)
                self.assertIn("→ spec", result["hookSpecificOutput"]["additionalContext"])
                pin = self.root / "state/sessions/codex-resume.root"
                self.assertEqual(str(self.tree), pin.read_text().strip())
                resumed = self.hook("session-start", cwd=str(self.primary), source="compact")
                self.assertIn("→ spec", resumed["hookSpecificOutput"]["additionalContext"])
                self.hook("session-end")
                self.assertFalse(pin.exists())


if __name__ == "__main__":
    unittest.main()
