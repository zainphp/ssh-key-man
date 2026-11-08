use super::{shell_init, Shell};
use anyhow::{bail, Context, Result};
use std::{
    env,
    fs::{self, File, OpenOptions, Permissions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

const START: &str = "# >>> ssh-key-man >>>";
const END: &str = "# <<< ssh-key-man <<<";

enum Registration {
    Unchanged,
    Saved { backup: Option<PathBuf> },
}

pub(crate) fn install(shell: Option<Shell>) -> Result<()> {
    let shell = match shell {
        Some(shell) => shell,
        None => match env::var_os("SHELL").as_deref().and_then(|path| Path::new(path).file_name()).and_then(|name| name.to_str()) {
            Some("bash") => Shell::Bash,
            Some("zsh") => Shell::Zsh,
            Some("fish") => Shell::Fish,
            _ => bail!("Could not detect Bash, Zsh, or Fish from SHELL. Run skm init bash --install, skm init zsh --install, or skm init fish --install."),
        },
    };
    let home = dirs::home_dir()
        .context("Could not find your home folder. Set HOME and retry shell setup.")?;
    if !home.is_absolute() {
        bail!("HOME must be an absolute path. Set it to your home folder and retry shell setup.");
    }
    let paths = match shell {
        Shell::Zsh => vec![config_dir("ZDOTDIR", &home)?.join(".zshrc")],
        Shell::Fish => vec![config_dir("XDG_CONFIG_HOME", &home.join(".config"))?
            .join("fish/conf.d/ssh-key-man.fish")],
        Shell::Bash => {
            let profile = [".bash_profile", ".bash_login", ".profile"]
                .into_iter()
                .map(|name| home.join(name))
                .find(|path| path.symlink_metadata().is_ok())
                .unwrap_or_else(|| home.join(".bash_profile"));
            vec![home.join(".bashrc"), profile]
        }
    };
    let statement = shell_init(shell);
    // shortcut: startup probes rely on ssh-add returning; use a bounded probe if stalled agents occur.
    let condition = match shell {
        Shell::Bash | Shell::Zsh => format!("if [ -z \"${{SSH_AUTH_SOCK:-}}\" ] || [ ! -S \"$SSH_AUTH_SOCK\" ] || ! (ssh-add -l >/dev/null 2>&1; test \"$?\" -le 1); then\n    {statement}\nfi"),
        Shell::Fish => format!("if not set -q SSH_AUTH_SOCK; or not test -S \"$SSH_AUTH_SOCK\"; or not sh -c 'ssh-add -l >/dev/null 2>&1; test \"$?\" -le 1'\n    {statement}\nend"),
    };
    let block = format!("{START}\n{condition}\n{END}");
    for path in paths {
        let registration = register_file(&path, &block).with_context(|| {
            format!("Could not finish shell setup for {}. Check file permissions and free disk space, then retry skm init --install.", path.display())
        })?;
        match registration {
            Registration::Unchanged => {
                println!("SSH agent settings already exist in {}.", path.display());
            }
            Registration::Saved { backup } => {
                println!("Added SSH agent settings to {}.", path.display());
                if let Some(backup) = backup {
                    println!("Backup: {}", backup.display());
                }
            }
        }
    }
    println!("Open a new terminal to use these settings. For this terminal, run:");
    println!(
        "{}",
        match shell {
            Shell::Fish => "skm init fish | source",
            Shell::Bash | Shell::Zsh => "eval \"$(skm init)\"",
        }
    );
    Ok(())
}

fn config_dir(variable: &str, fallback: &Path) -> Result<PathBuf> {
    let path = env::var_os(variable)
        .map(PathBuf::from)
        .unwrap_or_else(|| fallback.to_path_buf());
    if !path.is_absolute() {
        bail!("{variable} must be an absolute path. Fix it or unset it, then retry shell setup.");
    }
    Ok(path)
}

fn read_startup(path: &Path) -> Result<Option<(String, u32)>> {
    let mut file = match OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Could not read {}. Check file permissions and retry shell setup.",
                    path.display()
                )
            })
        }
    };
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != unsafe { libc::geteuid() }
        || metadata.mode() & 0o022 != 0
    {
        bail!("Could not update {} safely. Use a regular file owned by you and not writable by other users.", path.display());
    }
    let mut text = String::new();
    file.read_to_string(&mut text).with_context(|| {
        format!(
            "Could not read {} as UTF-8 text. Edit this startup file manually or fix its encoding.",
            path.display()
        )
    })?;
    Ok(Some((text, metadata.mode() & 0o777)))
}

fn register_file(path: &Path, block: &str) -> Result<Registration> {
    let target = match path.symlink_metadata() {
        Ok(metadata) if metadata.is_symlink() => fs::canonicalize(path).with_context(|| {
            format!(
                "Could not resolve startup link {}. Fix its target and retry shell setup.",
                path.display()
            )
        })?,
        Ok(_) => path.to_path_buf(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => path.to_path_buf(),
        Err(error) => {
            return Err(error).with_context(|| {
                format!(
                    "Could not inspect {}. Check permissions and retry shell setup.",
                    path.display()
                )
            })
        }
    };
    let source = read_startup(&target)?;
    let text = source.as_ref().map_or("", |(text, _)| text.as_str());
    if text.contains(block) {
        return Ok(Registration::Unchanged);
    }
    if text.contains(START) || text.contains(END) {
        bail!("The ssh-key-man setup block in {} is incomplete or was edited. Repair or remove that block, then retry skm init --install.", path.display());
    }
    let parent = target
        .parent()
        .context("Could not find the startup file folder. Check your shell settings.")?;
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "Could not create {}. Check folder permissions.",
            parent.display()
        )
    })?;
    let backup = if source.is_some() {
        let prefix = format!(
            "{}.skm-backup-",
            target.file_name().unwrap_or_default().to_string_lossy()
        );
        let mut backup = tempfile::Builder::new()
            .prefix(&prefix)
            .tempfile_in(parent)
            .with_context(|| {
                format!(
                    "Could not back up {}. Check folder permissions and free disk space.",
                    path.display()
                )
            })?;
        backup.write_all(text.as_bytes())?;
        backup.as_file().sync_all()?;
        Some(
            backup
                .keep()
                .context("Could not keep the shell startup backup. Check folder permissions.")?
                .1,
        )
    } else {
        None
    };
    let mut next = text.to_owned();
    if !next.is_empty() {
        if !next.ends_with('\n') {
            next.push('\n');
        }
        next.push('\n');
    }
    next.push_str(block);
    next.push('\n');
    let mut temporary = tempfile::Builder::new()
        .prefix(".skm-shell-")
        .tempfile_in(parent)
        .context(
            "Could not prepare shell settings. Check folder permissions and free disk space.",
        )?;
    temporary.as_file().set_permissions(Permissions::from_mode(
        source.as_ref().map_or(0o600, |(_, mode)| *mode),
    ))?;
    temporary.write_all(next.as_bytes()).context(
        "Could not write shell settings. Check free disk space; the original file was kept.",
    )?;
    temporary.as_file().sync_all()?;
    if read_startup(&target)? != source {
        bail!(
            "{} changed during shell setup. Retry skm init --install so those edits are preserved.",
            path.display()
        );
    }
    let result = if source.is_some() {
        temporary.persist(&target)
    } else {
        temporary.persist_noclobber(&target)
    };
    result.with_context(|| {
        format!(
            "Could not save {}. Check file permissions; the original startup file was kept.",
            path.display()
        )
    })?;
    File::open(parent).and_then(|file| file.sync_all()).with_context(|| format!("Shell settings were saved in {}, but the folder could not be synced. Check disk status and keep the backup.", path.display()))?;
    Ok(Registration::Saved { backup })
}
