use crate::keys;
use anyhow::{bail, Context, Result};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    ops::Range,
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

const FIELDS: [&str; 4] = ["hostname", "user", "port", "identityfile"];

pub fn validate_alias(alias: &str) -> Result<()> {
    if alias.is_empty()
        || alias.starts_with('-')
        || !alias
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c))
    {
        bail!("Invalid alias. Select or enter a single alias using letters, numbers, dots, underscores, or hyphens, such as work-server.");
    }
    Ok(())
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Host {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: String,
    pub identity_file: String,
}

impl Host {
    fn values(&self) -> [&str; 4] {
        [&self.hostname, &self.user, &self.port, &self.identity_file]
    }

    pub fn validate(&self) -> Result<()> {
        validate_alias(&self.alias)?;
        if self.hostname.is_empty()
            || self.hostname.starts_with('-')
            || !self
                .hostname
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || ".-_:[]%".contains(c))
        {
            bail!("Invalid server address. Enter a hostname or IP address without a username or port.");
        }
        if !self.user.is_empty()
            && (self.user.starts_with('-')
                || !self
                    .user
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c)))
        {
            bail!("Invalid username. Enter a login name such as alice, without spaces or @.");
        }
        if !self.port.is_empty() && self.port.parse::<u16>().ok().filter(|p| *p > 0).is_none() {
            bail!("Invalid SSH port. Enter a number from 1 to 65535, or leave it blank to use your SSH settings.");
        }
        if self
            .identity_file
            .chars()
            .any(|c| c.is_control() || "\"\\".contains(c))
        {
            bail!("Invalid key path. Use a path without double quotes, backslashes, or control characters.");
        }
        Ok(())
    }
}

#[derive(Debug)]
pub struct Entry {
    pub host: Host,
    pub read_only: bool,
    range: Range<usize>,
}

pub struct Config {
    source: Option<String>,
    pub entries: Vec<Entry>,
}

// Keep the source untouched; only tokenize directives needed to locate and edit host blocks.
fn directive(line: &str) -> Option<(String, Vec<String>)> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let end = line
        .find(|c: char| c.is_whitespace() || c == '=')
        .unwrap_or(line.len());
    let key = line[..end].to_ascii_lowercase();
    let rest = line[end..]
        .trim_start()
        .strip_prefix('=')
        .unwrap_or(line[end..].trim_start())
        .trim_start();
    let mut quoted = false;
    let mut value = String::new();
    let mut values = Vec::new();
    for c in rest.chars() {
        match c {
            '"' => quoted = !quoted,
            '#' if !quoted => break,
            c if c.is_whitespace() && !quoted => {
                if !value.is_empty() {
                    values.push(std::mem::take(&mut value));
                }
            }
            c => value.push(c),
        }
    }
    if quoted {
        return Some((key, Vec::new()));
    }
    if !value.is_empty() {
        values.push(value);
    }
    Some((key, values))
}

impl Config {
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(Self::parse(read_config(&dir.join("config"))?))
    }

    fn parse(source: Option<String>) -> Self {
        let text = source.as_deref().unwrap_or_default();
        let mut boundaries = Vec::new();
        let mut offset = 0;
        for line in text.split_inclusive('\n') {
            if let Some((key, values)) = directive(line) {
                if key == "host" || key == "match" {
                    boundaries.push((offset, key, values));
                }
            }
            offset += line.len();
        }
        let mut entries = Vec::new();
        for (i, (start, key, aliases)) in boundaries.iter().enumerate() {
            if key != "host" {
                continue;
            }
            let end = boundaries.get(i + 1).map(|b| b.0).unwrap_or(text.len());
            let mut values = [String::new(), String::new(), String::new(), String::new()];
            let mut seen = [false; 4];
            let mut read_only =
                aliases.len() != 1 || aliases.iter().any(|a| a.chars().any(|c| "*?!".contains(c)));
            for line in text[*start..end].lines().skip(1) {
                if let Some((key, args)) = directive(line) {
                    if let Some(field) = FIELDS.iter().position(|f| *f == key) {
                        if seen[field] || args.len() != 1 {
                            read_only = true;
                        }
                        if !seen[field] {
                            values[field] = args.first().cloned().unwrap_or_default();
                        }
                        seen[field] = true;
                    }
                }
            }
            let [hostname, user, port, identity_file] = values;
            entries.push(Entry {
                host: Host {
                    alias: aliases.join(" "),
                    hostname,
                    user,
                    port,
                    identity_file,
                },
                read_only,
                range: *start..end,
            });
        }
        Self { source, entries }
    }

    pub fn save(&self, dir: &Path, index: Option<usize>, host: &Host) -> Result<Option<PathBuf>> {
        host.validate()?;
        if self.entries.iter().enumerate().any(|(i, entry)| {
            Some(i) != index
                && entry
                    .host
                    .alias
                    .split_whitespace()
                    .any(|alias| alias.eq_ignore_ascii_case(&host.alias))
        }) {
            bail!("Alias {} already exists in ~/.ssh/config. Edit that entry or choose another alias.", host.alias);
        }
        let source = self.source.as_deref().unwrap_or_default();
        let eol = if source.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };
        let mut block = format!("Host {}{eol}", host.alias);
        for ((name, value), label) in
            FIELDS
                .iter()
                .zip(host.values())
                .zip(["HostName", "User", "Port", "IdentityFile"])
        {
            if !value.is_empty() {
                let value = if *name == "identityfile" {
                    format!("\"{value}\"")
                } else {
                    value.to_owned()
                };
                block.push_str(&format!("    {label} {value}{eol}"));
            }
        }
        let next = if let Some(index) = index {
            let entry = self.editable(index)?;
            // Preserve extra options and comments, including comments on replaced directives.
            for line in source[entry.range.clone()].split_inclusive('\n') {
                match directive(line) {
                    Some((key, _)) if key == "host" || FIELDS.contains(&key.as_str()) => {
                        if let Some(comment) = comment(line) {
                            block.push_str(&format!("    {comment}{eol}"));
                        }
                    }
                    _ => block.push_str(line),
                }
            }
            format!(
                "{}{}{}",
                &source[..entry.range.start],
                block,
                &source[entry.range.end..]
            )
        } else {
            // Reset scope before original global directives so they still apply to every other host.
            let needs_reset = source
                .lines()
                .filter_map(directive)
                .next()
                .is_some_and(|(key, _)| key != "host" && key != "match");
            format!(
                "{block}{eol}{}{source}",
                if needs_reset {
                    format!("Host *{eol}")
                } else {
                    String::new()
                }
            )
        };
        self.commit(dir, &next)
    }

    pub fn delete(&self, dir: &Path, index: usize) -> Result<Option<PathBuf>> {
        let entry = self.editable(index)?;
        let source = self.source.as_deref().unwrap_or_default();
        let mut kept = String::new();
        for line in source[entry.range.clone()].split_inclusive('\n') {
            if line.trim().starts_with('#') || line.trim().is_empty() {
                kept.push_str(line);
            }
        }
        self.commit(
            dir,
            &format!(
                "{}{}{}",
                &source[..entry.range.start],
                kept,
                &source[entry.range.end..]
            ),
        )
    }

    fn editable(&self, index: usize) -> Result<&Entry> {
        let entry = self
            .entries
            .get(index)
            .context("The selected host no longer exists. Reopen Hosts and select it again.")?;
        if entry.read_only {
            bail!("This entry matches multiple hosts or has repeated settings. Edit it directly in ~/.ssh/config to avoid changing other connections.");
        }
        Ok(entry)
    }

    fn commit(&self, dir: &Path, next: &str) -> Result<Option<PathBuf>> {
        keys::ensure_dir(dir)?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(dir.join(".skm-config.lock"))
            .context(
                "Could not prepare to save SSH config. Check permissions for ~/.ssh/.skm-config.lock.",
            )?;
        let meta = lock.metadata()?;
        if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
            bail!("Could not save SSH config safely. Make sure ~/.ssh/.skm-config.lock is owned by you and has permissions 600.");
        }
        if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            bail!("Another skm process is editing SSH config. Wait for it to finish, then reopen Hosts.");
        }
        let path = dir.join("config");
        if read_config(&path)? != self.source {
            bail!("SSH config changed since you opened Hosts. Reopen Hosts before saving so those changes are preserved.");
        }
        let backup = if let Some(source) = &self.source {
            let mut file = tempfile::Builder::new().prefix("config.skm-backup-").tempfile_in(dir)
                .context("Could not create an SSH config backup. Check ~/.ssh permissions and free disk space.")?;
            file.write_all(source.as_bytes())
                .context("Could not write the SSH config backup. Check free disk space.")?;
            file.as_file().sync_all().context("Could not save SSH config data to disk. Check free disk space; the original config was preserved.")?;
            Some(
                file.keep()
                    .context("Could not keep the SSH config backup. Check ~/.ssh permissions.")?
                    .1,
            )
        } else {
            None
        };
        let mut file = tempfile::Builder::new()
            .prefix(".skm-config-")
            .tempfile_in(dir)
            .context("Could not create a temporary SSH config. Check ~/.ssh permissions.")?;
        file.write_all(next.as_bytes()).context(
            "Could not write SSH config. Check free disk space; the original file was preserved.",
        )?;
        file.as_file().sync_all().context("Could not save SSH config data to disk. Check free disk space; the original config was preserved.")?;
        if read_config(&path)? != self.source {
            bail!("SSH config changed while saving. Reopen Hosts before saving again.");
        }
        file.persist(&path).context("Could not replace SSH config. Check ~/.ssh permissions; the original file was preserved.")?;
        File::open(dir).and_then(|file| file.sync_all()).context("SSH config was saved, but the directory could not be synced. Check disk status and keep the backup.")?;
        Ok(backup)
    }
}

fn comment(line: &str) -> Option<&str> {
    let mut quoted = false;
    for (index, c) in line.char_indices() {
        if c == '"' {
            quoted = !quoted;
        }
        if c == '#' && !quoted {
            return Some(line[index..].trim_end());
        }
    }
    None
}

fn read_config(path: &Path) -> Result<Option<String>> {
    let mut file = match OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path) {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e).with_context(|| format!("Could not read SSH config: {}. Check permissions; symlinked config files must be edited directly.", path.display())),
    };
    let meta = file.metadata()?;
    if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o022 != 0 {
        bail!("SSH config {} must be a regular file owned by you and not writable by others. Check ownership and run chmod 600 on it.", path.display());
    }
    if meta.len() > 1_048_576 {
        bail!("SSH config is larger than 1 MiB. Edit it directly or split it into Include files before using Hosts.");
    }
    let mut text = String::new();
    file.read_to_string(&mut text).with_context(|| {
        format!(
            "Could not read SSH config: {}. Use a UTF-8 text file and check permissions.",
            path.display()
        )
    })?;
    Ok(Some(text))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
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
        assert!(
            updated.contains("    # Office\n    # address\n  ProxyJump gateway\n  # Keep notes\n")
        );
        assert!(updated
            .ends_with("Match host other\n  User somebody\nHost *\n  ServerAliveInterval 60\n"));
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
}
