use anyhow::{bail, Context, Result};
use std::{
    io::Read,
    process::{Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

pub fn command(name: &str) -> Result<Command> {
    let path = which::which(name).with_context(|| {
        format!("Could not find {name}. Install OpenSSH and make sure {name} is in PATH.")
    })?;
    let mut command = Command::new(path);
    command.env("LC_ALL", "C");
    Ok(command)
}

pub fn capture(command: &mut Command) -> Result<Output> {
    let mut child = command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Could not start an SSH command. Check executable permissions.")?;
    let stdout = child
        .stdout
        .take()
        .context("Could not read command output. Try again.")?;
    let stderr = child
        .stderr
        .take()
        .context("Could not read command errors. Try again.")?;
    let out = thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(1_048_576)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let err = thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr
            .take(1_048_576)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let start = Instant::now();
    let status = loop {
        if let Some(status) = child
            .try_wait()
            .context("Could not check the SSH command. Try again.")?
        {
            break status;
        }
        if start.elapsed() > Duration::from_secs(3) {
            let _ = child.kill();
            let _ = child.wait();
            bail!("The SSH command did not respond within 3 seconds. Run skm doctor to check the SSH agent, then try again.");
        }
        thread::sleep(Duration::from_millis(20));
    };
    Ok(Output {
        status,
        stdout: out
            .join()
            .map_err(|_| anyhow::anyhow!("Could not read SSH output. Try again."))??,
        stderr: err
            .join()
            .map_err(|_| anyhow::anyhow!("Could not read SSH errors. Try again."))??,
    })
}

pub fn interactive(command: &mut Command, action: &str) -> Result<()> {
    let status = command
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .with_context(|| {
            format!("Could not start the command to {action}. Check that the OpenSSH command is installed and you have permission to run it.")
        })?;
    if !status.success() {
        bail!("Could not {action} ({status}). Check the OpenSSH message for the cause, then try again.");
    }
    Ok(())
}
