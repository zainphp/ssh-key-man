use anyhow::{bail, Context, Result};
use std::process::Command;

const REPOSITORY: &str = "https://github.com/zainphp/ssh-key-man";

pub(crate) fn run() -> Result<()> {
    let executable = std::env::current_exe()
        .context("Could not locate this installation. Run the curl installer to update instead.")?;
    let directory = executable.parent().context(
        "Could not locate the installation folder. Run the curl installer to update instead.",
    )?;
    println!("Checking the latest ssh-key-man release...");
    let release = fetch(
        &format!("{REPOSITORY}/releases/latest"),
        &["--output", "/dev/null", "--write-out", "%{url_effective}"],
    )?;
    let release = String::from_utf8(release)
        .context("GitHub returned an invalid release URL. Retry skm update later.")?;
    let prefix = format!("{REPOSITORY}/releases/tag/");
    let tag = release.trim().strip_prefix(&prefix)
        .context("GitHub did not return a release URL for ssh-key-man. Retry later or install a release manually.")?;
    if tag.is_empty()
        || !tag
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        bail!("GitHub did not return a supported release tag. Retry later or install a release manually.");
    }
    let commit = fetch(
        &format!("https://api.github.com/repos/zainphp/ssh-key-man/commits/tags%2F{tag}"),
        &["--header", "Accept: application/vnd.github.sha"],
    )?;
    let commit = String::from_utf8(commit)
        .context("GitHub returned an invalid commit ID. Retry skm update later.")?;
    let commit = commit.trim();
    if commit.len() != 40 || !commit.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("GitHub did not return a valid commit ID for {tag}. Retry later or install the release manually.");
    }
    let installer = tempfile::NamedTempFile::new().context(
        "Could not prepare the installer. Check temporary folder permissions and free disk space.",
    )?;
    let path = installer.path().to_str().context(
        "The temporary folder path is not valid UTF-8. Set TMPDIR to a UTF-8 path and retry.",
    )?;
    fetch(
        &format!("https://raw.githubusercontent.com/zainphp/ssh-key-man/{commit}/install.sh"),
        &["--output", path],
    )?;
    println!("Installing ssh-key-man {tag}...");
    let status = Command::new("sh")
        .arg(installer.path())
        .env("SKM_VERSION", tag)
        .env("SKM_INSTALL_DIR", directory)
        .status()
        .context("Could not start the installer. Make sure sh is installed and available in PATH, then retry.")?;
    if !status.success() {
        bail!("Update did not finish. Fix the installer error above, then run skm update again.");
    }
    Ok(())
}

fn fetch(url: &str, options: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("curl")
        .args([
            "--proto", "=https", "--proto-redir", "=https", "--tlsv1.2",
            "--fail", "--silent", "--show-error", "--location",
            "--connect-timeout", "10", "--max-time", "60", "--max-filesize", "1048576",
            "--retry", "2", "--user-agent", concat!("ssh-key-man/", env!("CARGO_PKG_VERSION")),
        ])
        .args(options)
        .arg(url)
        .output()
        .context("Could not start curl. Install curl and make sure it is in PATH, then retry skm update.")?;
    if !output.status.success() {
        bail!("Could not download {url} ({}): {}. Check your connection and retry later if GitHub is unavailable or rate limited.", output.status, String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output.stdout)
}
