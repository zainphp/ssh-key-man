mod inventory;
mod operations;
#[cfg(test)]
mod tests;

pub use crate::cli::interactive::{prompt, require_terminal};
pub use inventory::inventory;
pub use operations::{delete, deploy, generate, validate_remote};

use anyhow::{bail, Context, Result};
use std::{
    fs,
    os::unix::fs::{DirBuilderExt, MetadataExt},
    path::{Path, PathBuf},
};

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

pub fn public_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".pub");
    name.into()
}
