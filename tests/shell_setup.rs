#![cfg(unix)]
use std::{
    fs,
    os::unix::{
        fs::{symlink, PermissionsExt},
        net::UnixListener,
    },
    path::Path,
    process::{Child, Command, Output, Stdio},
    thread,
    time::Duration,
};

struct TestAgent(Child);
impl Drop for TestAgent {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

fn install(home: &Path, shell: &str, extra: &[(&str, &str)]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_skm"));
    command
        .args(["init", "--install"])
        .env("HOME", home)
        .env("SHELL", shell)
        .env_remove("ZDOTDIR")
        .env_remove("XDG_CONFIG_HOME");
    for (name, value) in extra {
        command.env(name, value);
    }
    command.output().unwrap()
}
fn succeeds(output: Output) {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn bash_setup_preserves_profile_and_is_repeatable() {
    let home = tempfile::tempdir().unwrap();
    let profile = home.path().join(".profile");
    fs::write(&profile, "export EDITOR=vim\n").unwrap();
    let output = install(home.path(), "/bin/bash", &[]);
    assert!(String::from_utf8_lossy(&output.stdout).contains("Added SSH agent settings to"));
    succeeds(output);
    let first = fs::read_to_string(&profile).unwrap();
    assert!(first.starts_with("export EDITOR=vim\n"));
    assert!(!home.path().join(".bash_profile").exists());
    let output = install(home.path(), "/bin/bash", &[]);
    let message = String::from_utf8_lossy(&output.stdout);
    assert_eq!(
        message
            .matches("SSH agent settings already exist in")
            .count(),
        2
    );
    assert!(!message.contains("Added SSH agent settings to"));
    succeeds(output);
    assert_eq!(fs::read_to_string(&profile).unwrap(), first);
    let backups: Vec<_> = fs::read_dir(home.path())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().contains(".skm-backup-"))
        .collect();
    assert_eq!(backups.len(), 1);
    assert_eq!(
        fs::read_to_string(backups[0].path()).unwrap(),
        "export EDITOR=vim\n"
    );
    assert_eq!(
        fs::metadata(backups[0].path())
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );

    let socket = home.path().join("existing.sock");
    let _agent = TestAgent(
        Command::new("ssh-agent")
            .args(["-D", "-a"])
            .arg(&socket)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap(),
    );
    for _ in 0..100 {
        if socket.exists() {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }
    assert!(socket.exists());
    let dead = home.path().join("dead.sock");
    drop(UnixListener::bind(&dead).unwrap());
    for existing in [
        None,
        Some(socket.as_path()),
        Some(Path::new("/missing/skm.sock")),
        Some(dead.as_path()),
    ] {
        let mut command = Command::new("bash");
        command
            .args([
                "--noprofile",
                "--norc",
                "-c",
                ". \"$HOME/.bashrc\"; printf '%s' \"$SSH_AUTH_SOCK\"",
            ])
            .env("HOME", home.path())
            .env_remove("SSH_AUTH_SOCK");
        if let Some(path) = existing {
            command.env("SSH_AUTH_SOCK", path);
        }
        let output = command.output().unwrap();
        assert!(output.status.success());
        let expected = if existing == Some(socket.as_path()) {
            socket.clone()
        } else {
            home.path().join(".ssh/agent.sock")
        };
        assert_eq!(
            String::from_utf8(output.stdout).unwrap(),
            expected.to_str().unwrap()
        );
    }
}

#[test]
fn zsh_setup_follows_dotfile_link_and_respects_zdotdir() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join("dotfiles");
    fs::create_dir(&config).unwrap();
    let target = config.join("zshrc");
    fs::write(&target, "# existing\n").unwrap();
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640)).unwrap();
    let link = config.join(".zshrc");
    symlink(&target, &link).unwrap();
    succeeds(install(
        home.path(),
        "/bin/zsh",
        &[("ZDOTDIR", config.to_str().unwrap())],
    ));
    assert!(link.symlink_metadata().unwrap().is_symlink());
    assert!(fs::read_to_string(target)
        .unwrap()
        .starts_with("# existing\n"));
    assert_eq!(
        fs::metadata(link).unwrap().permissions().mode() & 0o777,
        0o640
    );
    assert!(!home.path().join(".zshrc").exists());
}

#[test]
fn fish_uses_its_own_configuration_file() {
    let home = tempfile::tempdir().unwrap();
    let config = home.path().join("settings");
    succeeds(install(
        home.path(),
        "/bin/fish",
        &[("XDG_CONFIG_HOME", config.to_str().unwrap())],
    ));
    let file = config.join("fish/conf.d/ssh-key-man.fish");
    let first = fs::read_to_string(&file).unwrap();
    assert!(first.contains("set -gx SSH_AUTH_SOCK"));
    succeeds(install(
        home.path(),
        "/bin/fish",
        &[("XDG_CONFIG_HOME", config.to_str().unwrap())],
    ));
    assert_eq!(fs::read_to_string(file).unwrap(), first);
    assert!(!config.join("fish/config.fish").exists());
}

#[test]
fn rejects_unsafe_or_edited_configuration_without_overwriting() {
    let home = tempfile::tempdir().unwrap();
    assert!(!install(home.path(), "/bin/tcsh", &[]).status.success());
    assert!(
        !install(home.path(), "/bin/zsh", &[("ZDOTDIR", "relative")])
            .status
            .success()
    );
    let file = home.path().join(".zshrc");
    let original = "# >>> ssh-key-man >>>\ncustom\n";
    fs::write(&file, original).unwrap();
    assert!(!install(home.path(), "/bin/zsh", &[]).status.success());
    assert_eq!(fs::read_to_string(&file).unwrap(), original);
    fs::write(&file, "existing").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o666)).unwrap();
    assert!(!install(home.path(), "/bin/zsh", &[]).status.success());
    assert_eq!(fs::read_to_string(file).unwrap(), "existing");
}

#[test]
fn export_in_uncalled_function_does_not_skip_setup() {
    let home = tempfile::tempdir().unwrap();
    let file = home.path().join(".zshrc");
    let original = "setup_agent() {\n    export SSH_AUTH_SOCK=\"$HOME/.ssh/agent.sock\"\n}\n";
    fs::write(&file, original).unwrap();
    succeeds(install(home.path(), "/bin/zsh", &[]));
    let first = fs::read_to_string(&file).unwrap();
    assert!(first.starts_with(original));
    assert!(first.contains("# >>> ssh-key-man >>>"));
    let output = Command::new("bash")
        .args([
            "--noprofile",
            "--norc",
            "-c",
            ". \"$HOME/.zshrc\"; printf '%s' \"$SSH_AUTH_SOCK\"",
        ])
        .env("HOME", home.path())
        .env_remove("SSH_AUTH_SOCK")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        String::from_utf8(output.stdout).unwrap(),
        home.path().join(".ssh/agent.sock").to_str().unwrap()
    );
    succeeds(install(home.path(), "/bin/zsh", &[]));
    assert_eq!(fs::read_to_string(file).unwrap(), first);
}
