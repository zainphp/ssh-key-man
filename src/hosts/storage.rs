use super::Config;
use crate::keys;
use anyhow::{bail, Context, Result};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    os::{
        fd::AsRawFd,
        unix::fs::{MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

impl Config {
    pub fn load(dir: &Path) -> Result<Self> {
        Ok(Self::parse(read_config(&dir.join("config"))?))
    }

    pub(super) fn commit(&self, dir: &Path, next: &str) -> Result<Option<PathBuf>> {
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
