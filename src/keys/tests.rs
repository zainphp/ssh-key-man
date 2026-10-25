use super::*;
use std::fs;
#[test]
fn filenames_stay_in_ssh_directory() {
    for name in [
        "",
        "..",
        "../key",
        "/tmp/key",
        "a/b",
        "a/",
        "a/.",
        "-key",
        "x.pub",
        "agent.sock",
        "a\nkey",
    ] {
        assert!(key_path(Path::new("/tmp/ssh"), name).is_err(), "{name}");
    }
    assert_eq!(
        key_path(Path::new("/tmp/ssh"), "id_work").unwrap(),
        Path::new("/tmp/ssh/id_work")
    );
}
#[test]
fn remote_rejects_options_and_shell_input() {
    for remote in [
        "host",
        "-x@host",
        "user@-host",
        "user@host;id",
        "user@host name",
        "user@$(id)",
        "user@@host",
    ] {
        assert!(validate_remote(remote).is_err(), "{remote}");
    }
    for remote in ["alice@example.com", "root@192.0.2.1", "alice@[::1]"] {
        validate_remote(remote).unwrap();
    }
}
#[test]
fn refuses_existing_keys_without_running_keygen() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("id_test"), "keep").unwrap();
    assert!(generate(dir.path(), "id_test", false).is_err());
    assert_eq!(
        fs::read_to_string(dir.path().join("id_test")).unwrap(),
        "keep"
    );
}

#[test]
fn recovery_commands_keep_shell_quoted_paths_and_custom_ports() {
    let key = Key {
        path: std::path::PathBuf::from("/home/alice's laptop/.ssh/work key"),
        algorithm: "ssh-ed25519".into(),
        fingerprint: "SHA256:fixture".into(),
        private: true,
    };
    let command = super::operations::deployment_retry(&key, "alice@example.com", 65002);
    let output = std::process::Command::new("sh")
        .args(["-c", &format!("set -- {command}; printf '%s\\0' \"$@\"")])
        .output()
        .unwrap();
    assert!(output.status.success());
    let arguments: Vec<_> = output
        .stdout
        .split(|byte| *byte == 0)
        .filter(|part| !part.is_empty())
        .map(|part| String::from_utf8_lossy(part).into_owned())
        .collect();
    assert_eq!(
        arguments,
        [
            "ssh-copy-id",
            "-i",
            "/home/alice's laptop/.ssh/work key.pub",
            "-p",
            "65002",
            "alice@example.com"
        ]
    );
    assert_eq!(super::operations::load_retry(&key), "skm load 'work key'");
}
