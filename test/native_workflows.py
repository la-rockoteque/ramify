#!/usr/bin/env python3
"""Verify native story gates without changing a real repository or opening a GUI."""
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HOME = Path(__file__).resolve().parents[1]
CONFIG = '''work_dir: docs/stories/*
record:
  filename: story.md
  workflow_field: kind
  slug_field: story
  date_field: "on"
  note_field: result
auto_ask: true
workflows:
  feature:
    branch: story/{slug}
    lanes:
      standard: [qa-plan, implement-build, review, create-pr]
steps:
  qa-plan: { gate: human, files: [story.md, qa-plan.md] }
  implement-build: {}
  review: { gate: human, files: [review.md, qa-results.md] }
  create-pr: { opens_pr: true }
'''


class NativeWorkflow(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.story = self.root / 'docs/stories/todo/example/story.md'
        self.story.parent.mkdir(parents=True)
        self.story.write_text('---\nstory: example\nkind: feature\nlane: standard\ntracker: {item: {key: TM-242}}\nsteps: []\n---\n\n# Native story\n')
        (self.root / '.ramify').mkdir()
        (self.root / '.ramify/workflows.yml').write_text(CONFIG)
        (self.root / '.ramify.conf').write_text("RAMIFY_SERVICES=''\n")
        self.env = {**os.environ, 'WF_ROOT': str(self.root), 'WF_PRIMARY': str(self.root),
                    'WF_BRANCH': 'story/example', 'WF_HOME': str(HOME), 'RAMIFY_ROOT': str(self.root),
                    'RAMIFY_POPUP': '0', 'RAMIFY_AGENT': 'codex',
                    # The hook calls ramify: keep this throwaway repo out of the machine's registry.
                    'RAMIFY_STATE_DIR': str(self.root / 'state'), 'RAMIFY_REGISTRY': str(self.root / 'registry')}
        self.run_git('init', '-q', '-b', 'story/example')

    def run_git(self, *args):
        return subprocess.run(['git', '-C', str(self.root), *args], check=True, capture_output=True, text=True)

    def wf(self, *args, success=True, payload=None):
        result = subprocess.run([sys.executable, str(HOME / 'libexec/workflow.py'), *args], env=self.env,
                                input=json.dumps(payload) if payload else None, capture_output=True, text=True)
        if success:
            self.assertEqual(0, result.returncode, result.stderr)
        return result

    def ready(self):
        (self.story.parent / 'qa-plan.md').write_text('# QA plan\n\nCheck quantities.\n')

    def test_discovers_story_without_shadow_order_or_added_metadata(self):
        before = self.story.read_bytes()
        status = json.loads(self.wf('status', '--json').stdout)
        self.assertEqual('docs/stories/todo/example/story.md', status['order'])
        self.assertEqual('qa-plan', status['next']['step'])
        self.assertEqual(before, self.story.read_bytes())
        self.assertFalse(list(self.root.rglob('order.md')))

    def test_ensure_waits_for_files_then_requests_both_once(self):
        self.wf('ensure')
        self.assertEqual('null', self.wf('asked', '--json').stdout.strip())
        self.ready()
        self.assertIn('waits on the human', self.wf('ensure').stdout)
        before = self.story.read_bytes()
        self.assertEqual('', self.wf('ensure').stdout)
        self.assertEqual(before, self.story.read_bytes())
        request = json.loads(self.wf('asked', '--json').stdout)
        self.assertEqual(['docs/stories/todo/example/story.md', 'docs/stories/todo/example/qa-plan.md'], [f['path'] for f in request['files']])

    def test_human_decision_records_native_fields_and_preserves_body(self):
        self.ready()
        self.wf('ensure')
        self.wf('approve', 'qa-plan', '--note', 'Synthetic test decision')
        text = self.story.read_text()
        self.assertIn("'on':", text)
        self.assertIn('result: Synthetic test decision', text)
        self.assertIn('key: TM-242', text)
        self.assertTrue(text.endswith('\n# Native story\n'))
        self.assertNotIn('workflow:', text)
        self.assertNotIn('branch:', text)
        self.assertNotIn('ask:', text)
        self.assertEqual('implement-build', json.loads(self.wf('status', '--json').stdout)['next']['step'])

    def test_changed_plan_rejects_stale_approval_and_refreshes(self):
        self.ready()
        self.wf('ensure')
        (self.story.parent / 'qa-plan.md').write_text('# Revised plan\n')
        stale = self.wf('approve', 'qa-plan', success=False)
        self.assertNotEqual(0, stale.returncode)
        self.assertIn('files changed', stale.stderr)
        self.assertIn('waits on the human', self.wf('ensure').stdout)
        self.wf('approve', 'qa-plan')

    def test_rejected_request_is_not_resubmitted_without_a_change(self):
        self.ready()
        self.wf('ensure')
        self.wf('reject', 'qa-plan', '--note', 'Add the error case')
        before = self.story.read_bytes()
        self.assertEqual('', self.wf('ensure').stdout)
        self.assertEqual(before, self.story.read_bytes())
        self.assertIn('Add the error case', self.wf('wait', 'qa-plan', '--timeout', '0').stdout)

    def test_existing_unquoted_on_fields_survive_native_round_trip(self):
        self.story.write_text(self.story.read_text().replace('steps: []', 'steps:\n  - {step: qa-plan, on: 2026-10-25, result: already approved}'))
        self.wf('done', 'implement-build')
        text = self.story.read_text()
        self.assertIn("'on': 2026-10-25", text)
        self.assertNotIn('true:', text)
        self.assertEqual(2, text.count("'on':"))

    def test_native_creation_refuses_a_second_work_order(self):
        result = self.wf('start', 'feature', 'another', success=False)
        self.assertNotEqual(0, result.returncode)
        self.assertIn('repository owns', result.stderr)
        self.assertFalse(list(self.root.rglob('order.md')))

    def test_pr_and_implementation_are_blocked_at_pending_human_gate(self):
        self.assertEqual(1, self.wf('gate', '--pr', success=False).returncode)
        self.assertEqual(1, self.wf('gate', '--human', success=False).returncode)
        for payload in [
            {'tool_name': 'Write', 'tool_input': {'file_path': 'src/app.ts', 'content': 'bad'}},
            {'tool_name': 'exec_command', 'tool_input': {'cmd': "python -c 'write_source()'"}},
            {'tool_name': 'apply_patch', 'tool_input': {'patch': '*** Begin Patch\n*** Add File: src/app.ts\n+x\n*** End Patch'}},
            {'tool_name': 'functions.exec', 'tool_input': {'code': 'await tools.exec_command({cmd:"write source"})'}},
        ]:
            with self.subTest(payload=payload):
                output = json.loads(self.wf('guard', payload=payload).stdout)
                self.assertEqual('deny', output['hookSpecificOutput']['permissionDecision'])

    def test_planning_edits_and_reads_remain_available(self):
        for payload in [
            {'tool_name': 'Write', 'tool_input': {'file_path': str(self.story)}},
            {'tool_name': 'Bash', 'tool_input': {'command': 'git diff; rg quantity docs'}},
            {'tool_name': 'apply_patch', 'tool_input': {'patch': '*** Begin Patch\n*** Update File: docs/plan.md\n+x\n*** End Patch'}},
        ]:
            with self.subTest(payload=payload):
                self.assertEqual('', self.wf('guard', payload=payload).stdout)
        self.ready()
        self.wf('ensure')
        self.wf('approve', 'qa-plan')
        self.assertEqual('', self.wf('guard', payload={'tool_name': 'Write', 'tool_input': {'file_path': 'src/app.ts'}}).stdout)

    def test_story_move_is_discovered_and_duplicate_records_are_rejected(self):
        moved = self.root / 'docs/stories/active/example'
        moved.parent.mkdir(parents=True)
        self.story.parent.rename(moved)
        self.story = moved / 'story.md'
        self.assertIn('docs/stories/active/example/story.md', self.wf('status').stdout)
        copy = self.root / 'docs/stories/completed/example/story.md'
        copy.parent.mkdir(parents=True)
        copy.write_bytes(self.story.read_bytes())
        payload = {'tool_name': 'Write', 'tool_input': {'file_path': 'src/app.ts'}}
        result = json.loads(self.wf('guard', payload=payload).stdout)
        self.assertIn('several records', result['hookSpecificOutput']['permissionDecisionReason'])

    def test_broken_native_record_fails_closed(self):
        self.story.write_text(self.story.read_text().replace('kind: feature', 'kind: missing'))
        payload = {'tool_name': 'Write', 'tool_input': {'file_path': 'src/app.ts'}}
        result = json.loads(self.wf('guard', payload=payload).stdout)
        self.assertEqual('deny', result['hookSpecificOutput']['permissionDecision'])

    def test_auto_hook_requests_and_guard_hook_denies_source_edit(self):
        self.ready()
        payload = {'cwd': str(self.root), 'session_id': 'synthetic', 'tool_name': 'Write', 'tool_input': {'file_path': 'src/app.ts'}}
        for action in ['auto-gate', 'human-gate', 'pr-gate']:
            result = subprocess.run([str(HOME / 'hooks/ramify-hook'), action], env=self.env,
                                    input=json.dumps(payload), capture_output=True, text=True)
            self.assertEqual(0, result.returncode, result.stderr)
            if action in {'human-gate', 'pr-gate'}:
                self.assertEqual('deny', json.loads(result.stdout)['hookSpecificOutput']['permissionDecision'])
        self.assertEqual('qa-plan', json.loads(self.wf('asked', '--json').stdout)['step'])


    def deny_reason(self, payload):
        output = self.wf('guard', payload=payload).stdout
        return json.loads(output)['hookSpecificOutput']['permissionDecisionReason'] if output else ''

    def test_gate_holds_its_own_worktree_only(self):
        elsewhere = tempfile.mkdtemp()
        self.addCleanup(lambda: os.rmdir(elsewhere))
        for payload in [
            {'cwd': str(self.root), 'tool_name': 'Edit', 'tool_input': {'file_path': os.path.join(elsewhere, 'app.ts')}},
            {'cwd': str(self.root), 'tool_name': 'Bash', 'tool_input': {'command': f'cd {elsewhere} && npm test'}},
            {'cwd': elsewhere, 'tool_name': 'Bash', 'tool_input': {'command': 'npm test'}},
        ]:
            with self.subTest(payload=payload):
                self.assertEqual('', self.wf('guard', payload=payload).stdout)
        held = {'cwd': elsewhere, 'tool_name': 'Bash', 'tool_input': {'command': f'cd {self.root} && npm test'}}
        self.assertIn('qa-plan', self.deny_reason(held))

    def test_deny_names_the_missing_files_then_the_way_to_approve(self):
        source = {'tool_name': 'Write', 'tool_input': {'file_path': 'src/app.ts'}}
        reason = self.deny_reason(source)
        self.assertIn('docs/stories/todo/example/qa-plan.md', reason)
        self.assertIn('Write it now', reason)
        self.ready()
        self.wf('ensure')
        self.assertIn('`ramify approve qa-plan`', self.deny_reason(source))

    def test_ramify_and_harmless_redirects_pass_the_gate(self):
        for command in ['ramify gates 2>&1 | tail -5', 'ramify step >/dev/null 2>&1',
                        f'cd {self.root} && /opt/homebrew/bin/bash /x/bin/ramify step ask qa-plan --file docs/a.md']:
            with self.subTest(command=command):
                self.assertEqual('', self.wf('guard', payload={'tool_name': 'Bash', 'tool_input': {'command': command}}).stdout)
        self.assertNotEqual('', self.wf('guard', payload={'tool_name': 'Bash', 'tool_input': {'command': 'echo x > src/a.ts'}}).stdout)

    def test_the_human_approves_a_gate_nobody_asked_for(self):
        self.assertIsNone(json.loads(self.wf('asked', '--json').stdout))
        self.wf('approve', 'qa-plan', '--note', 'OK')
        self.assertIn('qa-plan', self.story.read_text())
        self.assertNotIn('ask:', self.story.read_text())
        self.assertEqual('', self.wf('guard', payload={'tool_name': 'Write', 'tool_input': {'file_path': 'src/app.ts'}}).stdout)

    def test_broken_definitions_warn_and_hold_nothing(self):
        (self.root / '.ramify/workflows.yml').write_text('steps: [unclosed\n')
        result = self.wf('guard', payload={'tool_name': 'Write', 'tool_input': {'file_path': 'src/app.ts'}})
        self.assertEqual('', result.stdout)
        self.assertIn('cannot verify', result.stderr)

    def test_status_says_what_opens_the_gate(self):
        self.assertIn('write docs/stories/todo/example/qa-plan.md, then Ramify asks', self.wf('status').stdout)
        self.ready()
        self.wf('ensure')
        self.assertIn('waits on `ramify approve qa-plan`', self.wf('status').stdout)


if __name__ == '__main__':
    unittest.main()
