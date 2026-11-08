#!/usr/bin/env python3
"""Test installation and updates with local release fixtures and no network."""
import hashlib
import io
import os
from pathlib import Path
import shutil
import socket
import subprocess
import sys
import tarfile
import tempfile
import time
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
        self.make_archive(b"#!/bin/sh\nprintf 'ssh-key-man 0.1.1\\n'\n")
        self.script("uname", '#!/bin/sh\ncase "$1" in -s) printf "%s\\n" "$SKM_TEST_OS";; -m) printf "%s\\n" "$SKM_TEST_ARCH";; esac\n')
        self.script("curl", f'''#!{sys.executable}
import os, pathlib, shutil, sys
args = sys.argv[1:]
url = next(arg for arg in args if arg.startswith('https://'))
with open(os.environ['SKM_TEST_LOG'], 'a') as log:
    log.write(url + '\\n')
if os.environ.get('SKM_TEST_DOWNLOAD_FAIL'):
    sys.exit(22)
if url.endswith('/releases/latest'):
    if os.environ.get('SKM_TEST_FAIL_STAGE') == 'release':
        sys.exit(22)
    print(os.environ.get('SKM_TEST_RELEASE_URL', 'https://github.com/zainphp/ssh-key-man/releases/tag/v0.1.1'), end='')
    sys.exit(0)
if '/commits/' in url:
    if os.environ.get('SKM_TEST_FAIL_STAGE') == 'commit':
        sys.exit(22)
    print(os.environ.get('SKM_TEST_COMMIT', 'ab' * 20))
    sys.exit(0)
if url.startswith('https://raw.githubusercontent.com/'):
    destination = args[args.index('--output') + 1]
    if os.environ.get('SKM_TEST_FAIL_STAGE') == 'script':
        pathlib.Path(destination).write_text('touch "$HOME/partial-installer-ran"\\n')
        sys.exit(22)
    shutil.copyfile(os.environ['SKM_TEST_INSTALLER'], destination)
    sys.exit(0)
source = os.environ['SKM_TEST_FIXTURE'] + ('.sha256' if url.endswith('.sha256') else '')
shutil.copyfile(source, args[args.index('-o') + 1])
''')
        self.env = {
            **os.environ,
            "PATH": str(self.tools) + os.pathsep + os.environ["PATH"],
            "HOME": str(self.root),
            "SHELL": "/bin/bash",
            "SKM_INSTALL_DIR": str(self.destination),
            "SKM_TEST_FIXTURE": str(self.fixture),
            "SKM_TEST_LOG": str(self.root / "downloads"),
            "SKM_TEST_OS": "Linux",
            "SKM_TEST_INSTALLER": str(ROOT / "install.sh"),
            "SKM_TEST_ARCH": "x86_64",
        }
        self.env.pop("SKM_VERSION", None)
        self.env.pop("ZDOTDIR", None)
        self.env.pop("XDG_CONFIG_HOME", None)

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
        self.assertEqual(subprocess.check_output([str(self.destination / "skm"), "--version"], text=True).strip(), "ssh-key-man 0.1.1")
        self.assertFalse(list(self.destination.glob(".skm-install.*")))

    def test_install_and_update_both_command_names(self):
        for _ in range(2):
            output = self.run_installer()
            self.assertEqual(output.returncode, 0, output.stderr)
            self.assert_installed()
        executable = self.destination / "ssh-key-man"
        inode = executable.stat().st_ino
        output = self.run_installer()
        self.assertEqual(output.returncode, 0, output.stderr)
        self.assertIn("already installed", output.stdout)
        self.assertEqual(executable.stat().st_ino, inode)
        self.make_archive(b"#!/bin/sh\nprintf 'ssh-key-man 0.2.0\\n'\n")
        output = self.run_installer()
        self.assertEqual(output.returncode, 0, output.stderr)
        self.assertEqual(subprocess.check_output([str(executable), "--version"], text=True).strip(), "ssh-key-man 0.2.0")

    def test_platforms_and_pinned_release(self):
        for system, architecture, target in [
            ("Linux", "x86_64", "x86_64-unknown-linux-musl"),
            ("Linux", "aarch64", "aarch64-unknown-linux-musl"),
            ("Darwin", "x86_64", "x86_64-apple-darwin"),
            ("Darwin", "arm64", "aarch64-apple-darwin"),
        ]:
            with self.subTest(target=target):
                output = self.run_installer(SKM_TEST_OS=system, SKM_TEST_ARCH=architecture, SKM_VERSION="v0.1.1")
                self.assertEqual(output.returncode, 0, output.stderr)
                self.assertIn(f"/download/v0.1.1/ssh-key-man-{target}.tar.gz", (self.root / "downloads").read_text())

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

    def test_install_registers_shell_once(self):
        for _ in range(2):
            output = self.run_installer(SHELL="/bin/bash")
            self.assertEqual(output.returncode, 0, output.stderr)
        for name in [".bashrc", ".bash_profile"]:
            self.assertEqual((self.root / name).read_text().count("# >>> ssh-key-man >>>"), 1)
        self.assertFalse(list(self.root.glob("*.skm-backup.*")))

    def test_bash_profile_backup_and_dead_agent_fallback(self):
        profile = self.root / ".profile"
        profile.write_text("export EDITOR=vim\n")
        profile.chmod(0o640)
        output = self.run_installer()
        self.assertEqual(output.returncode, 0, output.stderr)
        self.assertFalse((self.root / ".bash_profile").exists())
        self.assertTrue(profile.read_text().startswith("export EDITOR=vim\n"))
        self.assertEqual(profile.stat().st_mode & 0o777, 0o640)
        backups = list(self.root.glob(".profile.skm-backup.*"))
        self.assertEqual(len(backups), 1)
        self.assertEqual(backups[0].read_text(), "export EDITOR=vim\n")
        self.assertEqual(backups[0].stat().st_mode & 0o777, 0o600)
        self.assertEqual(self.run_installer().returncode, 0)
        self.assertEqual(len(list(self.root.glob(".profile.skm-backup.*"))), 1)
        active = self.root / "active.sock"
        agent = subprocess.Popen(["ssh-agent", "-D", "-a", str(active)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            for _ in range(100):
                if active.exists():
                    break
                time.sleep(0.01)
            self.assertTrue(active.exists())
            dead = self.root / "dead.sock"
            with socket.socket(socket.AF_UNIX) as listener:
                listener.bind(str(dead))
            for inherited in [None, str(active), str(dead), str(self.root / "missing.sock")]:
                env = {**self.env}
                env.pop("SSH_AUTH_SOCK", None)
                if inherited:
                    env["SSH_AUTH_SOCK"] = inherited
                result = subprocess.run(["bash", "--noprofile", "--norc", "-c", '. "$HOME/.bashrc"; printf "%s" "$SSH_AUTH_SOCK"'], env=env, text=True, capture_output=True)
                self.assertEqual(result.returncode, 0, result.stderr)
                self.assertEqual(result.stdout, str(active) if inherited == str(active) else str(self.root / ".ssh/agent.sock"))
        finally:
            agent.terminate()
            agent.wait(timeout=5)

    def test_zsh_preserves_symlink_and_uncalled_export(self):
        config = self.root / "dotfiles"
        config.mkdir()
        target = config / "zshrc"
        original = 'setup_agent() {\n    export SSH_AUTH_SOCK="$HOME/.ssh/agent.sock"\n}\n'
        target.write_text(original)
        link = config / ".zshrc"
        link.symlink_to("zshrc")
        for _ in range(2):
            result = self.run_installer(SHELL="/bin/zsh", ZDOTDIR=str(config))
            self.assertEqual(result.returncode, 0, result.stderr)
        self.assertTrue(link.is_symlink())
        self.assertEqual(os.readlink(link), "zshrc")
        self.assertTrue(target.read_text().startswith(original))
        self.assertEqual(target.read_text().count("# >>> ssh-key-man >>>"), 1)
        self.assertEqual(len(list(config.glob("zshrc.skm-backup.*"))), 1)
        env = {**self.env}
        env.pop("SSH_AUTH_SOCK", None)
        result = subprocess.run(["bash", "--noprofile", "--norc", "-c", '. "$1"; printf "%s" "$SSH_AUTH_SOCK"', "bash", str(target)], env=env, capture_output=True, text=True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertEqual(result.stdout, str(self.root / ".ssh/agent.sock"))

    def test_fish_uses_dedicated_configuration(self):
        config = self.root / "settings"
        for _ in range(2):
            result = self.run_installer(SHELL="/bin/fish", XDG_CONFIG_HOME=str(config))
            self.assertEqual(result.returncode, 0, result.stderr)
        text = (config / "fish/conf.d/ssh-key-man.fish").read_text()
        self.assertEqual(text.count("# >>> ssh-key-man >>>"), 1)
        self.assertIn("set -gx SSH_AUTH_SOCK", text)
        self.assertFalse((config / "fish/config.fish").exists())

    def test_rejects_invalid_or_unsafe_shell_setup(self):
        for overrides in [{"SHELL": "/bin/tcsh"}, {"SHELL": ""}, {"SHELL": "/bin/zsh", "ZDOTDIR": "relative"}, {"SHELL": "/bin/fish", "XDG_CONFIG_HOME": "relative"}]:
            result = self.run_installer(**overrides)
            self.assertNotEqual(result.returncode, 0, result.stderr)
        file = self.root / ".zshrc"
        for original in ["# >>> ssh-key-man >>>\ncustom\n", "# <<< ssh-key-man <<<\n"]:
            file.write_text(original)
            result = self.run_installer(SHELL="/bin/zsh")
            self.assertNotEqual(result.returncode, 0)
            self.assertEqual(file.read_text(), original)
        file.write_text("existing\n")
        file.chmod(0o666)
        result = self.run_installer(SHELL="/bin/zsh")
        self.assertNotEqual(result.returncode, 0)
        self.assertEqual(file.read_text(), "existing\n")

    def test_cli_update_replaces_its_own_installation(self):
        if BINARY is None:
            self.skipTest("Pass a built skm binary to check the CLI updater")
        self.destination.mkdir()
        executable = self.destination / "skm"
        shutil.copy2(BINARY, executable)
        output = subprocess.run([str(executable), "update"], env={**self.env, "SKM_VERSION": "v9.9.9"}, capture_output=True, text=True)
        self.assertEqual(output.returncode, 0, output.stderr)
        self.assert_installed()
        downloads = (self.root / "downloads").read_text().splitlines()
        self.assertEqual(downloads[:3], [
            "https://github.com/zainphp/ssh-key-man/releases/latest",
            "https://api.github.com/repos/zainphp/ssh-key-man/commits/tags%2Fv0.1.1",
            "https://raw.githubusercontent.com/zainphp/ssh-key-man/" + "ab" * 20 + "/install.sh",
        ])
        self.assertTrue(all("/download/v0.1.1/" in url for url in downloads[3:]))

    def test_cli_update_failures_never_run_partial_installer(self):
        if BINARY is None:
            self.skipTest("Pass a built skm binary to check updater failures")
        self.destination.mkdir()
        executable = self.destination / "skm"
        shutil.copy2(BINARY, executable)
        original = executable.read_bytes()
        for overrides in [
            {"SKM_TEST_FAIL_STAGE": stage} for stage in ["release", "commit", "script"]
        ] + [
            {"SKM_TEST_RELEASE_URL": "https://example.com/releases/tag/v0.1.1"},
            {"SKM_TEST_RELEASE_URL": "https://github.com/zainphp/ssh-key-man/releases/tag/../../main"},
            {"SKM_TEST_COMMIT": "main"},
        ]:
            with self.subTest(overrides=overrides):
                output = subprocess.run([str(executable), "update"], env={**self.env, **overrides}, capture_output=True, text=True)
                self.assertNotEqual(output.returncode, 0)
                self.assertEqual(executable.read_bytes(), original)
                self.assertFalse((self.root / "partial-installer-ran").exists())
                self.assertFalse((self.destination / "ssh-key-man").exists())


if __name__ == "__main__":
    unittest.main()
