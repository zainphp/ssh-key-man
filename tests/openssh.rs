#![cfg(unix)]

use ssh_key_man::{agent, keys};
use std::{
    fs,
    os::unix::fs::{symlink, PermissionsExt},
    path::Path,
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, Instant},
};

struct RunningAgent(Child);
impl Drop for RunningAgent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn start_agent(socket: &Path) -> RunningAgent {
    let child = Command::new("ssh-agent")
        .args(["-D", "-a"])
        .arg(socket)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("OpenSSH must be installed for integration tests");
    let agent = RunningAgent(child);
    let start = Instant::now();
    while !socket.exists() {
        assert!(
            start.elapsed() < Duration::from_secs(3),
            "ssh-agent did not start"
        );
        thread::sleep(Duration::from_millis(20));
    }
    agent
}

fn generate(dir: &Path, name: &str, kind: &str, passphrase: &str) {
    let status = Command::new("ssh-keygen")
        .args(["-q", "-t", kind, "-N", passphrase, "-f"])
        .arg(dir.join(name))
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn discovers_keys_checks_fingerprints_and_skips_duplicate_loads() {
    let dir = tempfile::tempdir().unwrap();
    generate(dir.path(), "id_test", "ed25519", "");
    generate(dir.path(), "id_locked", "ed25519", "test-only-passphrase");
    fs::write(
        dir.path().join("config"),
        "Host example\n  Hostname example.com\n",
    )
    .unwrap();
    fs::write(dir.path().join("broken.pub"), "not a key").unwrap();
    let inventory = keys::inventory(dir.path()).unwrap();
    assert_eq!(inventory.keys.len(), 2);
    assert_eq!(inventory.warnings.len(), 1);
    assert!(inventory
        .keys
        .iter()
        .all(|k| k.private && k.algorithm == "ssh-ed25519"));
    let key = inventory
        .keys
        .into_iter()
        .find(|k| k.path.ends_with("id_test"))
        .unwrap();
    let fingerprint = Command::new("ssh-keygen")
        .args(["-lf"])
        .arg(&key.path)
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&fingerprint.stdout).contains(&key.fingerprint));
    let socket = dir.path().join("agent-test.sock");
    let _running = start_agent(&socket);
    let mut agent = agent::probe(&socket).unwrap();
    assert!(agent.fingerprints.is_empty());
    assert!(agent::load(&mut agent, &key).unwrap());
    assert!(!agent::load(&mut agent, &key).unwrap());
    assert_eq!(agent.fingerprints.len(), 1);
    fs::set_permissions(&key.path, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(agent::load(&mut agent, &key).is_err());
}

#[test]
fn doctor_updates_stable_link_without_changing_parent_environment() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join(".ssh");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = home.path().join("agent-test.sock");
    let _running = start_agent(&socket);
    let run = || {
        Command::new(env!("CARGO_BIN_EXE_skm"))
            .arg("doctor")
            .env("HOME", home.path())
            .env("SSH_AUTH_SOCK", &socket)
            .output()
            .unwrap()
    };
    let output = run();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        fs::canonicalize(dir.join("agent.sock")).unwrap(),
        fs::canonicalize(&socket).unwrap()
    );
    assert!(run().status.success());
    fs::remove_file(dir.join("agent.sock")).unwrap();
    fs::write(dir.join("agent.sock"), "keep").unwrap();
    assert!(!run().status.success());
    assert_eq!(fs::read_to_string(dir.join("agent.sock")).unwrap(), "keep");
}

#[test]
fn rejects_dead_socket_and_symlinked_private_keys() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("dead.sock");
    let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
    drop(listener);
    assert!(agent::probe(&socket).is_err());
    generate(dir.path(), "id_test", "ed25519", "");
    let mut key = keys::inventory(dir.path()).unwrap().keys.remove(0);
    let link = dir.path().join("key_link");
    symlink(&key.path, &link).unwrap();
    key.path = link;
    let mut agent = agent::Agent {
        socket,
        fingerprints: Default::default(),
    };
    assert!(agent::load(&mut agent, &key).is_err());
}

#[test]
fn shell_initialization_is_clean_and_does_not_create_ssh_directory() {
    let home = tempfile::tempdir().unwrap();
    for binary in [env!("CARGO_BIN_EXE_skm"), env!("CARGO_BIN_EXE_ssh-key-man")] {
        for (shell, expected) in [
            ("bash", "export SSH_AUTH_SOCK=\"$HOME/.ssh/agent.sock\"\n"),
            ("zsh", "export SSH_AUTH_SOCK=\"$HOME/.ssh/agent.sock\"\n"),
            ("fish", "set -gx SSH_AUTH_SOCK \"$HOME/.ssh/agent.sock\"\n"),
        ] {
            let output = Command::new(binary)
                .args(["init", shell])
                .env("HOME", home.path())
                .output()
                .unwrap();
            assert!(output.status.success());
            assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
            assert!(output.stderr.is_empty());
        }
        let help = Command::new(binary).arg("--help").output().unwrap();
        assert!(help.status.success());
        let gen = Command::new(binary)
            .arg("gen")
            .stdin(Stdio::null())
            .output()
            .unwrap();
        assert!(!gen.status.success());
        assert!(String::from_utf8_lossy(&gen.stderr).contains("interactive terminal"));
    }
    assert!(!home.path().join(".ssh").exists());
}

#[test]
fn deployment_rejects_mismatched_public_key_before_connecting() {
    let dir = tempfile::tempdir().unwrap();
    generate(dir.path(), "id_first", "ed25519", "");
    generate(dir.path(), "id_second", "ed25519", "");
    let key = keys::inventory(dir.path())
        .unwrap()
        .keys
        .into_iter()
        .find(|k| k.path.ends_with("id_first"))
        .unwrap();
    fs::copy(
        dir.path().join("id_second.pub"),
        dir.path().join("id_first.pub"),
    )
    .unwrap();
    let error = keys::deploy(&key, "user@example.invalid", 22).unwrap_err();
    assert!(error.to_string().contains("does not match"));
}

fn doctor_with_isolated_tools(home: &Path, socket: &Path) -> std::process::Output {
    let tools = home.join("tools");
    fs::create_dir_all(&tools).unwrap();
    for (name, script) in [
        ("ssh-add", "#!/bin/sh\n[ \"$SSH_AUTH_SOCK\" = \"$SKM_TEST_SOCKET\" ] || exit 2\nexec \"$SKM_TEST_SSH_ADD\" \"$@\"\n"),
        ("ssh-agent", "#!/bin/sh\n\"$SKM_TEST_SSH_AGENT\" -a \"$SKM_TEST_SOCKET\" \"$@\" > \"$SKM_TEST_AGENT_OUTPUT\"\n/bin/cat \"$SKM_TEST_AGENT_OUTPUT\"\n"),
    ] {
        fs::write(tools.join(name), script).unwrap();
        fs::set_permissions(tools.join(name), fs::Permissions::from_mode(0o700)).unwrap();
    }
    let mut paths = vec![tools];
    paths.extend(std::env::split_paths(&std::env::var_os("PATH").unwrap()));
    Command::new(env!("CARGO_BIN_EXE_skm"))
        .arg("doctor")
        .env("HOME", home)
        .env("TMPDIR", home)
        .env("PATH", std::env::join_paths(paths).unwrap())
        .env("SSH_AUTH_SOCK", home.join("missing.sock"))
        .env("SKM_TEST_SOCKET", socket)
        .env("SKM_TEST_SSH_ADD", which::which("ssh-add").unwrap())
        .env("SKM_TEST_SSH_AGENT", which::which("ssh-agent").unwrap())
        .env("SKM_TEST_AGENT_OUTPUT", home.join("agent-output"))
        .output()
        .unwrap()
}

#[test]
fn doctor_discovers_socket_in_temporary_ssh_directory() {
    let home = tempfile::tempdir().unwrap();
    let socket_dir = home.path().join("ssh-discovery");
    fs::create_dir(&socket_dir).unwrap();
    let socket = socket_dir.join("agent.test");
    let _running = start_agent(&socket);
    let output = doctor_with_isolated_tools(home.path(), &socket);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        !home.path().join("agent-output").exists(),
        "An agent was started instead of discovering the existing one"
    );
    assert_eq!(
        fs::canonicalize(home.path().join(".ssh/agent.sock")).unwrap(),
        fs::canonicalize(&socket).unwrap()
    );
}

#[test]
fn doctor_starts_agent_when_no_allowed_socket_responds() {
    struct AgentPid(Option<i32>);
    impl Drop for AgentPid {
        fn drop(&mut self) {
            if let Some(pid) = self.0 {
                unsafe {
                    libc::kill(pid, libc::SIGTERM);
                }
            }
        }
    }
    let home = tempfile::tempdir().unwrap();
    let socket = home.path().join("new-agent.sock");
    let output = doctor_with_isolated_tools(home.path(), &socket);
    let emitted = fs::read_to_string(home.path().join("agent-output")).unwrap_or_default();
    let pid = emitted.lines().find_map(|line| {
        line.strip_prefix("SSH_AGENT_PID=")
            .and_then(|s| s.split(';').next())
            .and_then(|s| s.parse().ok())
    });
    let _guard = AgentPid(pid);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(pid.is_some());
    assert!(agent::probe(&socket).is_ok());
    assert_eq!(
        fs::canonicalize(home.path().join(".ssh/agent.sock")).unwrap(),
        fs::canonicalize(&socket).unwrap()
    );
}

#[test]
fn deployment_rejects_paths_that_ssh_copy_id_could_evaluate() {
    let home = tempfile::tempdir().unwrap();
    let dir = home.path().join("unsafe$key");
    fs::create_dir(&dir).unwrap();
    generate(&dir, "id_test", "ed25519", "");
    let key = keys::inventory(&dir).unwrap().keys.remove(0);
    let error = keys::deploy(&key, "user@example.invalid", 22).unwrap_err();
    assert!(error.to_string().contains("cannot safely handle"));
}

#[test]
fn deployment_rejects_multiple_public_keys_in_one_file() {
    let dir = tempfile::tempdir().unwrap();
    generate(dir.path(), "id_test", "ed25519", "");
    let key = keys::inventory(dir.path()).unwrap().keys.remove(0);
    let path = keys::public_path(&key.path);
    let public = fs::read_to_string(&path).unwrap();
    fs::write(&path, format!("{public}{public}")).unwrap();
    let error = keys::deploy(&key, "user@example.invalid", 22).unwrap_err();
    assert!(error.to_string().contains("exactly one key"));
}
