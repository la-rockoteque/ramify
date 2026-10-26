#!/usr/bin/env python3
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
import json
import os
from pathlib import Path
import sys
import subprocess
import tempfile
import threading
import time
import unittest
from unittest.mock import patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1] / 'libexec'))
import tickets
import jira


class Lifecycle(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = self.tmp.name
        self.state = Path(self.root) / 'state.json'
        self.env = patch.dict(os.environ, {
            'RAMIFY_JIRA_URL': 'https://test.atlassian.net',
            'RAMIFY_JIRA_EMAIL': 'me@test', 'RAMIFY_JIRA_TOKEN': 'secret',
            'RAMIFY_JIRA_PROGRESS': 'In Progress', 'RAMIFY_JIRA_REVIEW': 'In Review',
        }, clear=True)
        self.env.start()
        self.addCleanup(self.env.stop)
        self.name = 'To Do'
        self.assignee = None
        self.writes = []
        self.remote = None
        self.fail = None
        self.addCleanup(patch.stopall)
        patch.object(tickets, 'branch_of', return_value='feat/ABC-12-login').start()
        patch.object(tickets, 'request_state', side_effect=lambda *a: self.remote).start()
        patch.object(jira, 'api', side_effect=self.api).start()

    def api(self, site, auth, method, path, body=None):
        if self.fail:
            raise jira.Fail(self.fail)
        if path == '/rest/api/3/myself':
            return {'accountId': 'my-account'}
        if method == 'PUT':
            self.writes.append(('assign', body))
            self.assignee = body['accountId']
            return None
        if method == 'POST':
            self.writes.append(('transition', body))
            self.name = body['transition']['id']
            return None
        if path.endswith('/transitions'):
            return {'transitions': [
                {'id': n, 'name': n, 'to': {'name': n, 'statusCategory': {
                    'key': 'done' if n == 'Done' else 'indeterminate'}}}
                for n in ('In Progress', 'In Review', 'Done')]}
        return {'fields': {'status': {'name': self.name, 'statusCategory': {
            'key': 'done' if self.name == 'Done' else 'indeterminate'}},
            'assignee': {'accountId': self.assignee} if self.assignee else None}}

    def sync(self, start=False):
        return tickets.sync(self.root, self.state, start=start)

    def test_start_assigns_current_jira_user_and_moves_to_progress(self):
        self.sync(start=True)
        self.assertEqual(self.assignee, 'my-account')
        self.assertEqual(self.name, 'In Progress')

    def test_open_request_enters_review_for_each_host(self):
        for host in ('github', 'gitlab', 'bitbucket'):
            self.remote = 'open'
            self.sync()
            self.assertEqual(self.name, 'In Review', host)

    def test_merged_request_enters_done_for_each_host(self):
        self.remote = 'merged'
        self.sync()
        self.assertEqual(self.name, 'Done')

    def test_watcher_detects_browser_changes_and_resumes_after_downtime(self):
        self.sync(start=True)
        self.remote = 'open'
        self.sync()
        self.assertEqual(self.name, 'In Review')
        self.remote = 'merged'
        self.sync()
        self.assertEqual(self.name, 'Done')

    def test_transition_mapping_rejects_missing_and_ambiguous_targets(self):
        os.environ['RAMIFY_JIRA_REVIEW'] = 'Missing'
        self.remote = 'open'
        with self.assertRaises(jira.Fail): self.sync()
        os.environ['RAMIFY_JIRA_REVIEW'] = ''
        with self.assertRaises(jira.Fail): self.sync()
        os.environ['RAMIFY_JIRA_REVIEW'] = 'In Review'
        original = self.api
        def ambiguous(*args):
            result = original(*args)
            if args[2] == 'GET' and args[3].endswith('/transitions'):
                result['transitions'].append(dict(result['transitions'][1], id='other'))
            return result
        with patch.object(jira, 'api', side_effect=ambiguous):
            with self.assertRaises(jira.Fail): self.sync()
        self.assertEqual(self.writes, [])

    def test_unconfigured_or_unlinked_branch_performs_no_writes(self):
        os.environ['RAMIFY_JIRA_URL'] = ''
        self.sync(start=True)
        os.environ['RAMIFY_JIRA_URL'] = 'https://test.atlassian.net'
        with patch.object(tickets, 'branch_of', return_value='feat/login'):
            self.sync(start=True)
        self.assertEqual(self.writes, [])

    def test_failed_sync_preserves_worktree_and_retries(self):
        for failure in ('401 credentials', 'timeout', '429 rate limit', 'transition unavailable'):
            self.fail = failure
            with self.assertRaises(jira.Fail): self.sync(start=True)
            self.assertTrue(Path(self.root).is_dir())
        self.fail = None
        self.sync()
        self.assertEqual(self.assignee, 'my-account')
        self.assertEqual(self.name, 'In Progress')

    def test_repeated_sync_does_not_repeat_assignment_or_transition(self):
        self.sync(start=True)
        self.sync(start=True)
        self.assertEqual(len(self.writes), 2)
        self.remote = 'open'
        self.sync()
        self.sync()
        self.assertEqual(len(self.writes), 3)
        self.state.unlink()
        self.name = 'To Do'
        self.sync(start=True)
        self.assertEqual(sum(w[0] == 'assign' for w in self.writes), 1)

    def test_partial_start_retries_without_reassigning(self):
        original = self.api
        def fail_transition(*args):
            if args[2] == 'POST':
                raise jira.Fail('transition unavailable')
            return original(*args)
        with patch.object(jira, 'api', side_effect=fail_transition):
            with self.assertRaises(jira.Fail): self.sync(start=True)
        self.sync()
        self.assertEqual(self.writes[0][0], 'assign')
        self.assertEqual(sum(w[0] == 'assign' for w in self.writes), 1)
        self.assertEqual(self.name, 'In Progress')

    def test_pending_start_does_not_block_a_later_merge(self):
        self.fail = 'timeout'
        with self.assertRaises(jira.Fail): self.sync(start=True)
        self.fail = None
        self.name, self.remote = 'In Review', 'merged'
        original = self.api
        def forward_only(*args):
            result = original(*args)
            if args[2] == 'GET' and args[3].endswith('/transitions'):
                result['transitions'] = [result['transitions'][2]]
            return result
        with patch.object(jira, 'api', side_effect=forward_only):
            self.sync()
        self.assertEqual(self.name, 'Done')

    def test_host_failure_does_not_block_initial_assignment(self):
        with patch.object(tickets, 'request_state', side_effect=jira.Fail('host offline')):
            with self.assertRaises(jira.Fail): self.sync(start=True)
        self.assertEqual(self.name, 'In Progress')
        self.assertEqual(self.assignee, 'my-account')

    def test_transition_name_cache_handles_one_way_workflows(self):
        self.remote = 'open'
        os.environ['RAMIFY_JIRA_REVIEW'] = 'Submit for review'
        original = self.api
        def workflow(*args):
            result = original(*args)
            if args[2] == 'GET' and args[3].endswith('/transitions'):
                if self.name == 'In Review':
                    result['transitions'] = []
                else:
                    result['transitions'][1]['name'] = 'Submit for review'
            return result
        with patch.object(jira, 'api', side_effect=workflow):
            self.sync()
            self.sync()
        self.assertEqual(len(self.writes), 1)

    def test_declined_or_closed_unmerged_request_never_enters_done(self):
        self.remote = 'closed'
        self.sync()
        self.assertEqual(self.writes, [])

    def test_stale_request_never_reopens_done_story(self):
        self.name = 'Done'
        self.remote = 'open'
        self.sync(start=True)
        self.assertEqual(self.writes, [])


class Hosts(unittest.TestCase):
    def test_gitlab_uses_authenticated_api_without_exporting_a_token(self):
        row = {'state': 'merged', 'source_branch': 'ABC-1', 'source_project_id': 1, 'target_project_id': 1}
        with patch.dict(os.environ, {}, clear=True), \
             patch.object(tickets, 'git', return_value='https://gitlab.com/owner/repo.git'), \
             patch.object(tickets.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, json.dumps(row) + '\n', '')) as run:
            self.assertEqual(tickets.request_state('/unused', 'ABC-1'), 'merged')
            command = run.call_args.args[0]
            self.assertEqual(command[:2], ['glab', 'api'])
            self.assertIn('--paginate', command)
            self.assertIn('ndjson', command)

    def test_pagination_rejects_cross_origin_links(self):
        with patch.object(tickets, 'http_json', return_value=(
            {'values': [], 'next': 'https://elsewhere.example/steal'}, {})) as http:
            with self.assertRaises(jira.Fail):
                list(tickets.pages('https://api.bitbucket.org/x', {'Authorization': 'secret'}, 'bitbucket'))
            self.assertEqual(http.call_count, 1)

    def test_host_lookup_matches_repository_and_source_branch(self):
        cases = {
            'github': ('git@github.com:owner/repo.git', [
                {'state': 'open', 'head': {'ref': 'ABC-1', 'repo': {'full_name': 'owner/repo'}}},
                {'state': 'closed', 'merged_at': 'today', 'head': {'ref': 'ABC-1', 'repo': {'full_name': 'owner/repo'}}}]),
            'gitlab': ('https://gitlab.com/group/sub/repo.git', [
                {'state': 'opened', 'source_branch': 'ABC-1', 'source_project_id': 4, 'target_project_id': 4},
                {'state': 'merged', 'source_branch': 'ABC-1', 'source_project_id': 4, 'target_project_id': 4}]),
            'bitbucket': ('ssh://git@bitbucket.org/owner/repo.git', [
                {'state': 'OPEN', 'source': {'branch': {'name': 'ABC-1'}, 'repository': {'full_name': 'owner/repo'}}},
                {'state': 'MERGED', 'source': {'branch': {'name': 'ABC-1'}, 'repository': {'full_name': 'owner/repo'}}}]),
        }
        for host, (remote, rows) in cases.items():
            with self.subTest(host=host), patch.dict(os.environ, {'RAMIFY_GIT_TOKEN': 'secret'}, clear=True), \
                 patch.object(tickets, 'git', return_value=remote), \
                 patch.object(tickets, 'pages', return_value=rows) as pages:
                self.assertEqual(tickets.request_state('/unused', 'ABC-1'), 'open')
                self.assertEqual(tickets.request_state('/unused', 'unrelated'), None)
                pages.return_value = rows[1:]
                self.assertEqual(tickets.request_state('/unused', 'ABC-1'), 'merged')

    def test_pagination(self):
        with patch.object(tickets, 'http_json', side_effect=[([1], {'Link': '<https://api.github.com/x?page=2>; rel="next"'}), ([2], {})]):
            self.assertEqual(list(tickets.pages('https://api.github.com/x', {}, 'github')), [1, 2])
        with patch.object(tickets, 'http_json', side_effect=[({'values': [1], 'next': 'https://api.bitbucket.org/x?page=2'}, {}), ({'values': [2]}, {})]):
            self.assertEqual(list(tickets.pages('https://api.bitbucket.org/x', {}, 'bitbucket')), [1, 2])
        with patch.object(tickets, 'http_json', side_effect=[([1], {'X-Next-Page': '2'}), ([2], {})]):
            self.assertEqual(list(tickets.pages('https://gitlab.com/x?page=1', {}, 'gitlab')), [1, 2])

    def test_watcher_has_single_instance_and_stops_for_removed_worktree(self):
        with tempfile.TemporaryDirectory() as folder:
            state = Path(folder) / 'state'
            with tickets.lock(state, 'watch') as first:
                self.assertTrue(first)
                with tickets.lock(state, 'watch') as second:
                    self.assertFalse(second)
            with patch.object(tickets, 'branch_of', return_value='?'):
                self.assertEqual(tickets.watch('/missing', state, '/unused'), 0)

    def test_host_requests_over_http(self):
        observed = []
        class Handler(BaseHTTPRequestHandler):
            def log_message(self, *args): pass
            def do_GET(self):
                observed.append((self.path, self.headers.get('Authorization')))
                if '/repos/' in self.path:
                    body = [{'state': 'closed', 'merged_at': 'today', 'head': {
                        'ref': 'ABC-1', 'repo': {'full_name': 'owner/repo'}}}]
                elif '/projects/' in self.path:
                    body = [{'state': 'merged', 'source_branch': 'ABC-1',
                             'source_project_id': 1, 'target_project_id': 1}]
                else:
                    body = {'values': [{'state': 'MERGED', 'source': {
                        'branch': {'name': 'ABC-1'}, 'repository': {'full_name': 'owner/repo'}}}]}
                self.send_response(200)
                self.end_headers()
                self.wfile.write(json.dumps(body).encode())
        server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
        thread = threading.Thread(target=server.serve_forever, daemon=True)
        thread.start()
        try:
            for host in ('github', 'gitlab', 'bitbucket'):
                with patch.dict(os.environ, {'RAMIFY_GIT_HOST': host, 'RAMIFY_GIT_TOKEN': 'test-token',
                    'RAMIFY_GIT_API_URL': f'http://127.0.0.1:{server.server_port}'}, clear=True), \
                    patch.object(tickets, 'git', return_value='https://example.com/owner/repo.git'):
                    self.assertEqual(tickets.request_state('/unused', 'ABC-1'), 'merged')
            self.assertEqual(len(observed), 3)
            self.assertTrue(all(auth == 'Bearer test-token' for _, auth in observed))
        finally:
            server.shutdown()
            server.server_close()


class EndToEnd(unittest.TestCase):
    def test_new_and_watcher_update_jira_without_manual_sync(self):
        with tempfile.TemporaryDirectory() as folder:
            root = Path(folder)
            state = {'status': 'To Do', 'assignee': None, 'remote': None}
            writes = []
            class Handler(BaseHTTPRequestHandler):
                def log_message(self, *args): pass
                def reply(self, body=None):
                    self.send_response(200 if body is not None else 204)
                    self.end_headers()
                    if body is not None: self.wfile.write(json.dumps(body).encode())
                def do_GET(self):
                    if self.path == '/rest/api/3/myself':
                        return self.reply({'accountId': 'me'})
                    if self.path.startswith('/repos/'):
                        return self.reply([] if not state['remote'] else [{
                            'state': 'open' if state['remote'] == 'open' else 'closed',
                            'merged_at': 'today' if state['remote'] == 'merged' else None,
                            'head': {'ref': 'ABC-1-task', 'repo': {'full_name': str(origin)[1:-4]}}}])
                    if self.path.endswith('/transitions'):
                        return self.reply({'transitions': [{'id': n, 'name': n, 'to': {'name': n,
                            'statusCategory': {'key': 'done' if n == 'Done' else 'indeterminate'}}}
                            for n in ('In Progress', 'In Review', 'Done')]})
                    self.reply({'fields': {'status': {'name': state['status'], 'statusCategory': {
                        'key': 'done' if state['status'] == 'Done' else 'indeterminate'}},
                        'assignee': {'accountId': state['assignee']} if state['assignee'] else None}})
                def do_PUT(self):
                    body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                    state['assignee'] = body['accountId']
                    writes.append('assign')
                    self.reply()
                def do_POST(self):
                    body = json.loads(self.rfile.read(int(self.headers['Content-Length'])))
                    state['status'] = body['transition']['id']
                    writes.append(state['status'])
                    self.reply()
            server = ThreadingHTTPServer(('127.0.0.1', 0), Handler)
            threading.Thread(target=server.serve_forever, daemon=True).start()
            origin, repo = root / 'origin.git', root / 'repo'
            cli = str(Path(__file__).resolve().parents[1] / 'bin/ramify')
            env = {**os.environ, 'RAMIFY_STATE_DIR': str(root / 'state'), 'RAMIFY_REGISTRY': str(root / 'registry'),
                   'RAMIFY_JIRA_EMAIL': 'qa@example', 'RAMIFY_JIRA_TOKEN': 'token', 'RAMIFY_GIT_TOKEN': 'token',
                   'RAMIFY_POPUP': '0', 'RAMIFY_ROOT': str(repo)}
            def run(*args, cwd=root):
                result = subprocess.run(args, cwd=cwd, env=env, capture_output=True, text=True, timeout=20)
                self.assertEqual(result.returncode, 0, result.stdout + result.stderr)
                return result
            def wait_for(status):
                deadline = time.monotonic() + 15
                while state['status'] != status and time.monotonic() < deadline:
                    time.sleep(.1)
                self.assertEqual(state['status'], status)
            try:
                run('git', 'init', '--bare', '-b', 'main', str(origin))
                run('git', 'clone', origin.as_uri(), str(repo))
                run('git', 'config', 'user.email', 'qa@example', cwd=repo)
                run('git', 'config', 'user.name', 'QA', cwd=repo)
                (repo / '.ramify.conf').write_text(
                    f'RAMIFY_WORKTREES={root}/trees\nRAMIFY_AUTOSTART=0\nRAMIFY_TICKET_INTERVAL=5\n'
                    f'RAMIFY_JIRA_URL=http://127.0.0.1:{server.server_port}\n'
                    f'RAMIFY_JIRA_REVIEW="In Review"\nRAMIFY_GIT_HOST=github\n'
                    f'RAMIFY_GIT_API_URL=http://127.0.0.1:{server.server_port}\n')
                run('git', 'add', '.', cwd=repo)
                run('git', 'commit', '-m', 'fixture', cwd=repo)
                run('git', 'push', 'origin', 'main', cwd=repo)
                run(cli, 'new', 'ABC-1-task', cwd=repo)
                wait_for('In Progress')
                self.assertEqual(state['assignee'], 'me')
                state['remote'] = 'open'
                wait_for('In Review')
                state['remote'] = 'merged'
                wait_for('Done')
                self.assertEqual(writes, ['assign', 'In Progress', 'In Review', 'Done'])
                env['RAMIFY_ROOT'] = str(root / 'trees/ABC-1-task')
                run(cli, 'ticket', 'sync')
                self.assertEqual(len(writes), 4)
            finally:
                server.shutdown()
                server.server_close()


if __name__ == '__main__':
    unittest.main()
