#!/usr/bin/env python3
"""Test installation and updates with local release fixtures and no network."""
import hashlib
import io
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tarfile
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[1]
BINARY = Path(sys.argv.pop(1)).resolve() if len(sys.argv) > 1 else None


class InstallerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="skm-installer-")
        self.addCleanup(self.temporary.cleanup)
        self.root = Path(self.temporary.name)
        self.tools = self.root / "tools"
        self.tools.mkdir()
        self.destination = self.root / "install with spaces"
        self.fixture = self.root / "release.tar.gz"
        self.make_archive(b"#!/bin/sh\nprintf 'ssh-key-man 0.1.0\\n'\n")
        self.script("uname", '#!/bin/sh\ncase "$1" in -s) printf "%s\\n" "$SKM_TEST_OS";; -m) printf "%s\\n" "$SKM_TEST_ARCH";; esac\n')
        self.script("curl", f'''#!{sys.executable}
import os, pathlib, shutil, sys
args = sys.argv[1:]
url = next(arg for arg in args if arg.startswith('https://'))
with open(os.environ['SKM_TEST_LOG'], 'a') as log:
    log.write(url + '\\n')
if os.environ.get('SKM_TEST_DOWNLOAD_FAIL'):
    sys.exit(22)
source = os.environ['SKM_TEST_FIXTURE'] + ('.sha256' if url.endswith('.sha256') else '')
shutil.copyfile(source, args[args.index('-o') + 1])
''')
        self.env = {
            **os.environ,
            "PATH": str(self.tools) + os.pathsep + os.environ["PATH"],
            "HOME": str(self.root),
            "SKM_INSTALL_DIR": str(self.destination),
            "SKM_TEST_FIXTURE": str(self.fixture),
            "SKM_TEST_LOG": str(self.root / "downloads"),
            "SKM_TEST_OS": "Linux",
            "SKM_TEST_ARCH": "x86_64",
        }
        self.env.pop("SKM_VERSION", None)

    def script(self, name, text):
        path = self.tools / name
        path.write_text(text)
        path.chmod(0o755)

    def make_archive(self, content, symlink=False):
        with tarfile.open(self.fixture, "w:gz") as archive:
            member = tarfile.TarInfo("ssh-key-man")
            if symlink:
                member.type = tarfile.SYMTYPE
                member.linkname = "/bin/sh"
            else:
                member.size = len(content)
                member.mode = 0o755
            archive.addfile(member, None if symlink else io.BytesIO(content))
        self.fixture.with_suffix(".gz.sha256").write_text(hashlib.sha256(self.fixture.read_bytes()).hexdigest() + "  release.tar.gz\n")

    def run_installer(self, **overrides):
        return subprocess.run(["sh", str(ROOT / "install.sh")], env={**self.env, **overrides}, capture_output=True, text=True)

    def assert_installed(self):
        binary = self.destination / "ssh-key-man"
        self.assertTrue(binary.is_file())
        self.assertTrue(os.access(binary, os.X_OK))
        self.assertTrue((self.destination / "skm").is_symlink())
        self.assertEqual(os.readlink(self.destination / "skm"), "ssh-key-man")
        self.assertEqual(subprocess.check_output([str(self.destination / "skm"), "--version"], text=True).strip(), "ssh-key-man 0.1.0")
        self.assertFalse(list(self.destination.glob(".skm-install.*")))

    def test_install_and_update_both_command_names(self):
        for _ in range(2):
            output = self.run_installer()
            self.assertEqual(output.returncode, 0, output.stderr)
            self.assert_installed()

    def test_platforms_and_pinned_release(self):
        for system, architecture, target in [
            ("Linux", "x86_64", "x86_64-unknown-linux-musl"),
            ("Linux", "aarch64", "aarch64-unknown-linux-musl"),
            ("Darwin", "x86_64", "x86_64-apple-darwin"),
            ("Darwin", "arm64", "aarch64-apple-darwin"),
        ]:
            with self.subTest(target=target):
                output = self.run_installer(SKM_TEST_OS=system, SKM_TEST_ARCH=architecture, SKM_VERSION="v0.1.0")
                self.assertEqual(output.returncode, 0, output.stderr)
                self.assertIn(f"/download/v0.1.0/ssh-key-man-{target}.tar.gz", (self.root / "downloads").read_text())

    def test_failed_download_and_checksum_keep_existing_installation(self):
        self.assertEqual(self.run_installer().returncode, 0)
        before = (self.destination / "ssh-key-man").read_bytes()
        self.fixture.with_suffix(".gz.sha256").write_text("0" * 64)
        for overrides in [{}, {"SKM_TEST_DOWNLOAD_FAIL": "1"}]:
            with self.subTest(overrides=overrides):
                output = self.run_installer(**overrides)
                self.assertNotEqual(output.returncode, 0)
                self.assertEqual((self.destination / "ssh-key-man").read_bytes(), before)
                self.assert_installed()

    def test_rejects_unsupported_platform_and_symlinked_binary(self):
        self.assertNotEqual(self.run_installer(SKM_TEST_OS="Windows_NT").returncode, 0)
        self.assertNotEqual(self.run_installer(SKM_TEST_ARCH="riscv64").returncode, 0)
        self.make_archive(b"", symlink=True)
        output = self.run_installer()
        self.assertNotEqual(output.returncode, 0)
        self.assertFalse((self.destination / "ssh-key-man").exists())

    def test_cli_update_replaces_its_own_installation(self):
        if BINARY is None:
            self.skipTest("Pass a built skm binary to check the CLI updater")
        self.destination.mkdir()
        executable = self.destination / "skm"
        shutil.copy2(BINARY, executable)
        output = subprocess.run([str(executable), "update"], env=self.env, capture_output=True, text=True)
        self.assertEqual(output.returncode, 0, output.stderr)
        self.assert_installed()


if __name__ == "__main__":
    unittest.main()
