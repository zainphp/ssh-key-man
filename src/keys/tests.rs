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
