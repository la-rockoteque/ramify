#!/usr/bin/env python3
"""Run install.sh from a checkout and from a clone, with a fake cargo, and check the links."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

HOME = Path(__file__).resolve().parents[1]
SCRIPT = HOME / "install.sh"


class Install(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name).resolve()
        self.bin = self.root / "bin"
        fake = self.root / "fake"
        fake.mkdir()
        self.cargo_log = self.root / "cargo.log"
        (fake / "cargo").write_text(f'#!/bin/sh\necho "$*" >>"{self.cargo_log}"\n')
        (fake / "cargo").chmod(0o755)
        self.env = {"HOME": str(self.root / "home"), "PATH": f"{fake}:/usr/bin:/bin",
                    "RAMIFY_BIN_DIR": str(self.bin)}
        (self.root / "home").mkdir()

    def run_install(self, *args, script=SCRIPT, stdin=None):
        cmd = ["bash", str(script), *args] if stdin is None else ["bash", "-s", "--", *args]
        return subprocess.run(cmd, env=self.env, input=stdin, capture_output=True, text=True,
                              cwd=self.root)

    def test_links_both_launchers_from_the_checkout(self):
        out = self.run_install()
        self.assertEqual(out.returncode, 0, out.stderr)
        for name in ("ram", "ramify"):
            self.assertEqual(os.readlink(self.bin / name), str(HOME / "bin" / name))
        builds = self.cargo_log.read_text()
        self.assertIn(str(HOME / "cli" / "Cargo.toml"), builds)
        self.assertIn(str(HOME / "Cargo.toml"), builds)

    def test_warns_when_the_bin_dir_is_not_on_path(self):
        out = self.run_install("--no-build")
        self.assertIn("is not on PATH", out.stderr)
        self.env["PATH"] = f"{self.bin}:{self.env['PATH']}"
        self.assertNotIn("is not on PATH", self.run_install("--no-build").stderr)

    def test_no_build_skips_cargo(self):
        out = self.run_install("--no-build")
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertFalse(self.cargo_log.exists())

    def test_refuses_to_replace_a_regular_file_without_force(self):
        self.bin.mkdir()
        (self.bin / "ram").write_text("mine\n")
        out = self.run_install("--no-build")
        self.assertNotEqual(out.returncode, 0)
        self.assertIn("--force", out.stderr)
        self.assertEqual((self.bin / "ram").read_text(), "mine\n")
        out = self.run_install("--no-build", "--force")
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertTrue((self.bin / "ram").is_symlink())

    def test_uninstall_removes_only_its_own_links(self):
        self.run_install("--no-build")
        os.remove(self.bin / "ramify")
        os.symlink("/elsewhere/ramify", self.bin / "ramify")
        out = self.run_install("--uninstall")
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertFalse((self.bin / "ram").exists() or (self.bin / "ram").is_symlink())
        self.assertEqual(os.readlink(self.bin / "ramify"), "/elsewhere/ramify")

    def test_piped_install_clones_then_updates(self):
        source = self.root / "source"
        subprocess.run(["git", "clone", "-q", "--no-hardlinks", str(HOME), str(source)], check=True)
        # The clone must carry this branch's install.sh and launchers, committed or not.
        for rel in ("install.sh", "bin/ram", "bin/ramify"):
            (source / rel).write_bytes((HOME / rel).read_bytes())
        git = ["git", "-C", str(source), "-c", "user.email=t@example.com", "-c", "user.name=t"]
        subprocess.run([*git, "checkout", "-q", "-B", "main"], check=True)
        subprocess.run([*git, "add", "-A"], check=True)
        subprocess.run([*git, "commit", "-q", "--allow-empty", "-m", "fixture"], check=True)
        self.env["RAMIFY_REPO"] = str(source)
        clone = self.root / "home" / ".local" / "share" / "ramify"
        out = self.run_install("--no-build", stdin=SCRIPT.read_text())
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn("cloning", out.stdout)
        self.assertEqual(os.readlink(self.bin / "ram"), str(clone / "bin" / "ram"))
        out = self.run_install("--no-build", stdin=SCRIPT.read_text())
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn("updating", out.stdout)

    def test_parses_on_macos_bash_3(self):
        if Path("/bin/bash").exists():
            subprocess.run(["/bin/bash", "-n", str(SCRIPT)], check=True)


if __name__ == "__main__":
    unittest.main()
