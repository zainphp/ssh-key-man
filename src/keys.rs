use crate::process;
use anyhow::{bail, Context, Result};
use ssh_key::{HashAlg, PrivateKey, PublicKey};
use std::{
    fs,
    io::{self, IsTerminal, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
};
use walkdir::WalkDir;

#[derive(Debug, Clone)]
pub struct Key {
    pub path: PathBuf,
    pub algorithm: String,
    pub fingerprint: String,
    pub private: bool,
}

pub struct Inventory {
    pub keys: Vec<Key>,
    pub warnings: Vec<String>,
}

pub fn ssh_dir() -> Result<PathBuf> {
    dirs::home_dir()
        .map(|p| p.join(".ssh"))
        .context("Could not find your home directory. Set HOME to your home directory.")
}

pub fn ensure_dir(dir: &Path) -> Result<()> {
    if !dir.exists() {
        fs::DirBuilder::new()
            .mode(0o700)
            .create(dir)
            .with_context(|| {
                format!(
                    "Could not create {}. Check home directory permissions.",
                    dir.display()
                )
            })?;
    }
    let meta = fs::symlink_metadata(dir).with_context(|| {
        format!(
            "Could not inspect {}. Check directory permissions.",
            dir.display()
        )
    })?;
    if !meta.is_dir() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o022 != 0 {
        bail!("SSH directory {} must be a directory owned by you and not writable by other users. Check ownership and run chmod 700 on it.", dir.display());
    }
    Ok(())
}

pub fn key_path(dir: &Path, name: &str) -> Result<PathBuf> {
    let path = Path::new(name);
    if name.is_empty()
        || name.starts_with('-')
        || name.chars().any(char::is_control)
        || path.file_name() != Some(path.as_os_str())
        || name.ends_with(".pub")
        || name == "agent.sock"
        || name.starts_with(".skm-")
    {
        bail!("Invalid key filename. Use a single filename such as id_ed25519_work, without a path or .pub suffix.");
    }
    Ok(dir.join(path))
}

pub fn inventory(dir: &Path) -> Result<Inventory> {
    let mut result = Inventory {
        keys: Vec::new(),
        warnings: Vec::new(),
    };
    if !dir.exists() {
        return Ok(result);
    }
    for entry in WalkDir::new(dir)
        .max_depth(1)
        .min_depth(1)
        .follow_links(false)
        .sort_by_file_name()
    {
        let entry = entry.with_context(|| {
            format!(
                "Could not read {}. Check directory permissions.",
                dir.display()
            )
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path();
        let meta = entry.metadata()?;
        if meta.len() > 1_048_576 {
            continue;
        }
        let bytes = fs::read(path).with_context(|| {
            format!(
                "Could not read key file: {}. Check file permissions.",
                path.display()
            )
        })?;
        let Ok(text) = std::str::from_utf8(&bytes) else {
            continue;
        };
        let public = if text.starts_with("-----BEGIN OPENSSH PRIVATE KEY-----") {
            match PrivateKey::from_openssh(text) {
                Ok(key) => Some((key.public_key().clone(), true)),
                Err(e) => {
                    result.warnings.push(format!(
                        "Could not parse {}: {e}. Check the key with ssh-keygen -l -f.",
                        path.display()
                    ));
                    None
                }
            }
        } else if path.extension().is_some_and(|ext| ext == "pub") {
            match PublicKey::from_openssh(text.trim()) {
                Ok(key) => Some((key, false)),
                Err(e) => {
                    result.warnings.push(format!(
                        "Could not parse {}: {e}. Check the public key format.",
                        path.display()
                    ));
                    None
                }
            }
        } else if text.starts_with("-----BEGIN RSA PRIVATE KEY-----")
            || text.starts_with("-----BEGIN EC PRIVATE KEY-----")
            || text.starts_with("-----BEGIN PRIVATE KEY-----")
            || text.starts_with("-----BEGIN ENCRYPTED PRIVATE KEY-----")
        {
            result.warnings.push(format!(
                "Legacy PEM key found: {}. Convert it with ssh-keygen -p -o -f <filename>, then refresh the key list.",
                path.display()
            ));
            None
        } else {
            None
        };
        if let Some((public, private)) = public {
            result.keys.push(Key {
                path: path.into(),
                algorithm: public.algorithm().to_string(),
                fingerprint: public.fingerprint(HashAlg::Sha256).to_string(),
                private,
            });
        }
    }
    let private_paths: Vec<_> = result
        .keys
        .iter()
        .filter(|k| k.private)
        .map(|k| public_path(&k.path))
        .collect();
    result
        .keys
        .retain(|k| k.private || !private_paths.contains(&k.path));
    Ok(result)
}

pub fn public_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".pub");
    name.into()
}

pub fn generate(dir: &Path, name: &str, rsa: bool) -> Result<Key> {
    ensure_dir(dir)?;
    let path = key_path(dir, name)?;
    for candidate in [&path, &public_path(&path)] {
        if fs::symlink_metadata(candidate).is_ok() {
            bail!("Key file already exists: {}. Choose another filename; existing keys will not be overwritten.", candidate.display());
        }
    }
    // Generate in a private staging directory so ssh-keygen can never overwrite an existing key.
    let stage = dir.join(format!(".skm-key-{}", std::process::id()));
    fs::DirBuilder::new().mode(0o700).create(&stage).context("Could not create the key staging directory. Remove a stale .skm-key directory after checking its contents, then try again.")?;
    let result = (|| {
        let staged = stage.join("key");
        let mut command = process::command("ssh-keygen")?;
        command.args(["-t", if rsa { "rsa" } else { "ed25519" }]);
        if rsa {
            command.args(["-b", "4096"]);
        }
        command.arg("-f").arg(&staged).args(["-C", name]);
        process::interactive(&mut command, "generate the SSH key")?;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o600))?;
        let key = inventory(&stage)?
            .keys
            .into_iter()
            .find(|k| k.private)
            .context("Generated key could not be read. Check ssh-keygen output and try again.")?;
        fs::hard_link(&staged, &path).with_context(|| {
            format!(
                "Could not save {}. Choose an unused filename and check directory permissions.",
                path.display()
            )
        })?;
        if let Err(error) = fs::hard_link(public_path(&staged), public_path(&path)) {
            let _ = fs::remove_file(&path);
            return Err(error).context("Could not save the public key. Choose an unused filename and check directory permissions.");
        }
        Ok(Key { path, ..key })
    })();
    let _ = fs::remove_dir_all(stage);
    result
}

pub fn validate_remote(remote: &str) -> Result<()> {
    let Some((user, host)) = remote.split_once('@') else {
        bail!("Invalid server address. Enter user@hostname, such as alice@example.com.");
    };
    let valid_user = !user.is_empty()
        && !user.starts_with('-')
        && user
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    let valid_host = !host.is_empty()
        && !host.starts_with('-')
        && host
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || ".-:[]".contains(c));
    if !valid_user || !valid_host {
        bail!("Invalid server address. Use user@hostname without spaces or shell characters.");
    }
    Ok(())
}

pub fn deploy(key: &Key, remote: &str, port: u16) -> Result<()> {
    validate_remote(remote)?;
    if port == 0 {
        bail!("Invalid SSH port: 0. Choose a port from 1 to 65535.");
    }
    let public = public_path(&key.path);
    let text = fs::read_to_string(&public).with_context(|| {
        format!(
            "Could not read public key: {}. Check file permissions.",
            public.display()
        )
    })?;
    if text.trim().lines().count() != 1 {
        bail!("Public key file {} must contain exactly one key. Restore the matching single-line .pub file before registering it.", public.display());
    }
    let parsed = PublicKey::from_openssh(text.trim())
        .context("Could not parse the public key. Restore the matching .pub file and try again.")?;
    if parsed.fingerprint(HashAlg::Sha256).to_string() != key.fingerprint {
        bail!("The public key does not match {}. Restore its matching .pub file before registering it.", key.path.display());
    }
    // ssh-copy-id evaluates its identity filename as shell text on some OpenSSH versions.
    if public
        .as_os_str()
        .to_string_lossy()
        .chars()
        .any(|c| c.is_control() || "\"$`\\".contains(c))
    {
        bail!("The public key path contains characters ssh-copy-id cannot safely handle: {}. Move the key to a path without double quotes, dollar signs, backticks, backslashes, or control characters.", public.display());
    }
    let mut command = process::command("ssh-copy-id")?;
    command
        .arg("-i")
        .arg(public)
        .args(["-p", &port.to_string(), remote]);
    process::interactive(&mut command, "register the public key on the server")
}

pub fn require_terminal() -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("This action needs an interactive terminal. Run it directly in your terminal without piping input or output.");
    }
    Ok(())
}

pub fn prompt(label: &str, default: &str) -> Result<String> {
    print!("{label}");
    io::stdout()
        .flush()
        .context("Could not display the prompt. Check your terminal.")?;
    let mut input = String::new();
    if io::stdin().read_line(&mut input)? == 0 {
        bail!("Input ended before setup finished. Run skm gen again in a terminal.");
    }
    let input = input.trim();
    Ok(if input.is_empty() {
        default.to_owned()
    } else {
        input.to_owned()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
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
}
