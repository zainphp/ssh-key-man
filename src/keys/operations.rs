use super::{ensure_dir, inventory, key_path, public_path, Key};
use crate::process;
use anyhow::{bail, Context, Result};
use ssh_key::{HashAlg, PrivateKey, PublicKey};
use std::{
    fs::{self, OpenOptions},
    io::{self, Read},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::Path,
};

pub fn delete(dir: &Path, key: &Key) -> Result<()> {
    ensure_dir(dir)?;
    if key.path.parent() != Some(dir) {
        bail!(
            "Could not delete a key outside {}. Select a key from this folder.",
            dir.display()
        );
    }
    let text = read_owned_key(&key.path)?;
    let public = if key.private {
        PrivateKey::from_openssh(&text).context("Could not read the private key. The file was kept; check its format before deleting it.")?.public_key().clone()
    } else {
        if text.trim().lines().count() != 1 {
            bail!("Public key file {} contains more than one line. The file was kept; check its contents before deleting it manually.", key.path.display());
        }
        PublicKey::from_openssh(text.trim()).context("Could not read the public key. The file was kept; check its format before deleting it.")?
    };
    if public.fingerprint(HashAlg::Sha256).to_string() != key.fingerprint {
        bail!(
            "Key file {} changed since it was selected. Reopen the Keys page before deleting it.",
            key.path.display()
        );
    }
    let companion = public_path(&key.path);
    let has_companion = key.private
        && match fs::symlink_metadata(&companion) {
            Ok(_) => {
                let text = read_owned_key(&companion)?;
                if text.trim().lines().count() != 1
                    || PublicKey::from_openssh(text.trim())
                        .map(|p| p.fingerprint(HashAlg::Sha256).to_string())
                        .ok()
                        .as_deref()
                        != Some(&key.fingerprint)
                {
                    bail!("Public key {} does not match the selected private key. Both files were kept. Check the .pub file before deleting this pair.", companion.display());
                }
                true
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => false,
            Err(e) => {
                return Err(e).with_context(|| {
                    format!(
                        "Could not inspect {}. The key files were kept; check file permissions.",
                        companion.display()
                    )
                })
            }
        };
    if key.private {
        crate::agent::unload(dir, &public)?;
    }
    fs::remove_file(&key.path).with_context(|| format!("Could not delete {}. Check folder permissions. If the key was unloaded, load it again with {} if needed.", key.path.display(), load_retry(key)))?;
    if has_companion {
        fs::remove_file(&companion).with_context(|| format!("The private key was deleted, but {} could not be removed. Check folder permissions and remove this public key file manually.", companion.display()))?;
    }
    Ok(())
}

fn read_owned_key(path: &Path) -> Result<String> {
    let mut file = OpenOptions::new().read(true).custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK).open(path)
        .with_context(|| format!("Could not read {}. The file was kept. Check permissions; linked files must be deleted manually.", path.display()))?;
    let meta = file.metadata()?;
    if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.len() > 1_048_576 {
        bail!("Could not delete {} safely. Select a regular key file owned by you and smaller than 1 MiB.", path.display());
    }
    let mut text = String::new();
    file.read_to_string(&mut text).with_context(|| {
        format!(
            "Could not read {}. The file was kept; check its format and permissions.",
            path.display()
        )
    })?;
    Ok(text)
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
    fs::DirBuilder::new().mode(0o700).create(&stage).with_context(|| format!("Could not create a temporary folder for the new key: {}. Check ~/.ssh permissions. If this folder already exists, check its contents before moving it aside.", stage.display()))?;
    let result = (|| {
        let staged = stage.join("key");
        let mut command = process::command("ssh-keygen")?;
        command.args(["-t", if rsa { "rsa" } else { "ed25519" }]);
        if rsa {
            command.args(["-b", "4096"]);
        }
        command.arg("-f").arg(&staged).args(["-C", name]);
        process::interactive(&mut command, "create the SSH key")?;
        fs::set_permissions(&staged, fs::Permissions::from_mode(0o600))?;
        let key = inventory(&stage)?
            .keys
            .into_iter()
            .find(|k| k.private)
            .context("The new key could not be read. Check ssh-keygen output and try again.")?;
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
        bail!("Public key file {} must contain exactly one key. Restore the matching single-line .pub file before adding it to a server.", public.display());
    }
    let parsed = PublicKey::from_openssh(text.trim())
        .context("Could not parse the public key. Restore the matching .pub file and try again.")?;
    if parsed.fingerprint(HashAlg::Sha256).to_string() != key.fingerprint {
        bail!("The public key does not match {}. Restore its matching .pub file before adding it to a server.", key.path.display());
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
    (|| {
        let mut command = process::command("ssh-copy-id")?;
        command
            .arg("-i")
            .arg(public)
            .args(["-p", &port.to_string(), remote]);
        process::interactive(&mut command, "add the public key to the server")
    })()
    .with_context(|| {
        format!(
            "Remote setup did not finish. Retry with {}.",
            deployment_retry(key, remote, port)
        )
    })
}

pub(crate) fn deployment_retry(key: &Key, remote: &str, port: u16) -> String {
    format!(
        "ssh-copy-id -i {} -p {port} {}",
        process::shell_quote(&public_path(&key.path).to_string_lossy()),
        process::shell_quote(remote)
    )
}

pub(crate) fn load_retry(key: &Key) -> String {
    format!(
        "skm load {}",
        process::shell_quote(&key.path.file_name().unwrap_or_default().to_string_lossy())
    )
}
