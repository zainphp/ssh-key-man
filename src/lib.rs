#[cfg(not(unix))]
compile_error!("ssh-key-man requires Unix sockets. Use Linux, macOS, or WSL.");

pub mod agent;
pub mod cli;
pub mod hosts;
pub mod keys;
mod process;
mod tui;

use anyhow::{bail, Context, Result};
use clap::Parser;
use std::{path::Path, process::ExitCode};

pub fn run() -> ExitCode {
    match dispatch(cli::Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn dispatch(cli: cli::Cli) -> Result<()> {
    if let Some(cli::Commands::Init { shell }) = cli.command {
        println!("{}", cli::shell_init(shell));
        return Ok(());
    }
    let dir = keys::ssh_dir()?;
    match cli.command {
        None => {
            keys::require_terminal()?;
            tui::run(&dir)
        }
        Some(cli::Commands::Gen) => generate_wizard(&dir),
        Some(cli::Commands::Hosts) => {
            keys::require_terminal()?;
            tui::run_hosts(&dir)
        }
        Some(cli::Commands::Load { filename }) => {
            let inventory = keys::inventory(&dir)?;
            for warning in &inventory.warnings {
                eprintln!("Warning: {warning}");
            }
            let selected = if let Some(name) = filename {
                let path = keys::key_path(&dir, &name)?;
                vec![inventory
                    .keys
                    .into_iter()
                    .find(|k| k.path == path)
                    .with_context(|| {
                        format!(
                            "Could not find a readable key: {}. Check the filename and key format.",
                            path.display()
                        )
                    })?]
            } else {
                inventory.keys.into_iter().filter(|k| k.private).collect()
            };
            if selected.is_empty() {
                bail!(
                    "No private keys found in {}. Run skm gen to create one.",
                    dir.display()
                );
            }
            let mut agent = agent::ensure(&dir)?;
            let mut errors = Vec::new();
            for key in selected {
                match agent::load(&mut agent, &key) {
                    Ok(loaded) => println!(
                        "{}: {} ({})",
                        key.path.display(),
                        if loaded {
                            "Loaded in agent"
                        } else {
                            "Already loaded"
                        },
                        key.fingerprint
                    ),
                    Err(e) => errors.push(format!("{e:#}")),
                }
            }
            if !errors.is_empty() {
                bail!("{}", errors.join("\n"));
            }
            Ok(())
        }
        Some(cli::Commands::Doctor) => {
            for binary in ["ssh-keygen", "ssh-add", "ssh-agent", "ssh-copy-id"] {
                match which::which(binary) {
                    Ok(path) => println!("{binary}: {}", path.display()),
                    Err(_) => println!("{binary}: missing. Install OpenSSH to use this command."),
                }
            }
            let agent = agent::ensure(&dir)?;
            println!("SSH agent connection: {}\nShell connection: {}\nKeys loaded into agent: {}\nAdd the output of skm init to your shell startup file.", agent.socket.display(), dir.join("agent.sock").display(), agent.fingerprints.len());
            Ok(())
        }
        Some(cli::Commands::Init { .. }) => unreachable!(),
    }
}

fn generate_wizard(dir: &Path) -> Result<()> {
    keys::require_terminal()?;
    let algorithm = loop {
        let answer = keys::prompt("Key type [ed25519/rsa] (ed25519): ", "ed25519")?;
        match answer.to_lowercase().as_str() {
            "ed25519" => break false,
            "rsa" | "rsa4096" => break true,
            _ => eprintln!("Choose ed25519 or rsa. RSA keys use 4096 bits."),
        }
    };
    let default = if algorithm { "id_rsa" } else { "id_ed25519" };
    let name = keys::prompt(&format!("Filename in ~/.ssh/ ({default}): "), default)?;
    println!(
        "OpenSSH will ask for a passphrase to protect the private key. Leave it blank for an unencrypted key."
    );
    let key = keys::generate(dir, &name, algorithm)?;
    println!(
        "Created {}\nFingerprint: {}",
        key.path.display(),
        key.fingerprint
    );
    let registration = (|| -> Result<()> {
        let answer = keys::prompt("Add this public key to a server now? [y/N] ", "n")?;
        if answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes") {
            let remote = keys::prompt("Server (user@hostname): ", "")?;
            let port = keys::prompt("SSH port (22): ", "22")?
                .parse::<u16>()
                .context("Invalid SSH port. Enter a number from 1 to 65535.")?;
            keys::deploy(&key, &remote, port)?;
        }
        Ok(())
    })();
    let load = agent::ensure(dir).and_then(|mut agent| agent::load(&mut agent, &key));
    if load.is_ok() {
        println!("Loaded in agent: {}", key.path.display());
    }
    match (registration, load) {
        (Err(a), Err(b)) => bail!("Key was saved, but adding it to the server and loading it into the agent failed.\n{a:#}\n{b:#}\nRetry loading with skm load {name}."),
        (Err(e), _) => Err(e).context("Key was saved and loaded, but remote setup did not finish. Add it later with ssh-copy-id -i <key>.pub user@hostname."),
        (_, Err(e)) => Err(e).with_context(|| format!("Key was saved, but agent loading failed. Retry with skm load {name}.")),
        _ => Ok(()),
    }
}
