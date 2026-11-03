#!/usr/bin/env python3
"""Run install.sh from a checkout and from a clone, with a fake cargo, and check the links."""
import hashlib
import os
import platform
import tarfile
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

    def source_repo(self):
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

    def test_piped_install_clones_then_updates(self):
        self.source_repo()
        clone = self.root / "home" / ".local" / "share" / "ramify"
        out = self.run_install("--from-source", "--no-build", stdin=SCRIPT.read_text())
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn("cloning", out.stdout)
        self.assertEqual(os.readlink(self.bin / "ram"), str(clone / "bin" / "ram"))
        out = self.run_install("--from-source", "--no-build", stdin=SCRIPT.read_text())
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn("updating", out.stdout)

    def test_piped_install_clones_into_a_missing_explicit_home(self):
        self.source_repo()
        home = self.root / "elsewhere" / "ramify"
        self.env["RAMIFY_HOME"] = str(home)
        out = self.run_install("--from-source", "--no-build", stdin=SCRIPT.read_text())
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn("cloning", out.stdout)
        self.assertEqual(os.readlink(self.bin / "ram"), str(home / "bin" / "ram"))

    def test_an_existing_explicit_home_is_not_pulled(self):
        self.env["RAMIFY_HOME"] = str(HOME)
        out = self.run_install("--no-build", stdin=SCRIPT.read_text())
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertNotIn("updating", out.stdout)
        self.assertEqual(os.readlink(self.bin / "ram"), str(HOME / "bin" / "ram"))

    def release(self, tag, *, latest=True):
        """Publish a fake release archive for this machine under file://, as the workflow would."""
        arch = {"arm64": "aarch64", "aarch64": "aarch64"}.get(platform.machine(), "x86_64")
        os_name = "apple-darwin" if platform.system() == "Darwin" else "unknown-linux-gnu"
        asset = f"ramify-{arch}-{os_name}.tar.gz"
        stage = self.root / "stage" / tag
        for rel in ("Cargo.toml", "workflows.yml", "bin/ram", "bin/ramify"):
            (stage / rel).parent.mkdir(parents=True, exist_ok=True)
            (stage / rel).write_bytes((HOME / rel).read_bytes())
            (stage / rel).chmod((HOME / rel).stat().st_mode)
        for rel in ("target/release/ram-code", "cli/target/release/ramify"):
            (stage / rel).parent.mkdir(parents=True, exist_ok=True)
            (stage / rel).write_text(f'#!/bin/sh\necho "{tag} $(basename "$0")"\n')
            (stage / rel).chmod(0o755)
        (stage / "RELEASE").write_text(tag + "\n")
        releases = self.root / "releases"
        dirs = [releases / "download" / tag] + ([releases / "latest" / "download"] if latest else [])
        for d in dirs:
            d.mkdir(parents=True, exist_ok=True)
            with tarfile.open(d / asset, "w:gz") as tar:
                for item in stage.iterdir():
                    tar.add(item, arcname=item.name)
            digest = hashlib.sha256((d / asset).read_bytes()).hexdigest()
            (d / f"{asset}.sha256").write_text(f"{digest}  {asset}\n")
        self.env["RAMIFY_RELEASES"] = f"file://{releases}"
        return dirs[-1] / asset

    def default_home(self):
        return self.root / "home" / ".local" / "share" / "ramify"

    def run_bin(self, name):
        return subprocess.run([str(self.bin / name), "--version"], capture_output=True, text=True,
                              env=self.env).stdout.strip()

    def test_piped_install_downloads_the_latest_release_without_cargo(self):
        self.release("v1.0.0")
        out = self.run_install(stdin=SCRIPT.read_text())
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertIn("installed v1.0.0", out.stdout)
        self.assertNotIn("unbound variable", out.stderr)
        self.assertEqual(os.readlink(self.bin / "ram"), str(self.default_home() / "bin" / "ram"))
        self.assertEqual(self.run_bin("ram"), "v1.0.0 ram-code")
        self.assertEqual(self.run_bin("ramify"), "v1.0.0 ramify")
        self.assertFalse(self.cargo_log.exists())

    def test_a_second_run_replaces_the_release(self):
        self.release("v1.0.0")
        self.run_install(stdin=SCRIPT.read_text())
        self.release("v1.1.0")
        out = self.run_install(stdin=SCRIPT.read_text())
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(self.run_bin("ram"), "v1.1.0 ram-code")
        leftovers = [p.name for p in self.default_home().parent.iterdir() if p.name != "ramify"]
        self.assertEqual(leftovers, [])

    def test_version_pins_a_release(self):
        self.release("v1.0.0", latest=False)
        self.release("v2.0.0")
        out = self.run_install("--version", "v1.0.0", stdin=SCRIPT.read_text())
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertEqual(self.run_bin("ram"), "v1.0.0 ram-code")

    def test_a_bad_checksum_installs_nothing(self):
        asset = self.release("v1.0.0")
        Path(f"{asset}.sha256").write_text("0" * 64 + "  x\n")
        out = self.run_install(stdin=SCRIPT.read_text())
        self.assertNotEqual(out.returncode, 0)
        self.assertIn("checksum", out.stderr)
        self.assertFalse(self.default_home().exists())
        self.assertFalse((self.bin / "ram").exists())

    def test_a_release_replaces_an_earlier_source_install(self):
        self.source_repo()
        self.run_install("--from-source", "--no-build", stdin=SCRIPT.read_text())
        self.assertTrue((self.default_home() / ".git").is_dir())
        self.release("v1.0.0")
        out = self.run_install(stdin=SCRIPT.read_text())
        self.assertEqual(out.returncode, 0, out.stderr)
        self.assertFalse((self.default_home() / ".git").exists())
        self.assertEqual(self.run_bin("ram"), "v1.0.0 ram-code")

    def test_parses_on_macos_bash_3(self):
        if Path("/bin/bash").exists():
            subprocess.run(["/bin/bash", "-n", str(SCRIPT)], check=True)


if __name__ == "__main__":
    unittest.main()
