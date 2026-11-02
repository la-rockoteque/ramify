#!/usr/bin/env python3
"""Synchronize a worktree's Jira story with its Git host request."""
import base64
from contextlib import contextmanager
import fcntl
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request

import jira


def git(root, *args):
    try:
        return subprocess.run(['git', '-C', root, *args], capture_output=True,
                              text=True, check=True, timeout=15).stdout.strip()
    except (OSError, subprocess.SubprocessError):
        return ''


def branch_of(root):
    return git(root, 'symbolic-ref', '--quiet', '--short', 'HEAD') or '?'


@contextmanager
def lock(state, kind):
    path = Path(str(state) + '.' + kind + '.lock')
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open('a') as handle:
        try:
            fcntl.flock(handle, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError:
            yield False
            return
        try:
            yield True
        finally:
            fcntl.flock(handle, fcntl.LOCK_UN)


class NoRedirect(urllib.request.HTTPRedirectHandler):
    def redirect_request(self, req, fp, code, msg, headers, newurl):
        return None


def http_json(url, headers):
    try:
        req = urllib.request.Request(url, headers=headers)
        with urllib.request.build_opener(NoRedirect).open(req, timeout=15) as response:
            return json.load(response), response.headers
    except urllib.error.HTTPError as exc:
        raise jira.Fail(f"Git host returned HTTP {exc.code}; check host credentials and repository access")
    except (urllib.error.URLError, TimeoutError, ValueError) as exc:
        raise jira.Fail(f"cannot read Git host request state ({type(exc).__name__})")


def pages(url, headers, host):
    origin = urllib.parse.urlparse(url)
    seen = set()
    while url:
        parsed = urllib.parse.urlparse(url)
        if (parsed.scheme, parsed.netloc) != (origin.scheme, origin.netloc) or url in seen:
            raise jira.Fail('Git host returned an unsafe or repeated pagination URL')
        seen.add(url)
        data, response = http_json(url, headers)
        yield from data.get('values', []) if host == 'bitbucket' else data
        next_url = None
        if host == 'bitbucket':
            next_url = data.get('next')
        elif host == 'github':
            match = re.search(r'<([^>]+)>;\s*rel="next"', response.get('Link', ''))
            next_url = match.group(1) if match else None
        elif response.get('X-Next-Page'):
            query = urllib.parse.parse_qs(parsed.query)
            query['page'] = [response['X-Next-Page']]
            next_url = urllib.parse.urlunparse(parsed._replace(query=urllib.parse.urlencode(query, doseq=True)))
        url = urllib.parse.urljoin(url, next_url) if next_url else None


def remote_info(root):
    remote = git(root, 'remote', 'get-url', os.environ.get('RAMIFY_REMOTE', 'origin'))
    if '://' not in remote:
        match = re.fullmatch(r'(?:[^@/:]+@)?([^/:]+):(.+)', remote)
        if not match:
            raise jira.Fail('cannot identify Git host from the configured remote')
        remote = 'ssh://' + match[1] + '/' + match[2]
    parsed = urllib.parse.urlparse(remote)
    host = parsed.hostname or ''
    provider = os.environ.get('RAMIFY_GIT_HOST') or {
        'github.com': 'github', 'gitlab.com': 'gitlab', 'bitbucket.org': 'bitbucket'}.get(host)
    if provider not in ('github', 'gitlab', 'bitbucket'):
        raise jira.Fail('set RAMIFY_GIT_HOST=github|gitlab|bitbucket for this remote')
    repo = parsed.path.strip('/').removesuffix('.git')
    if not repo or '/' not in repo:
        raise jira.Fail('the Git remote needs an owner and repository')
    return provider, host, repo


def host_token(provider, hostname):
    token = os.environ.get('RAMIFY_GIT_TOKEN') or os.environ.get({
        'github': 'GH_TOKEN', 'gitlab': 'GITLAB_TOKEN', 'bitbucket': 'RAMIFY_BITBUCKET_TOKEN'}[provider])
    if token:
        return token
    if provider == 'gitlab':
        return None
    if provider == 'github':
        try:
            result = subprocess.run(['gh', 'auth', 'token', '--hostname', hostname],
                                    capture_output=True, text=True, timeout=15, check=True)
            if result.stdout.strip():
                return result.stdout.strip()
        except (OSError, subprocess.SubprocessError):
            pass
    raise jira.Fail(f'{provider}: set RAMIFY_GIT_TOKEN or sign in with the host CLI; Bitbucket uses RAMIFY_BITBUCKET_TOKEN')


def gitlab_rows(root, hostname, endpoint):
    try:
        result = subprocess.run(['glab', 'api', endpoint, '--hostname', hostname,
                                 '--paginate', '--output', 'ndjson'], cwd=root,
                                capture_output=True, text=True, timeout=90, check=True)
        return [json.loads(line) for line in result.stdout.splitlines() if line.strip()]
    except (OSError, subprocess.SubprocessError, ValueError):
        raise jira.Fail('GitLab request lookup failed; run glab auth login or set RAMIFY_GIT_TOKEN')


def request_state(root, branch):
    provider, hostname, repo = remote_info(root)
    token = host_token(provider, hostname)
    headers = {'Accept': 'application/json'}
    if token:
        headers['Authorization'] = 'Bearer ' + token
    base = os.environ.get('RAMIFY_GIT_API_URL', '').rstrip('/')
    if not base:
        base = {'github': 'https://api.github.com' if hostname == 'github.com' else f'https://{hostname}/api/v3',
                'gitlab': f'https://{hostname}/api/v4', 'bitbucket': 'https://api.bitbucket.org/2.0'}[provider]
    jira.validate_site(base)
    quote = lambda value: urllib.parse.quote(value, safe='')
    main = os.environ.get('RAMIFY_MAIN', 'main')
    if provider == 'github':
        query = urllib.parse.urlencode({'state': 'all', 'head': repo.split('/')[0] + ':' + branch,
                                        'base': main, 'sort': 'created', 'direction': 'desc', 'per_page': 100})
        url = f'{base}/repos/{repo}/pulls?{query}'
    elif provider == 'gitlab':
        if token:
            headers['PRIVATE-TOKEN'] = token
        query = urllib.parse.urlencode({'scope': 'all', 'state': 'all', 'source_branch': branch,
                                        'target_branch': main, 'order_by': 'created_at', 'sort': 'desc', 'per_page': 100})
        url = f'{base}/projects/{quote(repo)}/merge_requests?{query}'
    else:
        email = os.environ.get('RAMIFY_BITBUCKET_EMAIL')
        if email:
            headers['Authorization'] = 'Basic ' + base64.b64encode(f'{email}:{token}'.encode()).decode()
        query = urllib.parse.urlencode({'state': ['OPEN', 'MERGED', 'DECLINED', 'SUPERSEDED'],
                                        'q': f'source.branch.name = {json.dumps(branch)} AND destination.branch.name = {json.dumps(main)}',
                                        'sort': '-created_on', 'pagelen': 100}, doseq=True)
        url = f'{base}/repositories/{repo}/pullrequests?{query}'
    if provider == 'gitlab' and not token:
        if os.environ.get('RAMIFY_GIT_API_URL'):
            raise jira.Fail('set RAMIFY_GIT_TOKEN when RAMIFY_GIT_API_URL overrides the GitLab API')
        rows = gitlab_rows(root, hostname, url.split('/api/v4/', 1)[1])
    else:
        rows = pages(url, headers, provider)
    for row in rows:
        if provider == 'github':
            head = row.get('head') or {}
            if head.get('ref') != branch or (head.get('repo') or {}).get('full_name', '').casefold() != repo.casefold():
                continue
            if row.get('base', {}).get('ref', main) != main:
                continue
            return 'merged' if row.get('merged_at') else ('open' if row.get('state') == 'open' else 'closed')
        if provider == 'gitlab':
            if row.get('source_branch') != branch or row.get('source_project_id') != row.get('target_project_id'):
                continue
            if row.get('target_branch', main) != main:
                continue
            return {'opened': 'open', 'merged': 'merged'}.get(row.get('state'), 'closed')
        source = row.get('source') or {}
        if source.get('branch', {}).get('name') != branch or source.get('repository', {}).get('full_name', '').casefold() != repo.casefold():
            continue
        if row.get('destination', {}).get('branch', {}).get('name', main) != main:
            continue
        return {'OPEN': 'open', 'MERGED': 'merged'}.get(row.get('state'), 'closed')
    return None


def save(path, state):
    tmp = path.with_suffix('.tmp')
    tmp.write_text(json.dumps(state) + '\n')
    tmp.replace(path)


def sync(root, state_path, start=False):
    site = os.environ.get('RAMIFY_JIRA_URL', '')
    branch = branch_of(root)
    if not site or os.environ.get('RAMIFY_JIRA_SYNC', '1') == '0':
        return []
    pattern = os.environ.get('RAMIFY_TICKET_PATTERN', '[A-Za-z][A-Za-z0-9]*-[0-9]+')
    match = re.search('-(' + pattern + ')-', '-' + branch.replace('/', '-') + '-')
    if not match:
        return []
    jira.validate_site(site)
    key = match[1].upper()
    state_path = Path(state_path)
    with lock(state_path, 'sync') as acquired:
        if not acquired:
            return []
        try:
            state = json.loads(state_path.read_text())
        except (FileNotFoundError, ValueError):
            state = {}
        identity = [site, branch, key]
        if state.get('identity') != identity:
            state = {'identity': identity}
        if start and not state.get('started'):
            state['pending_start'] = True
            save(state_path, state)
        remote_error = None
        try:
            remote = request_state(root, branch)
        except jira.Fail as exc:
            remote, remote_error = None, exc
        email, token = jira.credentials(site)
        auth = base64.b64encode(f'{email}:{token}'.encode()).decode()
        messages = []
        targets = state.setdefault('targets', {})
        if state.get('pending_start'):
            messages.append(jira.start(site, auth, key, targets, progress=remote not in ('open', 'merged')))
            state.update(started=True, pending_start=False)
            save(state_path, state)
        if remote_error:
            raise remote_error
        if remote in ('open', 'merged'):
            messages.append(jira.transition(site, auth, key, 'review' if remote == 'open' else 'done', targets))
            save(state_path, state)
        return messages


def watch(root, state, cli):
    with lock(state, 'watch') as acquired:
        if not acquired:
            return 0
        initial = branch_of(root)
        if initial == '?':
            return 0
        interval = max(5, int(os.environ.get('RAMIFY_TICKET_INTERVAL', '60')))
        while Path(root).is_dir() and branch_of(root) == initial:
            try:
                result = subprocess.run([cli, 'ticket', 'sync'], cwd=root,
                                        env={**os.environ, 'RAMIFY_ROOT': root}, timeout=120)
                if result.returncode == 3:
                    break
            except subprocess.TimeoutExpired:
                print('ticket sync timed out; retrying on the next poll', file=sys.stderr, flush=True)
            time.sleep(interval)
    return 0


def main(argv):
    if len(argv) < 3 or argv[0] not in ('start', 'sync', 'watch'):
        raise jira.Fail('usage: tickets.py start|sync|watch ROOT STATE [CLI]')
    command, root, state = argv[:3]
    if command == 'watch':
        return watch(root, state, argv[3])
    if os.environ.get('RAMIFY_JIRA_SYNC', '1') == '0' or not os.environ.get('RAMIFY_JIRA_URL'):
        return 3
    for message in sync(root, state, start=command == 'start'):
        print(message)
    return 0


if __name__ == '__main__':
    try:
        sys.exit(main(sys.argv[1:]))
    except (jira.Fail, OSError, ValueError, KeyError, TypeError) as exc:
        print(f'ticket: {exc}; retry with ramify ticket sync (watcher retries automatically)', file=sys.stderr)
        sys.exit(1)
