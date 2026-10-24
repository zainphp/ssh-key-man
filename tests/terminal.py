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
                cli.expect("Key type")
                cli.send("\n")
                cli.expect("Filename in ~/.ssh/")
                cli.send("id_cli\n")
                cli.expect("Enter passphrase")
                cli.send("terminal-test-secret\n")
                cli.expect("Enter same passphrase again")
                cli.send("terminal-test-secret\n")
                cli.expect("Add this public key to a server now? [y/N]")
                cli.send("n\n")
                cli.expect("Enter passphrase for")
                cli.send("terminal-test-secret\n")
                cli.expect("Loaded in agent:")
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
                remote.expect("Key type")
                remote.send("\n")
                remote.expect("Filename in ~/.ssh/")
                remote.send("id_remote\n")
                remote.expect("Enter passphrase")
                remote.send("\n")
                remote.expect("Enter same passphrase again")
                remote.send("\n")
                remote.expect("Add this public key")
                remote.send("y\n")
                remote.expect("Server (user@hostname):")
                remote.send("user@example.invalid\n")
                remote.expect("SSH port")
                remote.send("2222\n")
                remote.expect("Simulated remote failure")
                remote.expect("Loaded in agent:")
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
                hosts.expect("[N] New key")
                hosts.send("H")
                hosts.expect("Hosts in ~/.ssh/config")
                hosts.send("\x1b[B\x1b[B")
                hosts.expect("Enter: run action")
                hosts.send("\r")
                hosts.expect("Add SSH host")
                hosts.send("office\x1b[Bserver.example.invalid\x1b[Balice\x1b[B2222\x1b[B~/.ssh/id_cli\r")
                hosts.expect("Saved office.")
                saved = config_path.read_text()
                assert saved.endswith(original), "Adding an alias changed existing content"
                assert "Port 2222" in saved
                hosts.send("\r")
                hosts.expect("Message", visible=False)
                hosts.send("e")
                hosts.expect("Edit SSH host")
                hosts.send("\x15work\x1b[B\x1b[B\x1b[B\x152200\r")
                hosts.expect("Saved work.")
                settings = subprocess.check_output(["ssh", "-G", "-F", str(config_path), "work"], env=env, text=True, stderr=subprocess.DEVNULL)
                for value in ["hostname server.example.invalid\n", "user alice\n", "port 2200\n", "identityfile ~/.ssh/id_cli\n"]:
                    assert value in settings, settings
                hosts.send("\r")
                hosts.expect("Message", visible=False)
                hosts.send("\x1b")
                hosts.finish()
            finally:
                hosts.close()
            for command, expected in [("exit", 0), ("fail", 1)]:
                connection = Session(binary, ["hosts"], remote_env, tui=True)
                try:
                    connection.expect("Enter: connect and close skm")
                    connection.send("\r")
                    connection.expect("SSH session ready for work")
                    connection.send(command + "\n")
                    if expected:
                        connection.expect("Could not connect to SSH alias work")
                    connection.finish(expected=expected)
                    assert connection.transcript.count(b"\x1b[?1049h") == 1, "The TUI reopened after SSH"
                finally:
                    connection.close()
            hosts = Session(binary, ["hosts"], env, tui=True)
            try:
                hosts.expect("Hosts in ~/.ssh/config")
                hosts.send("d")
                hosts.expect("Delete SSH host")
                hosts.send("\r")
                hosts.expect("Delete SSH host", visible=False)
                assert "Host work" in config_path.read_text(), "The default confirmation deleted an alias"
                hosts.send("d")
                hosts.expect("Delete SSH host")
                hosts.send("\x1b[B\r")
                hosts.expect("Deleted alias work.")
                assert "Host work" not in config_path.read_text()
                assert original in config_path.read_text()
                hosts.send("\r")
                hosts.expect("Message", visible=False)
                hosts.send("\x1b")
                hosts.finish()
                assert list((home / ".ssh").glob("config.skm-backup-*")), "No config backup was created"
            finally:
                hosts.close()
            tui = Session(binary, [], env, tui=True)
            try:
                tui.expect("Hosts in ~/.ssh/config")
                tui.send("\t")
                tui.send("\x1b[D")
                tui.expect("Enter: load key and close skm")
                tui.expect("Key details")
                assert " Status " not in tui.screen()
                assert "Esc: quit" not in tui.screen()
                tui.send("\x1b[C")
                tui.expect("Hosts in ~/.ssh/config")
                tui.send("n")
                tui.expect("Add SSH host")
                tui.send("\x1b")
                tui.expect("Add SSH host", visible=False)
                tui.send("K")
                tui.expect("[N] New key")
                tui.send("n")
                tui.expect("Create key: step 1 of 3")
                tui.send("\x1b[C\x1b[B" + "\x7f" * len("id_rsa") + "id_tui\r")
                tui.expect("Enter passphrase")
                tui.send("\n")
                tui.expect("Enter same passphrase again")
                tui.send("\n")
                tui.expect("Add public key: step 2 of 3")
                tui.send("\x1b[A\r")
                tui.expect("Server setup: step 2 of 3")
                tui.send("\r")
                tui.expect("Invalid server address.")
                tui.send("\x1b")
                tui.expect("Add public key: step 2 of 3")
                tui.send("\x1b[B\r")
                tui.expect("Message")
                tui.expect("Loaded in agent")
                tui.send("\r")
                tui.expect("Message", visible=False)
                fingerprint = subprocess.check_output(["ssh-keygen", "-lf", str(home / ".ssh/id_tui")], text=True)
                assert fingerprint.startswith("4096 ") and "(RSA)" in fingerprint, fingerprint
                tui.send("\x1b[B\x1b[Bd")
                tui.expect("Delete key")
                tui.send("\r")
                tui.expect("Delete key", visible=False)
                assert (home / ".ssh/id_tui").exists(), "Default confirmation deleted a key"
                tui.send("\x1b[B\x1b[C\r")
                tui.expect("Delete key")
                tui.send("\x1b[B\r")
                tui.expect("Deleted local key files")
                assert not (home / ".ssh/id_tui").exists()
                assert not (home / ".ssh/id_tui.pub").exists()
                identities = subprocess.check_output(["ssh-add", "-l"], env=env, text=True)
                assert fingerprint.split()[1] not in identities, "Deleted key remains loaded in the agent"
                tui.send("\r")
                tui.expect("Message", visible=False)
                tui.send("\x1b")
                tui.finish()
                assert b"\x1b[?1049l" in tui.transcript, "Alternate screen was not restored"
            finally:
                tui.close()
            subprocess.run(["ssh-keygen", "-q", "-t", "ed25519", "-N", "", "-f", str(home / ".ssh/id_00_unloaded")], check=True)
            load = Session(binary, [], env, tui=True)
            try:
                load.expect("Hosts in ~/.ssh/config")
                load.send("K")
                load.expect("Enter: load key and close skm")
                load.send("\r")
                load.expect("Loaded in agent")
                load.finish()
                assert load.transcript.count(b"\x1b[?1049h") == 1, "The TUI reopened after loading a key"
                identities = subprocess.check_output(["ssh-add", "-l"], env=env, text=True)
                key_fingerprint = subprocess.check_output(["ssh-keygen", "-lf", str(home / ".ssh/id_00_unloaded")], text=True).split()[1]
                assert key_fingerprint in identities
            finally:
                load.close()
            (home / ".ssh/id_00_unloaded").chmod(0o644)
            failed_load = Session(binary, [], env, tui=True)
            try:
                failed_load.expect("Hosts in ~/.ssh/config")
                failed_load.send("K")
                failed_load.expect("Enter: load key and close skm")
                failed_load.send("\r")
                failed_load.expect("permissions 600")
                failed_load.finish(expected=1)
                assert failed_load.transcript.count(b"\x1b[?1049h") == 1, "The TUI reopened after a failed key load"
            finally:
                failed_load.close()
                (home / ".ssh/id_00_unloaded").chmod(0o600)
            assert (home / ".ssh/id_cli").stat().st_mode & 0o777 == 0o600
            assert not list((home / ".ssh").glob(".skm-key-*")), "Staging directory was not removed"
            assert (home / ".ssh/agent.sock").resolve() == socket.resolve()
        finally:
            agent.terminate()
            agent.wait(timeout=5)
    print("PASS: CLI encrypted generation, hidden passphrases, RSA TUI wizard, remote failure recovery, host alias editing, backups, cancellation, validation, loading, arrow pickers, Esc quit, terminal actions that close skm, confirmed key deletion, agent unloading, and terminal restoration")


if __name__ == "__main__":
    main()
