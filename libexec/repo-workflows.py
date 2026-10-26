#!/usr/bin/env python3
"""Discover repository development guides and route agents to their native workflow."""
import json
import os
from pathlib import Path
import re
import shlex
import sys


class Fail(Exception):
    pass


def read(path):
    try:
        return path.read_text(encoding='utf-8')
    except (OSError, UnicodeError):
        return ''


def local_path(root, relative):
    path = (root / relative).resolve()
    try:
        path.relative_to(root.resolve())
    except ValueError:
        raise Fail(f'workflow guide must stay inside the repository: {relative}')
    return path


def development(text):
    heading = re.search(r'^#+ .*?(development workflow|delivery pipeline|engineering process|software workflow)', text, re.M | re.I)
    signals = (r'pull request|merge request|create-pr', r'qa[- ]plan|\btests?\b',
               r'\breview\b', r'\bcommits?\b', r'worktree|\bbranch\b')
    code_delivery = re.search(r'pull request|merge request|create-pr|worktree|\bgit\b', text, re.I)
    return bool(heading) or bool(code_delivery and sum(bool(re.search(signal, text, re.I)) for signal in signals) >= 2)


def links(text):
    return re.findall(r'\]\(([^)#]+)(?:#[^)]*)?\)|(?m:^@([^\s]+))', text)


def detect(root):
    root = Path(root).resolve()
    if (root / '.ramify/workflows.yml').is_file():
        return {'source': 'ramify', 'docs': [], 'reason': 'explicit .ramify/workflows.yml'}
    candidates = set()
    conventional = ['WORKFLOW.md', 'WORKFLOWS.md', 'docs/workflow.md', 'docs/development-workflow.md', 'docs/process/workflow.md']
    conventional += [f'docs/{track}-pipeline.md' for track in ('feature', 'bug-fix', 'tweak', 'spike', 'release')]
    for relative in conventional:
        if development(read(root / relative)):
            candidates.add(relative)
    pending = [(name, 0) for name in ('AGENTS.md', 'CLAUDE.md', 'CONTRIBUTING.md', 'CONTRIBUTING', 'README.md', 'README')]
    visited = set()
    while pending:
        relative, depth = pending.pop()
        if relative in visited or depth > 2:
            continue
        visited.add(relative)
        try:
            path = local_path(root, relative)
        except Fail:
            continue
        text = read(path)
        if relative in ('CONTRIBUTING.md', 'CONTRIBUTING') and re.search(r'^#+ .*workflow', text, re.M | re.I) and development(text):
            candidates.add(relative)
        for markdown, mention in links(text):
            link = markdown or mention
            if re.match(r'[a-z]+:|//', link, re.I):
                continue
            linked = str((Path(relative).parent / link))
            try:
                destination = local_path(root, linked)
            except Fail:
                continue
            if not destination.is_file():
                continue
            if destination.suffix.lower() not in ('', '.md', '.markdown', '.txt', '.rst'):
                continue
            name = destination.relative_to(root).as_posix()
            body = read(destination)
            guide = re.search(r'workflow|pipeline', destination.stem, re.I) or re.search(
                r'^#+ .*?(development workflow|delivery pipeline|engineering process)', body, re.M | re.I)
            if guide and development(body):
                candidates.add(name)
            elif destination.name.lower() in ('agents.md', 'claude.md', 'conventions.md', 'contributing.md', 'docs-structure.md'):
                pending.append((name, depth + 1))
    docs = sorted(candidates)
    if not docs:
        return {'source': 'ramify', 'docs': [], 'reason': 'no repository development workflow detected'}
    family = all(Path(doc).name in {f'{track}-pipeline.md' for track in ('feature', 'bug-fix', 'tweak', 'spike', 'release')} for doc in docs)
    # Distinct standalone guides need an explicit choice; a named pipeline family is one workflow.
    source = 'repo' if family or len({read(root / doc).strip() for doc in docs}) == 1 else 'select'
    return {'source': source, 'docs': docs, 'reason': 'repository pipeline documents' if source == 'repo' else 'select the authoritative workflow guides'}


def configured(root):
    source = os.environ.get('RAMIFY_WORKFLOW_SOURCE', 'auto')
    raw = os.environ.get('RAMIFY_WORKFLOW_DOCS', '')
    if source == 'ramify':
        return {'source': source, 'docs': [], 'reason': 'explicit Ramify workflow selection'}
    if source not in ('auto', 'repo', 'select'):
        raise Fail('RAMIFY_WORKFLOW_SOURCE must be auto, repo, or ramify')
    if source in ('repo', 'select') and raw:
        try:
            docs = json.loads(raw)
        except ValueError:
            raise Fail('RAMIFY_WORKFLOW_DOCS must be a JSON array of relative guide paths')
        if not isinstance(docs, list) or not docs or not all(isinstance(doc, str) for doc in docs):
            raise Fail('RAMIFY_WORKFLOW_DOCS must be a non-empty JSON array of relative guide paths')
        return {'source': source, 'docs': docs, 'reason': 'configured repository workflow'}
    result = detect(root)
    if source == 'repo' and result['source'] != 'repo':
        raise Fail('select repository guides with RAMIFY_WORKFLOW_DOCS before using repository mode')
    return result


def guides(root):
    result = configured(root)
    if result['source'] == 'select':
        raise Fail('select the authoritative guides with RAMIFY_WORKFLOW_SOURCE=repo and RAMIFY_WORKFLOW_DOCS: ' + ', '.join(result['docs']))
    if not result['docs']:
        raise Fail('repository workflow has no configured guides')
    for relative in result['docs']:
        if Path(relative).is_absolute():
            raise Fail('workflow guide paths must be relative to the checkout')
        path = local_path(root, relative)
        if not path.is_file() or not read(path):
            raise Fail(f'repository workflow guide is missing or unreadable: {relative}')
    return result['docs']


def track_of(path):
    name = Path(path).stem
    return name.removesuffix('-pipeline')


def native_items(root):
    slug = os.environ.get('WF_BRANCH', '').split('/')[-1]
    if not slug or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]*', slug):
        return []
    records = []
    for folder, filename in (('stories', 'story.md'), ('bugs', 'report.md'), ('bugs', 'bug-request.md'),
                             ('tweaks', 'note.md'), ('spikes', 'spike.md'), ('releases', 'release.md'), ('releases', 'notes.md')):
        for path in sorted(root.glob(f'docs/{folder}/*/{slug}/{filename}')):
            if path.is_file():
                records.append(path.relative_to(root).as_posix())
    return records


def route(command, args):
    root = Path(os.environ['WF_ROOT']).resolve()
    docs = guides(root)
    if command == 'asked' or (command == 'status' and '--json' in args):
        print('null')
        return 0
    if command == 'gate':
        return 0
    if command in ('branch', 'start', 'show'):
        positional = []
        index = 0
        while index < len(args):
            if args[index] in ('--lane', '--tag'):
                index += 2
            elif args[index].startswith('-'):
                raise Fail(f'unsupported option {args[index]}')
            else:
                positional.append(args[index])
                index += 1
        if not positional:
            raise Fail('name a repository track; use ramify workflow to list the guides')
        track = 'bug-fix' if positional[0] == 'bug' else positional[0]
        matches = [doc for doc in docs if track_of(doc) == track]
        if not matches and len(docs) != 1:
            raise Fail(f'no repository track {track}; read ' + ', '.join(docs))
        selected = matches or docs
        if command in ('branch', 'start'):
            if len(positional) != 2 or not re.fullmatch(r'[A-Za-z0-9][A-Za-z0-9._-]*', positional[1]):
                raise Fail('provide a track and a slug containing letters, digits, dots, underscores, or hyphens')
        if command == 'branch':
            print(os.environ.get('RAMIFY_BRANCH_PREFIX', '') + positional[1])
            return 0
        docs = selected
    if command in ('list', 'show', 'status', 'start'):
        if command == 'status' and '--line' in args:
            print('repository workflow → ramify workflow')
            return 0
        print('Repository workflow — follow the project guides and their native skills:')
        for doc in docs:
            print(f'  {track_of(doc):<12} {doc}')
        for item in native_items(root):
            print(f'  work item    {item}')
        print('Read the native work item and follow its next step. Keep its existing records and gates; do not create a Ramify work order.')
        return 0
    raise Fail('this repository owns its workflow records; follow the repository guides instead of ramify step ' + command)


if __name__ == '__main__':
    try:
        if sys.argv[1] == 'config':
            result = configured(Path(sys.argv[2]))
            if result['source'] == 'repo':
                guides(Path(sys.argv[2]))
            if result['source'] == 'ramify' and os.environ.get('RAMIFY_WORKFLOW_SOURCE', 'auto') == 'auto' and not result['docs'] and result['reason'].startswith('no repository'):
                result['source'] = 'auto'
            print('# Workflow selection: ' + result['reason'])
            print('RAMIFY_WORKFLOW_SOURCE=' + shlex.quote(result['source']))
            print('RAMIFY_WORKFLOW_DOCS=' + shlex.quote(json.dumps(result['docs'])))
        else:
            sys.exit(route(sys.argv[1], sys.argv[2:]))
    except (Fail, OSError) as exc:
        print(f'ramify: {exc}', file=sys.stderr)
        sys.exit(1)
