#!/usr/bin/env python3
import json
import os
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

HOME = Path(__file__).resolve().parents[1]
CLI = str(HOME / 'bin/ramify')
HOOK = str(HOME / 'hooks/ramify-hook')


class RepositoryWorkflows(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name).resolve()
        self.repo = self.root / 'repo'
        self.repo.mkdir()
        self.env = {**os.environ, 'PATH': str(Path(sys.executable).parent) + ':' + os.environ['PATH'],
                    'RAMIFY_STATE_DIR': str(self.root / 'state'), 'RAMIFY_REGISTRY': str(self.root / 'registry'),
                    'RAMIFY_AUTOSTART': '0', 'RAMIFY_POPUP': '0', 'RAMIFY_ROOT': str(self.repo)}
        self.git('init', '-q', '-b', 'main')
        self.git('config', 'user.email', 'qa@example')
        self.git('config', 'user.name', 'QA')
        self.git('config', 'commit.gpgsign', 'false')
        self.write('README.md', '# Fixture\n')
        self.git('add', '.')
        self.git('commit', '-qm', 'fixture')

    def run_cmd(self, *argv, ok=True, root=None):
        env = {**self.env, 'RAMIFY_ROOT': str(root or self.repo)}
        result = subprocess.run(argv, cwd=root or self.repo, env=env, capture_output=True, text=True, timeout=20)
        if ok: self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
        return result

    def git(self, *args): return self.run_cmd('git', *args).stdout
    def cli(self, *args, **kwargs): return self.run_cmd(CLI, *args, **kwargs)
    def write(self, path, text):
        target = self.repo / path
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_text(text)

    def pipelines(self):
        for track in ('feature', 'bug-fix', 'tweak', 'spike', 'release'):
            self.write(f'docs/{track}-pipeline.md', f'# {track} pipeline\nDevelopment: qa-plan → build → review → pull request.\n')

    def setup_repo(self):
        self.pipelines()
        self.cli('setup')

    def test_setup_detects_moship_pipeline_documents(self):
        self.setup_repo()
        config = (self.repo / '.ramify.conf').read_text()
        self.assertIn('RAMIFY_WORKFLOW_SOURCE=repo', config)
        for track in ('feature', 'bug-fix', 'tweak', 'spike', 'release'):
            self.assertIn(f'docs/{track}-pipeline.md', config)

    def test_setup_print_reports_sources_without_writes(self):
        self.pipelines()
        before = self.git('status', '--porcelain')
        output = self.cli('setup', '--print').stdout
        self.assertIn('RAMIFY_WORKFLOW_SOURCE=repo', output)
        self.assertIn('pipeline', output)
        self.assertFalse((self.repo / '.ramify.conf').exists())
        self.assertEqual(before, self.git('status', '--porcelain'))

    def test_setup_print_with_existing_config_creates_no_runtime_state(self):
        self.write('.ramify.conf', "RAMIFY_SERVICES=''\n")
        self.cli('setup', '--print')
        self.assertFalse((self.root / 'state').exists())
        self.assertFalse((self.root / 'registry').exists())

    def test_setup_follows_explicit_development_guide_links(self):
        self.write('AGENTS.md', 'Follow [the project workflow](docs/process.md).\n')
        self.write('docs/process.md', '# Development workflow\nBuild → review → pull request\n')
        self.cli('setup')
        self.assertIn('docs/process.md', self.cli('workflow').stdout)

    def test_setup_from_worktree_reads_its_current_documents(self):
        tree = self.root / 'tree'
        self.git('worktree', 'add', '-qb', 'story/demo', str(tree))
        (tree / 'docs').mkdir()
        (tree / 'docs/feature-pipeline.md').write_text('# Feature pipeline\nDevelopment, review, and pull request steps.\n')
        self.assertFalse((self.repo / 'docs/feature-pipeline.md').exists())
        self.assertIn('RAMIFY_WORKFLOW_SOURCE=repo', self.cli('setup', '--print', root=tree).stdout)

    def test_repo_workflow_routes_cli_and_session_guidance(self):
        self.setup_repo()
        self.assertIn('docs/feature-pipeline.md', self.cli('workflow').stdout)
        self.assertIn('Repository workflow', self.cli('step').stdout)
        for agent in ('claude', 'codex'):
            result = subprocess.run([HOOK, 'session-start'], cwd=self.repo, env={**self.env, 'RAMIFY_AGENT': agent},
                input=json.dumps({'cwd': str(self.repo), 'session_id': 'fixture', 'source': 'resume'}),
                capture_output=True, text=True, timeout=20)
            self.assertEqual(result.returncode, 0, result.stderr)
            self.assertIn('docs/feature-pipeline.md', result.stdout)
            self.assertNotIn('ramify:spec', result.stdout)

    def test_new_uses_repo_workflow_without_duplicate_order(self):
        self.setup_repo()
        with (self.repo / '.ramify.conf').open('a') as f:
            f.write(f'\nRAMIFY_MAIN=main\nRAMIFY_WORKTREES={self.root}/trees\nRAMIFY_BRANCH_PREFIX=story/\n')
        origin = self.root / 'origin.git'
        self.run_cmd('git', 'init', '--bare', '-q', '-b', 'main', str(origin))
        self.git('remote', 'add', 'origin', str(origin))
        self.git('add', '.')
        self.git('commit', '-qm', 'workflow')
        self.git('push', '-qu', 'origin', 'main')
        output = self.cli('new', 'demo', '--workflow', 'feature').stdout
        tree = self.root / 'trees/demo'
        self.assertTrue(tree.is_dir())
        self.assertFalse((tree / '.ramify/work/demo/order.md').exists())
        self.assertIn('docs/feature-pipeline.md', output)
        self.assertEqual(self.run_cmd('git', 'branch', '--show-current', root=tree).stdout.strip(), 'story/demo')
        self.assertIn('docs/feature-pipeline.md', self.cli('new', 'second').stdout)
        self.assertFalse((self.root / 'trees/second/.ramify/work/second/order.md').exists())

    def test_repo_mode_preserves_native_records_and_gates(self):
        self.setup_repo()
        self.write('docs/stories/active/demo/story.md', '---\nsteps: [{step: build, on: 2026-10-26}]\n---\nNative story\n')
        story = self.repo / 'docs/stories/active/demo/story.md'
        before = story.read_bytes()
        self.cli('step', 'gate', '--pr')
        result = self.cli('step', 'done', 'build', ok=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('repository', result.stderr.lower())
        self.assertEqual(before, story.read_bytes())

    def test_explicit_workflow_source_overrides_detection(self):
        self.setup_repo()
        with (self.repo / '.ramify.conf').open('a') as f: f.write('\nRAMIFY_WORKFLOW_SOURCE=ramify\n')
        self.assertIn('standard', self.cli('workflow').stdout)
        self.write('.ramify/workflows.yml', 'workflows: off\n')
        self.assertIn('off', self.cli('workflow').stdout)

    def test_repo_without_workflow_keeps_defaults(self):
        self.cli('setup')
        self.assertIn('standard', self.cli('workflow').stdout)
        self.pipelines()
        self.assertIn('RAMIFY_WORKFLOW_SOURCE=repo', self.cli('setup', '--print').stdout)

    def test_domain_workflow_is_not_a_development_pipeline(self):
        self.write('docs/equipment-workflow.md', '# Equipment workflow\nThis capability is under development.\nReview requests and test equipment before shipping.\n')
        self.write('AGENTS.md', 'Read [equipment workflow](docs/equipment-workflow.md).\n')
        self.cli('setup')
        self.assertNotIn('RAMIFY_WORKFLOW_SOURCE=repo', (self.repo / '.ramify.conf').read_text())

    def test_ci_pipeline_is_not_a_documented_delivery_workflow(self):
        self.write('README.md', 'CI: [pipeline](bitbucket-pipelines.yml).\n')
        self.write('bitbucket-pipelines.yml', 'pipelines:\n  script: git commit && run tests && review\n')
        self.cli('setup')
        self.assertNotIn('RAMIFY_WORKFLOW_SOURCE=repo', (self.repo / '.ramify.conf').read_text())

    def test_conflicting_or_missing_sources_are_actionable(self):
        self.write('WORKFLOW.md', '# Development workflow\nCommit → review → pull request\n')
        self.write('docs/workflow.md', '# Development workflow\nQA plan → build → pull request\n')
        self.cli('setup')
        result = self.cli('workflow', ok=False)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn('select', result.stderr.lower())
        self.write('.ramify.conf', "RAMIFY_WORKFLOW_SOURCE=repo\nRAMIFY_WORKFLOW_DOCS='[\"docs/missing.md\"]'\n")
        result = self.cli('workflow', ok=False)
        self.assertIn('missing', result.stderr)

    def test_worktree_uses_its_own_workflow_documents(self):
        self.setup_repo()
        self.git('add', '.')
        self.git('commit', '-qm', 'workflow')
        tree = self.root / 'tree'
        self.git('worktree', 'add', '-qb', 'story/demo', str(tree))
        (tree / 'docs/feature-pipeline.md').unlink()
        result = self.cli('workflow', root=tree, ok=False)
        self.assertIn('missing', result.stderr)
        self.assertIn('missing', self.cli('setup', '--print', root=tree, ok=False).stderr)

    def test_existing_ramify_order_survives_repo_workflow_selection(self):
        self.git('checkout', '-qb', 'feat/demo')
        env = {**self.env, 'WF_ROOT': str(self.repo), 'WF_PRIMARY': str(self.repo),
               'WF_BRANCH': 'feat/demo', 'WF_HOME': str(HOME)}
        subprocess.run([sys.executable, str(HOME / 'libexec/workflow.py'), 'start', 'feature', 'demo'],
                       cwd=self.repo, env=env, check=True, capture_output=True)
        order = self.repo / '.ramify/work/demo/order.md'
        before = order.read_bytes()
        self.setup_repo()
        self.assertIn('→ spec', self.cli('step').stdout)
        self.assertEqual(before, order.read_bytes())
        with (self.repo / '.ramify.conf').open('a') as f:
            f.write(f'\nRAMIFY_MAIN=main\nRAMIFY_WORKTREES={self.root}/trees\nRAMIFY_BRANCH_PREFIX=story/\n')
        self.git('add', 'docs', '.ramify.conf')
        self.git('commit', '-qm', 'shared workflow')
        self.git('update-ref', 'refs/heads/main', 'HEAD')
        origin = self.root / 'origin.git'
        self.run_cmd('git', 'init', '--bare', '-q', '-b', 'main', str(origin))
        self.git('remote', 'add', 'origin', str(origin))
        self.git('push', '-qu', 'origin', 'main')
        self.cli('new', 'fresh', '--workflow', 'feature')
        self.assertEqual(self.run_cmd('git', 'branch', '--show-current', root=self.root / 'trees/fresh').stdout.strip(), 'story/fresh')
        self.assertEqual(before, order.read_bytes())


if __name__ == '__main__': unittest.main()
