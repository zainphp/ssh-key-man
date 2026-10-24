use super::*;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::os::unix::fs::{symlink, PermissionsExt};

fn host(alias: &str) -> Host {
    Host {
        alias: alias.into(),
        hostname: "server.example.invalid".into(),
        user: "alice".into(),
        port: "2222".into(),
        identity_file: "~/.ssh/work key".into(),
    }
}

fn fixture(text: &str) -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("config"), text).unwrap();
    dir
}

#[test]
fn parses_equals_quotes_case_and_match_boundaries() {
    let source = "# Keep this\r\nhOsT = \"work\" # Office\r\n HOSTNAME=example.invalid\r\n User = alice\r\n IdentityFile \"~/.ssh/key with # space\"\r\nMatch host other\r\n User bob\r\nHost *.internal jump\r\n Port 22\r\nHost duplicate\r\n IdentityFile one\r\n IdentityFile two\r\n";
    let config = Config::parse(Some(source.into()));
    assert_eq!(config.entries.len(), 3);
    assert_eq!(config.entries[0].host.alias, "work");
    assert_eq!(config.entries[0].host.user, "alice");
    assert_eq!(
        config.entries[0].host.identity_file,
        "~/.ssh/key with # space"
    );
    assert!(!config.entries[0].read_only);
    assert!(config.entries[1].read_only);
    assert!(config.entries[2].read_only);
}

#[test]
fn edits_only_selected_block_preserving_extras_and_backup() {
    let original = "# First\nHost work # Office\n  HostName old.invalid # address\n  User old\n  Port 22\n  IdentityFile \"~/.ssh/old key\"\n  ProxyJump gateway\n  # Keep notes\n\nMatch host other\n  User somebody\nHost *\n  ServerAliveInterval 60\n";
    let dir = fixture(original);
    let config = Config::load(dir.path()).unwrap();
    let backup = config
        .save(dir.path(), Some(0), &host("renamed"))
        .unwrap()
        .unwrap();
    assert_eq!(fs::read_to_string(backup).unwrap(), original);
    let updated = fs::read_to_string(dir.path().join("config")).unwrap();
    assert!(updated.starts_with("# First\nHost renamed\n"));
    assert!(updated.contains("    # Office\n    # address\n  ProxyJump gateway\n  # Keep notes\n"));
    assert!(
        updated.ends_with("Match host other\n  User somebody\nHost *\n  ServerAliveInterval 60\n")
    );
    assert_eq!(
        Config::load(dir.path()).unwrap().entries[0].host,
        host("renamed")
    );
    assert_eq!(
        fs::metadata(dir.path().join("config")).unwrap().mode() & 0o777,
        0o600
    );
}

#[test]
fn deleting_preserves_comments_and_other_scopes() {
    let dir = fixture("Host work\n HostName old.invalid\n # Keep this note\n\nHost other\n User bob\nMatch all\n Port 2200\n");
    Config::load(dir.path())
        .unwrap()
        .delete(dir.path(), 0)
        .unwrap();
    assert_eq!(
        fs::read_to_string(dir.path().join("config")).unwrap(),
        " # Keep this note\n\nHost other\n User bob\nMatch all\n Port 2200\n"
    );
}

#[test]
fn refuses_duplicate_aliases_and_shared_blocks_without_writing() {
    let original = "Host work shared\n Port 22\nHost *\n User somebody\n";
    let dir = fixture(original);
    let config = Config::load(dir.path()).unwrap();
    assert!(config.save(dir.path(), None, &host("WORK")).is_err());
    assert!(config.save(dir.path(), Some(0), &host("changed")).is_err());
    assert!(config.delete(dir.path(), 1).is_err());
    assert_eq!(
        fs::read_to_string(dir.path().join("config")).unwrap(),
        original
    );
}

#[test]
fn stale_snapshot_cannot_overwrite_external_edits() {
    let dir = fixture("Host work\n User alice\n");
    let config = Config::load(dir.path()).unwrap();
    fs::write(
        dir.path().join("config"),
        "# Changed elsewhere\nHost work\n User bob\n",
    )
    .unwrap();
    let error = config.save(dir.path(), Some(0), &host("work")).unwrap_err();
    assert!(error.to_string().contains("changed since"));
    assert!(fs::read_to_string(dir.path().join("config"))
        .unwrap()
        .contains("User bob"));
}

#[test]
fn refuses_symlinks_and_unsafe_permissions() {
    let dir = tempfile::tempdir().unwrap();
    let target = dir.path().join("target");
    fs::write(&target, "# Keep\n").unwrap();
    symlink(&target, dir.path().join("config")).unwrap();
    assert!(Config::load(dir.path()).is_err());
    assert!(Config::parse(None)
        .save(dir.path(), None, &host("work"))
        .is_err());
    assert_eq!(fs::read_to_string(target).unwrap(), "# Keep\n");
    fs::remove_file(dir.path().join("config")).unwrap();
    fs::write(dir.path().join("config"), "# Keep\n").unwrap();
    fs::set_permissions(dir.path().join("config"), fs::Permissions::from_mode(0o666)).unwrap();
    assert!(Config::load(dir.path()).is_err());
}

#[test]
fn creates_config_without_requiring_existing_keys() {
    let dir = tempfile::tempdir().unwrap();
    let ssh = dir.path().join(".ssh");
    assert!(Config::load(&ssh)
        .unwrap()
        .save(&ssh, None, &host("work"))
        .unwrap()
        .is_none());
    assert_eq!(Config::load(&ssh).unwrap().entries[0].host, host("work"));
}

#[test]
fn blocks_directive_injection_and_invalid_values() {
    for (field, value) in [
        (0, "work\nHost *"),
        (0, "*.example"),
        (1, "user@host"),
        (1, "host\nUser root"),
        (2, "user name"),
        (3, "0"),
        (3, "65536"),
        (4, "key\"\nProxyCommand bad"),
    ] {
        let mut host = host("work");
        match field {
            0 => host.alias = value.into(),
            1 => host.hostname = value.into(),
            2 => host.user = value.into(),
            3 => host.port = value.into(),
            _ => host.identity_file = value.into(),
        }
        assert!(host.validate().is_err(), "{value}");
    }
}

#[test]
fn openssh_resolves_new_alias_and_preserves_global_defaults_for_others() {
    let original = "# Global defaults\nPort 2200\nUser default-user\nHost old\n HostName old.example.invalid\nHost *\n ServerAliveInterval 15\n";
    let dir = fixture(original);
    let before = std::process::Command::new("ssh")
        .args(["-G", "-F"])
        .arg(dir.path().join("config"))
        .arg("old")
        .output()
        .unwrap();
    Config::load(dir.path())
        .unwrap()
        .save(dir.path(), None, &host("work"))
        .unwrap();
    let run = |alias| {
        std::process::Command::new("ssh")
            .args(["-G", "-F"])
            .arg(dir.path().join("config"))
            .arg(alias)
            .output()
            .unwrap()
    };
    let old = run("old");
    assert!(old.status.success());
    assert_eq!(old.stdout, before.stdout);
    let work = run("work");
    assert!(
        work.status.success(),
        "{}",
        String::from_utf8_lossy(&work.stderr)
    );
    let settings = String::from_utf8(work.stdout).unwrap();
    for value in [
        "hostname server.example.invalid\n",
        "user alice\n",
        "port 2222\n",
        "identityfile ~/.ssh/work key\n",
    ] {
        assert!(settings.contains(value), "{settings}");
    }
}

#[test]
fn includes_are_preserved_without_reading_or_running_them() {
    let source = "Include ~/.ssh/extra.conf\nHost work\n HostName old.invalid\nMatch exec \"never-run-this\"\n User other\n";
    let dir = fixture(source);
    Config::load(dir.path())
        .unwrap()
        .save(dir.path(), Some(0), &host("work"))
        .unwrap();
    let updated = fs::read_to_string(dir.path().join("config")).unwrap();
    assert!(updated.starts_with("Include ~/.ssh/extra.conf\n"));
    assert!(updated.ends_with("Match exec \"never-run-this\"\n User other\n"));
}
