use crate::{
    keys::{self, Key},
    process,
};
use anyhow::{bail, Context, Result};
use std::{
    collections::HashSet,
    env,
    fs::{self, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{symlink, FileTypeExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
    thread,
    time::{Duration, Instant},
};
use walkdir::WalkDir;

pub fn unload(dir: &Path, public: &ssh_key::PublicKey) -> Result<()> {
    use std::io::Write;
    let Some(agent) = current(dir)? else {
        return Ok(());
    };
    let fingerprint = public.fingerprint(ssh_key::HashAlg::Sha256).to_string();
    if !agent.fingerprints.contains(&fingerprint) {
        return Ok(());
    }
    let mut file = tempfile::Builder::new()
        .prefix(".skm-public-")
        .suffix(".pub")
        .tempfile_in(dir)
        .context(
            "Could not prepare to unload the key. Check ~/.ssh permissions and free disk space.",
        )?;
    let mut identity = public.clone();
    identity.set_comment("");
    file.write_all(identity.to_openssh()?.as_bytes())?;
    let output = process::capture(
        process::command("ssh-add")?
            .env("SSH_AUTH_SOCK", &agent.socket)
            .arg("-d")
            .arg(file.path()),
    )?;
    if !output.status.success() {
        bail!("Could not unload the key from the SSH agent: {}. The key files were kept. Run ssh-add -d <keyfile> and try again.", String::from_utf8_lossy(&output.stderr).trim());
    }
    if probe(&agent.socket)?.fingerprints.contains(&fingerprint) {
        bail!("The key is still loaded in the SSH agent. The key files were kept. Run ssh-add -d <keyfile> before deleting them.");
    }
    Ok(())
}

#[derive(Debug)]
pub struct Agent {
    pub socket: PathBuf,
    pub fingerprints: HashSet<String>,
}

pub fn probe(socket: &Path) -> Result<Agent> {
    let meta = fs::metadata(socket).with_context(|| {
        format!(
            "Could not check the SSH agent connection at {}. Run skm doctor to find or start an agent.",
            socket.display()
        )
    })?;
    if !meta.file_type().is_socket() || meta.uid() != unsafe { libc::geteuid() } {
        bail!(
            "Could not use the SSH agent connection at {}: it is not a socket owned by you. Run skm doctor to choose a safe agent.",
            socket.display()
        );
    }
    let output = process::capture(
        process::command("ssh-add")?
            .env("SSH_AUTH_SOCK", socket)
            .args(["-l", "-E", "sha256"]),
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success()
        && !(output.status.code() == Some(1) && stdout.trim() == "The agent has no identities.")
    {
        bail!(
            "SSH agent at {} did not respond: {}. Run skm doctor to find or start an agent.",
            socket.display(),
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    let fingerprints = stdout
        .lines()
        .filter_map(|line| line.split_whitespace().nth(1))
        .filter(|value| value.starts_with("SHA256:"))
        .map(str::to_owned)
        .collect();
    Ok(Agent {
        socket: socket.to_path_buf(),
        fingerprints,
    })
}

pub fn current(dir: &Path) -> Result<Option<Agent>> {
    process::command("ssh-add")?;
    Ok(env::var_os("SSH_AUTH_SOCK")
        .and_then(|s| probe(Path::new(&s)).ok())
        .or_else(|| probe(&dir.join("agent.sock")).ok()))
}

pub fn ensure(dir: &Path) -> Result<Agent> {
    keys::ensure_dir(dir)?;
    let lock = OpenOptions::new().read(true).write(true).create(true).truncate(false)
        .mode(0o600).custom_flags(libc::O_NOFOLLOW).open(dir.join(".skm-agent.lock"))
        .context("Could not prepare to manage the SSH agent. Check ownership and permissions for ~/.ssh/.skm-agent.lock.")?;
    let meta = lock.metadata()?;
    if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        bail!("Could not manage the SSH agent safely. Make sure ~/.ssh/.skm-agent.lock is owned by you and has permissions 600.");
    }
    let start = Instant::now();
    while unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        if start.elapsed() > Duration::from_secs(10) {
            bail!(
                "Another skm process is managing the agent. Wait for it to finish and try again."
            );
        }
        thread::sleep(Duration::from_millis(50));
    }
    let stable = dir.join("agent.sock");
    check_link_slot(&stable)?;
    if let Some(agent) = current(dir)? {
        update_link(&stable, &agent.socket)?;
        return Ok(agent);
    }
    let mut candidates = Vec::new();
    for root in [Path::new("/tmp").to_path_buf(), env::temp_dir()] {
        for entry in WalkDir::new(root)
            .max_depth(2)
            .min_depth(2)
            .follow_links(false)
            .into_iter()
            .filter_map(Result::ok)
        {
            if entry.file_name().to_string_lossy().starts_with("agent.")
                && entry
                    .path()
                    .parent()
                    .and_then(Path::file_name)
                    .is_some_and(|s| s.to_string_lossy().starts_with("ssh-"))
            {
                candidates.push(entry.into_path());
            }
        }
    }
    candidates.sort();
    candidates.dedup();
    for socket in candidates {
        if let Ok(agent) = probe(&socket) {
            update_link(&stable, &socket)?;
            return Ok(agent);
        }
    }
    let output = process::capture(process::command("ssh-agent")?.arg("-s"))?;
    if !output.status.success() {
        bail!("Could not start ssh-agent: {}. Check OpenSSH installation and temporary directory permissions.", String::from_utf8_lossy(&output.stderr).trim());
    }
    let stdout = String::from_utf8_lossy(&output.stdout);
    let socket = stdout.lines().find_map(|line| line.strip_prefix("SSH_AUTH_SOCK=").and_then(|s| s.split(';').next()))
        .context("Could not find the connection address for the new SSH agent. Check your OpenSSH installation and run skm doctor again.")?;
    let agent = probe(Path::new(socket))?;
    update_link(&stable, &agent.socket)?;
    Ok(agent)
}

fn check_link_slot(stable: &Path) -> Result<()> {
    match fs::symlink_metadata(stable) {
        Ok(meta) if !meta.file_type().is_symlink() => bail!("{} already exists and is not a symlink. Move it to a safe location before running skm doctor.", stable.display()),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e).with_context(|| format!("Could not inspect {}. Check directory permissions.", stable.display())),
    }
}

fn update_link(stable: &Path, socket: &Path) -> Result<()> {
    let target = fs::canonicalize(socket)
        .context("Could not find the SSH agent connection. Run skm doctor again.")?;
    check_link_slot(stable)?;
    let temporary = stable.with_file_name(format!(".skm-agent-link-{}", std::process::id()));
    symlink(&target, &temporary).with_context(|| format!("Could not set up the SSH agent connection. Check ~/.ssh permissions. If {} already exists, remove that temporary link and try again.", temporary.display()))?;
    let result = fs::rename(&temporary, stable)
        .context("Could not update ~/.ssh/agent.sock. Check directory permissions.");
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

pub fn load(agent: &mut Agent, key: &Key) -> Result<bool> {
    if !key.private {
        bail!(
            "{} is a public key only. Select the matching private key to load it.",
            key.path.display()
        );
    }
    let meta = fs::symlink_metadata(&key.path).with_context(|| {
        format!(
            "Could not read key file: {}. Check file permissions.",
            key.path.display()
        )
    })?;
    if !meta.is_file() || meta.uid() != unsafe { libc::geteuid() } || meta.mode() & 0o077 != 0 {
        bail!("Private key {} must be a regular file owned by you with permissions 600. Fix its ownership and permissions before loading it.", key.path.display());
    }
    *agent = probe(&agent.socket)?;
    if agent.fingerprints.contains(&key.fingerprint) {
        return Ok(false);
    }
    let mut command = process::command("ssh-add")?;
    command.env("SSH_AUTH_SOCK", &agent.socket).arg(&key.path);
    process::interactive(&mut command, "load the SSH key")?;
    *agent = probe(&agent.socket)?;
    if !agent.fingerprints.contains(&key.fingerprint) {
        bail!("The agent did not report the expected fingerprint for {}. Check that the private key and public key match.", key.path.display());
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn regular_files_are_not_sockets() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("socket");
        fs::write(&path, "keep").unwrap();
        assert!(probe(&path).is_err());
        assert!(check_link_slot(&path).is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "keep");
    }
    #[test]
    fn link_update_resolves_existing_link_without_looping() {
        let dir = tempfile::tempdir().unwrap();
        let socket = dir.path().join("socket");
        let _listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let stable = dir.path().join("agent.sock");
        update_link(&stable, &socket).unwrap();
        update_link(&stable, &stable).unwrap();
        assert_eq!(
            fs::canonicalize(stable).unwrap(),
            fs::canonicalize(socket).unwrap()
        );
    }
    #[test]
    fn missing_link_slot_is_allowed() {
        let dir = tempfile::tempdir().unwrap();
        check_link_slot(&dir.path().join("agent.sock")).unwrap();
    }
}
