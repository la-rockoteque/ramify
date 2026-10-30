#!/usr/bin/env python3
import importlib.util
import json
import os
from pathlib import Path
import plistlib
import shutil
import subprocess
import tempfile
import unittest
from unittest.mock import patch

HOME = Path(__file__).resolve().parents[1]
spec = importlib.util.spec_from_file_location('app_launch', HOME / 'libexec/app-launch.py')
launch = importlib.util.module_from_spec(spec)
spec.loader.exec_module(launch)


class AppLaunch(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.addCleanup(self.tmp.cleanup)
        self.root = Path(self.tmp.name)
        self.target = self.root / 'source/app/approvals'
        self.target.mkdir(parents=True)
        (self.target / 'package.json').write_text('{"name":"ramify-approvals","main":"main.js"}')
        (self.target / 'main.js').write_text('// fixture\n')
        self.source_root = self.root / 'source'
        for name in ('bin', 'libexec', 'assets'):
            (self.source_root / name).mkdir()
            (self.source_root / name / 'fixture').write_text(name)
        (self.source_root / 'workflows.yml').write_text('workflows: off\n')

    def test_mac_bundle_has_unique_identity_and_default_ramify_entry(self):
        source = self.root / 'Electron.app'
        (source / 'Contents/Resources').mkdir(parents=True)
        (source / 'Contents/MacOS').mkdir()
        (source / 'Contents/MacOS/Electron').write_text('fixture')
        with (source / 'Contents/Info.plist').open('wb') as f:
            plistlib.dump({'CFBundleExecutable': 'Electron', 'CFBundleIdentifier': 'com.github.Electron'}, f)
        def command(argv, **kwargs):
            if argv[0] == 'cp':
                raise subprocess.CalledProcessError(1, argv)
            return subprocess.CompletedProcess(argv, 0)
        with patch.object(launch.subprocess, 'run', side_effect=command):
            bundle = launch.mac_bundle(source, self.target, self.root / 'cache', 'approvals')
        info = plistlib.loads((bundle / 'Contents/Info.plist').read_bytes())
        self.assertEqual(info['CFBundleIdentifier'], 'dev.ramify.approvals')
        self.assertEqual(info['CFBundleDisplayName'], 'Ramify Approvals')
        app = bundle / 'Contents/Resources/app'
        self.assertEqual(json.loads((app / 'package.json').read_text())['main'], 'main.js')
        self.assertIn('../ramify/app/approvals/main.js', (app / 'main.js').read_text())
        (self.target / 'main.js').write_text('// next version\n')
        with patch.object(launch.subprocess, 'run', side_effect=command):
            updated = launch.mac_bundle(source, self.target, self.root / 'cache', 'approvals')
        self.assertNotEqual(updated, bundle)
        self.assertEqual((bundle / 'Contents/Resources/ramify/app/approvals/main.js').read_text(), '// fixture\n')
        self.assertEqual((self.root / 'cache/apps/Ramify Approvals.app').resolve(), updated.resolve())
        shutil.rmtree(self.source_root)
        snapshot = bundle / 'Contents/Resources/ramify'
        self.assertTrue((snapshot / 'app/approvals/main.js').is_file())
        self.assertTrue((snapshot / 'bin/fixture').is_file())
        self.assertNotIn(str(self.source_root), (app / 'main.js').read_text())

    def fake(self, body):
        binary = self.root / 'electron'
        binary.write_text('#!/usr/bin/env python3\n' + body)
        binary.chmod(0o755)
        return binary

    def test_launch_waits_for_ramify_readiness(self):
        binary = self.fake('import os,json\nfrom pathlib import Path\nPath(os.environ["RAMIFY_LAUNCH_READY"]).write_text(json.dumps({"title":"ramify — approvals","visible":True}))\n')
        result = launch.run(binary, self.target, self.root / 'cache', 'approvals', timeout=2, platform='linux')
        self.assertEqual(result['title'], 'ramify — approvals')

    def test_generic_electron_is_not_success(self):
        binary = self.fake('import time\ntime.sleep(.2)\n')
        with self.assertRaisesRegex(launch.LaunchError, 'before.*ready'):
            launch.run(binary, self.target, self.root / 'cache', 'approvals', timeout=1, platform='linux')

    def test_startup_failure_retains_diagnostic_log(self):
        binary = self.fake('import sys\nprint("fixture startup failure",file=sys.stderr)\nsys.exit(7)\n')
        with self.assertRaisesRegex(launch.LaunchError, 'fixture startup failure'):
            launch.run(binary, self.target, self.root / 'cache', 'approvals', timeout=1, platform='linux')
        self.assertIn('fixture startup failure', (self.root / 'cache/launch/approvals.log').read_text())

    def test_electron_node_environment_is_removed(self):
        binary = self.fake('import os,json\nfrom pathlib import Path\nassert "ELECTRON_RUN_AS_NODE" not in os.environ\nassert "NODE_OPTIONS" not in os.environ\nPath(os.environ["RAMIFY_LAUNCH_READY"]).write_text(json.dumps({"title":"ramify — approvals","visible":True}))\n')
        with patch.dict(os.environ, {'ELECTRON_RUN_AS_NODE': '1', 'NODE_OPTIONS': '--use-openssl-ca'}):
            launch.run(binary, self.target, self.root / 'cache', 'approvals', timeout=2, platform='linux')


    def test_a_window_from_another_build_is_retired(self):
        cache = self.root / 'cache'
        bundle = cache / 'apps/approvals/new/Ramify Approvals.app'
        listing = (f' 101 {cache}/apps/approvals/old/Ramify Approvals.app/Contents/MacOS/Electron\n'
                   f' 102 {bundle}/Contents/MacOS/Electron\n'
                   f' 103 {cache}/apps/scratch/old/Ramify Scratch.app/Contents/MacOS/Electron\n')
        with patch.object(launch.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, listing)), \
                patch.object(launch.os, 'kill') as kill:
            launch.retire_other_builds(cache, 'approvals', bundle)
        kill.assert_called_once_with(101, launch.signal.SIGTERM)

    def test_newer_window_survives_an_older_launch(self):
        # A session still on the older plugin must not quit the newer window: it hands over to it.
        cache = self.root / 'cache'
        old = cache / 'apps/approvals/old/Ramify Approvals.app'
        new = cache / 'apps/approvals/new/Ramify Approvals.app'
        for bundle, mtime in ((old, 100), (new, 200)):
            bundle.mkdir(parents=True)
            os.utime(bundle.parent, (mtime, mtime))
        listing = f' 102 {new}/Contents/MacOS/Electron\n'
        with patch.object(launch.subprocess, 'run', return_value=subprocess.CompletedProcess([], 0, listing)), \
                patch.object(launch.os, 'kill') as kill:
            newer = launch.retire_other_builds(cache, 'approvals', old)
        kill.assert_not_called()
        self.assertEqual(newer, new)

    def test_ready_file_is_under_the_cache(self):
        # A sandboxed caller has its own $TMPDIR, and the app cannot write there.
        seen = self.root / 'seen'
        binary = self.fake('import os,json\nfrom pathlib import Path\nready = os.environ["RAMIFY_LAUNCH_READY"]\n'
                           f'Path({str(seen)!r}).write_text(ready)\n'
                           'Path(ready).write_text(json.dumps({"title":"ramify — approvals","visible":True}))\n')
        launch.run(binary, self.target, self.root / 'cache', 'approvals', timeout=2, platform='linux')
        self.assertTrue(Path(seen.read_text()).is_relative_to(self.root / 'cache'), seen.read_text())


if __name__ == '__main__':
    unittest.main()
