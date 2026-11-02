#!/usr/bin/env python3
"""Launch a Ramify window and wait for its readiness acknowledgement."""
import hashlib
import json
import os
from pathlib import Path
import plistlib
import re
import shutil
import signal
import subprocess
import sys
import tempfile
import threading
import time


class LaunchError(Exception):
    pass


def source_files(root):
    for name in ('app', 'bin', 'libexec', 'assets', 'workflows.yml'):
        entry = root / name
        if not entry.exists():
            raise LaunchError(f'Ramify source is incomplete: {entry}')
        paths = entry.rglob('*') if entry.is_dir() else [entry]
        for path in sorted(paths):
            if path.is_file() and not any(p in ('node_modules', '__pycache__', '.DS_Store') for p in path.parts):
                yield path


def app_alias(cache, name, bundle):
    alias = cache / 'apps' / (name + '.app')
    if alias.exists() and not alias.is_symlink():
        alias.rename(alias.with_name(alias.name + f'.previous-{time.time_ns()}'))
    temporary = alias.with_name(alias.name + f'.link-{os.getpid()}')
    temporary.symlink_to(bundle, target_is_directory=True)
    temporary.replace(alias)


def mac_bundle(source, target, cache, kind):
    name = 'Ramify' if kind == 'app' else 'Ramify ' + kind.title()
    root = target.parent if kind == 'app' else target.parents[1]
    files = list(source_files(root))
    digest = hashlib.sha256((source / 'Contents/Info.plist').read_bytes() + kind.encode())
    for path in files:
        digest.update(str(path.relative_to(root)).encode())
        digest.update(path.read_bytes())
    signature = digest.hexdigest()
    bundle = cache / 'apps' / kind / signature / (name + '.app')
    marker = bundle / 'Contents/Resources/ramify-source'
    if marker.is_file() and marker.read_text() == signature:
        app_alias(cache, name, bundle)
        return bundle
    bundle.parent.mkdir(parents=True, exist_ok=True)
    staging = Path(tempfile.mkdtemp(prefix='.ramify-', dir=bundle.parent))
    draft = staging / bundle.name
    try:
        try:
            subprocess.run(['cp', '-cR', str(source), str(draft)], check=True, capture_output=True)
        except subprocess.CalledProcessError:
            if draft.exists():
                shutil.rmtree(draft)
            shutil.copytree(source, draft, symlinks=True)
        info_path = draft / 'Contents/Info.plist'
        info = plistlib.loads(info_path.read_bytes())
        info.update(CFBundleIdentifier='dev.ramify.' + kind, CFBundleName=name, CFBundleDisplayName=name)
        info_path.write_bytes(plistlib.dumps(info))
        entry = draft / 'Contents/Resources/app'
        entry.mkdir()
        package = json.loads((target / 'package.json').read_text())
        package['main'] = 'main.js'
        (entry / 'package.json').write_text(json.dumps(package))
        snapshot = draft / 'Contents/Resources/ramify'
        for path in files:
            copied = snapshot / path.relative_to(root)
            copied.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy2(path, copied)
        main = '../ramify/' + (target / 'main.js').relative_to(root).as_posix()
        (entry / 'main.js').write_text(
            "const request = process.argv.find(arg => arg.startsWith('--ramify-launch='));\n"
            "if (request) Object.assign(process.env, JSON.parse(require('node:fs').readFileSync(request.slice(16), 'utf8')));\n"
            'process.env.RAMIFY_APP_BUILD = ' + json.dumps(signature) + ';\nrequire(' + json.dumps(main) + ');\n')
        (draft / 'Contents/Resources/ramify-source').write_text(signature)
        subprocess.run(['codesign', '--force', '--sign', '-', '--preserve-metadata=entitlements', str(draft)],
                       check=True, capture_output=True)
        previous = staging / 'previous.app'
        if bundle.exists():
            bundle.rename(previous)
        try:
            draft.rename(bundle)
        except OSError:
            if previous.exists(): previous.rename(bundle)
            raise
    finally:
        shutil.rmtree(staging)
    app_alias(cache, name, bundle)
    return bundle


def build_age(folder):
    try:
        return folder.stat().st_mtime
    except OSError:
        return 0


def retire_other_builds(cache, kind, bundle):
    """Quit a window of this kind left running from an older build: each build is its own app with
    its own ramify snapshot, so an old window would keep listing gates with old code. A window of a
    newer build stays, and its bundle is returned for the launch to hand over to. Sessions started
    before a plugin update run the older build: if each quit the other, every request in turn
    would restart the window."""
    prefix = str(cache / 'apps' / kind) + '/'
    ours = build_age(bundle.parent)
    newest = None
    listing = subprocess.run(['ps', '-axo', 'pid=,command='], capture_output=True, text=True).stdout
    for line in listing.splitlines():
        pid, _, command = line.strip().partition(' ')
        found = re.search(re.escape(prefix) + r'([^/]+)/([^/]+\.app)', command)
        if not found or str(bundle) in command or not pid.isdigit():
            continue
        other = Path(prefix + found.group(1)) / found.group(2)
        if build_age(other.parent) > ours:
            if newest is None or build_age(other.parent) > build_age(newest.parent):
                newest = other
            continue
        try:
            os.kill(int(pid), signal.SIGTERM)
        except ProcessLookupError:
            pass
    return newest


def run(binary, target, cache, kind, timeout=30, platform=sys.platform):
    binary, target, cache = Path(binary), Path(target).resolve(), Path(cache)
    if not (target / 'main.js').is_file():
        raise LaunchError(f'Ramify entry point is missing: {target}/main.js')
    if platform == 'darwin':
        import fcntl
        source = binary.parents[2]
        cache.mkdir(parents=True, exist_ok=True)
        with (cache / f'{kind}.bundle.lock').open('a') as handle:
            fcntl.flock(handle, fcntl.LOCK_EX)
            bundle = mac_bundle(source, target, cache, kind)
        bundle = retire_other_builds(cache, kind, bundle) or bundle
        command = ['open', '-n', '-a', str(bundle)]
    else:
        command = [str(binary), str(target)]
    logs = cache / 'launch'
    logs.mkdir(parents=True, exist_ok=True)
    log_path = logs / (kind + '.log')
    # Under the cache, not in $TMPDIR: a sandboxed caller has a $TMPDIR the app cannot write.
    with tempfile.TemporaryDirectory(prefix='ready-', dir=logs) as folder:
        ready = Path(folder) / 'ready.json'
        env = {k: v for k, v in os.environ.items() if k not in ('NODE_OPTIONS', 'ELECTRON_RUN_AS_NODE')}
        env['RAMIFY_LAUNCH_READY'] = str(ready)
        if platform == 'darwin':
            request = Path(folder) / 'environment.json'
            request.write_text(json.dumps(env))
            request.chmod(0o600)
            command += ['--stdout', str(log_path), '--stderr', str(log_path),
                        '--args', '--ramify-launch=' + str(request)]
        with log_path.open('ab') as output:
            child = subprocess.Popen(command, stdin=subprocess.DEVNULL, stdout=output, stderr=output,
                                     env=env, start_new_session=True)
        threading.Thread(target=child.wait, daemon=True).start()
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            try:
                result = json.loads(ready.read_text())
                if result.get('visible') and str(result.get('title', '')).lower().startswith('ramify'):
                    return result
            except (FileNotFoundError, ValueError):
                pass
            code = child.poll()
            if code is not None and code != 0:
                break
            # A successful secondary instance exits before the primary acknowledges it.
            time.sleep(.05)
        detail = log_path.read_text(errors='replace')[-2000:].strip()
        raise LaunchError(f'Ramify exited or timed out before its window was ready. Log: {log_path}' +
                          (f'\n{detail}' if detail else ''))


if __name__ == '__main__':
    try:
        run(*sys.argv[1:5])
        print(f'ramify {sys.argv[4]} ready')
    except (LaunchError, OSError, subprocess.SubprocessError, ValueError) as exc:
        print(f'ramify: {exc}', file=sys.stderr)
        sys.exit(1)
