#!/usr/bin/env python3
"""Exercise real terminal prompts without changing the user's SSH directory."""
import codecs
import errno
import fcntl
import os
from pathlib import Path
import pty
import select
import re
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time


class Session:
    def __init__(self, binary, args, env, tui=False):
        self.pid, self.fd = pty.fork()
        if self.pid == 0:
            fcntl.ioctl(0, termios.TIOCSWINSZ, struct.pack("HHHH", 30, 110, 0, 0))
            os.execve(str(binary), [str(binary), *args], env)
        self.pending = b""
        self.transcript = b""
        self.exited = False
        self.tui = tui
        self.cells = [[" "] * 110 for _ in range(30)]
        self.row = self.column = 0
        self.decoder = codecs.getincrementaldecoder("utf-8")("replace")
        self.ansi = ""

    def render(self, data):
        self.ansi += self.decoder.decode(data)
        while self.ansi:
            if self.ansi.startswith("\x1b["):
                match = re.match(r"\x1b\[([0-?]*)([ -/]*)([@-~])", self.ansi)
                if not match:
                    break
                values, _, code = match.groups()
                self.ansi = self.ansi[match.end():]
                if code in ("H", "f"):
                    parts = values.split(";")
                    self.row = int(parts[0] or 1) - 1
                    self.column = int(parts[1] or 1) - 1 if len(parts) > 1 else 0
                elif code == "J" and values == "2":
                    self.cells = [[" "] * 110 for _ in range(30)]
                continue
            char, self.ansi = self.ansi[0], self.ansi[1:]
            if char == "\x1b":
                if not self.ansi:
                    self.ansi = char
                    break
                continue
            if char == "\r":
                self.column = 0
            elif char == "\n":
                self.row += 1
                if self.row >= 30:
                    self.cells.pop(0)
                    self.cells.append([" "] * 110)
                    self.row = 29
            elif char >= " ":
                if self.column >= 110:
                    self.column = 0
                    self.row = min(29, self.row + 1)
                if 0 <= self.row < 30 and 0 <= self.column < 110:
                    self.cells[self.row][self.column] = char
                self.column += 1

    def screen(self):
        return "\n".join("".join(row) for row in self.cells)

    def send(self, text):
        os.write(self.fd, text.encode())

    def expect(self, text, visible=True):
        expected = text.encode()
        deadline = time.monotonic() + 20
        while (text in self.screen() if not visible else expected not in self.pending and not (self.tui and text in self.screen())):
            assert time.monotonic() < deadline, f"Timed out waiting for {text!r}: {self.screen() if self.tui else self.pending[:2000]!r}"
            if not select.select([self.fd], [], [], 0.1)[0]:
                continue
            try:
                data = os.read(self.fd, 65536)
            except OSError as error:
                if error.errno == errno.EIO:
                    raise AssertionError(f"Terminal closed while waiting for {text!r}: {self.screen() if self.tui else self.pending[:2000]!r}") from error
                raise
            assert data, f"Terminal closed while waiting for {text!r}"
            self.render(data)
            self.pending += data
            self.transcript += data
        self.pending = self.pending.split(expected, 1)[1] if expected in self.pending else b""

    def finish(self, expected=0):
        deadline = time.monotonic() + 10
        while time.monotonic() < deadline:
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.exited = True
                assert os.waitstatus_to_exitcode(status) == expected, f"Program exited with status {status}"
                return
            if select.select([self.fd], [], [], 0.1)[0]:
                try:
                    data = os.read(self.fd, 65536)
                    self.transcript += data
                except OSError as error:
                    if error.errno != errno.EIO:
                        raise
        raise AssertionError("Program did not exit")

    def close(self):
        if not self.exited:
            try:
                os.kill(self.pid, signal.SIGKILL)
            except ProcessLookupError:
                pass
            os.waitpid(self.pid, 0)
        os.close(self.fd)


def main():
    binary = Path(sys.argv[1] if len(sys.argv) > 1 else "target/debug/skm").resolve()
    with tempfile.TemporaryDirectory(prefix="skm-terminal-") as temporary:
        home = Path(temporary)
        socket = home / "agent.sock"
        agent = subprocess.Popen(["ssh-agent", "-D", "-a", str(socket)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 5
            while not socket.exists():
                assert time.monotonic() < deadline, "Test agent did not start"
                time.sleep(0.02)
            env = {**os.environ, "HOME": str(home), "SSH_AUTH_SOCK": str(socket), "TERM": "xterm-256color", "LC_ALL": "C", "SSH_ASKPASS_REQUIRE": "never"}
            cli = Session(binary, ["gen"], env)
            try:
                cli.expect("Key algorithm")
                cli.send("\n")
                cli.expect("Filename in ~/.ssh/")
                cli.send("id_cli\n")
                cli.expect("Enter passphrase")
                cli.send("terminal-test-secret\n")
                cli.expect("Enter same passphrase again")
                cli.send("terminal-test-secret\n")
                cli.expect("Would you like to register this new key to a remote server now? [y/N]")
                cli.send("n\n")
                cli.expect("Enter passphrase for")
                cli.send("terminal-test-secret\n")
                cli.expect("Loaded in Agent:")
                cli.finish()
                assert b"terminal-test-secret" not in cli.transcript, "Passphrase appeared in terminal output"
            finally:
                cli.close()
            tools = home / "tools"
            tools.mkdir()
            copy_id = tools / "ssh-copy-id"
            copy_id.write_text("#!/bin/sh\nprintf 'Simulated remote failure\\n'\nexit 1\n")
            copy_id.chmod(0o700)
            remote_env = {**env, "PATH": str(tools) + os.pathsep + env["PATH"]}
            remote = Session(binary, ["gen"], remote_env)
            try:
                remote.expect("Key algorithm")
                remote.send("\n")
                remote.expect("Filename in ~/.ssh/")
                remote.send("id_remote\n")
                remote.expect("Enter passphrase")
                remote.send("\n")
                remote.expect("Enter same passphrase again")
                remote.send("\n")
                remote.expect("Would you like to register")
                remote.send("y\n")
                remote.expect("Server (user@hostname):")
                remote.send("user@example.invalid\n")
                remote.expect("SSH port")
                remote.send("2222\n")
                remote.expect("Simulated remote failure")
                remote.expect("Loaded in Agent:")
                remote.finish(expected=1)
            finally:
                remote.close()
            config_path = home / ".ssh/config"
            original = "# Preserve this file\nHost existing\n    HostName old.example.invalid\n    ProxyJump gateway\n\nHost *\n    ServerAliveInterval 30\n"
            config_path.write_text(original)
            ssh_stub = tools / "ssh"
            ssh_stub.write_text("#!/bin/sh\n[ \"$#\" = 2 ] && [ \"$1\" = -- ] && [ \"$2\" = work ] || exit 91\n[ -t 0 ] && [ -t 1 ] || exit 92\nstty -a | grep -q -- '-icanon' && exit 93\nprintf 'SSH session ready for %s\\n' \"$2\"\nIFS= read -r command\n[ \"$command\" = exit ] || exit 255\n")
            ssh_stub.chmod(0o700)
            hosts = Session(binary, ["hosts"], remote_env, tui=True)
            try:
                hosts.expect("Hosts in ~/.ssh/config")
                hosts.send("K")
                hosts.expect("[G]enerate & Setup")
                hosts.send("H")
                hosts.expect("Hosts in ~/.ssh/config")
                hosts.send("a")
                hosts.expect("Add SSH host")
                hosts.send("office\tserver.example.invalid\talice\t2222\t~/.ssh/id_cli\r")
                hosts.expect("Saved office.")
                saved = config_path.read_text()
                assert saved.endswith(original), "Adding an alias changed existing content"
                assert "Port 2222" in saved
                hosts.send("e")
                hosts.expect("Edit SSH host")
                hosts.send("\x15work\t\t\t\x152200\r")
                hosts.expect("Saved work.")
                settings = subprocess.check_output(["ssh", "-G", "-F", str(config_path), "work"], env=env, text=True, stderr=subprocess.DEVNULL)
                for value in ["hostname server.example.invalid\n", "user alice\n", "port 2200\n", "identityfile ~/.ssh/id_cli\n"]:
                    assert value in settings, settings
                hosts.expect("Enter: connect to selected host")
                hosts.send("\r")
                hosts.expect("SSH session ready for work")
                hosts.send("exit\n")
                hosts.expect("SSH session to work ended.")
                hosts.send("\r")
                hosts.expect("SSH session ready for work")
                hosts.send("fail\n")
                hosts.expect("Could not connect to SSH alias work")
                hosts.send("d")
                hosts.expect("Delete SSH host")
                hosts.send("n")
                hosts.expect("Delete SSH host", visible=False)
                assert "Host work" in config_path.read_text(), "Cancelling deletion removed an alias"
                hosts.send("d")
                hosts.expect("Delete SSH host")
                hosts.send("y")
                hosts.expect("Deleted alias work.")
                assert "Host work" not in config_path.read_text()
                assert original in config_path.read_text()
                hosts.send("q")
                hosts.finish()
                assert list((home / ".ssh").glob("config.skm-backup-*")), "No config backup was created"
            finally:
                hosts.close()
            tui = Session(binary, [], env, tui=True)
            try:
                tui.expect("ssh-key-man")
                tui.expect("Tab: switch tabs")
                tui.expect("Enter: load selected key")
                tui.expect("Key details")
                tui.send("\t")
                tui.expect("Hosts in ~/.ssh/config")
                tui.send("a")
                tui.expect("Add SSH host")
                tui.send("\x1b")
                tui.expect("Add SSH host", visible=False)
                tui.send("\x1b[Z")
                tui.expect("[G]enerate & Setup")
                tui.send("g")
                tui.expect("Generate key: step 1 of 3")
                tui.send(" \t" + "\x7f" * len("id_rsa") + "id_tui\r")
                tui.expect("Enter passphrase")
                tui.send("\n")
                tui.expect("Enter same passphrase again")
                tui.send("\n")
                tui.expect("Register key: step 2 of 3")
                tui.send("y")
                tui.expect("Server setup: step 2 of 3")
                tui.send("\r")
                tui.expect("Invalid server address.")
                tui.send("\x1b")
                tui.expect("Register key: step 2 of 3")
                tui.send("n")
                tui.expect("Loaded in Agent")
                tui.send("l")
                tui.expect("Already Loaded")
                tui.send("d")
                tui.expect("Stable link:")
                tui.send("q")
                tui.finish()
                assert b"\x1b[?1049l" in tui.transcript, "Alternate screen was not restored"
            finally:
                tui.close()
            assert (home / ".ssh/id_cli").stat().st_mode & 0o777 == 0o600
            fingerprint = subprocess.check_output(["ssh-keygen", "-lf", str(home / ".ssh/id_tui")], text=True)
            assert fingerprint.startswith("4096 ") and "(RSA)" in fingerprint, fingerprint
            assert not list((home / ".ssh").glob(".skm-key-*")), "Staging directory was not removed"
            assert (home / ".ssh/agent.sock").resolve() == socket.resolve()
        finally:
            agent.terminate()
            agent.wait(timeout=5)
    print("PASS: CLI encrypted generation, hidden passphrases, RSA TUI wizard, remote failure recovery, host alias editing, backups, cancellation, validation, loading, doctor, and terminal restoration")


if __name__ == "__main__":
    main()
