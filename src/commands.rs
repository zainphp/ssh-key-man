use crate::theme::Tone;
use crate::{agent, cli, keys, tui};
use anyhow::{bail, Context, Result};
use std::path::Path;

pub(crate) fn dispatch(cli: cli::Cli) -> Result<()> {
    match cli.command {
        None | Some(cli::Commands::Hosts) => {
            let directory = keys::ssh_dir()?;
            cli::interactive::require_terminal()?;
            tui::run(&directory)
        }
        Some(cli::Commands::Update) => update(),
        Some(cli::Commands::Gen) => generate_wizard(&keys::ssh_dir()?),
        Some(cli::Commands::Load { filename }) => load_keys(&keys::ssh_dir()?, filename),
        Some(cli::Commands::Doctor) => check_agent(&keys::ssh_dir()?),
        Some(cli::Commands::Init { shell }) => {
            println!("{}", cli::shell_init(shell));
            Ok(())
        }
    }
}

fn load_keys(dir: &Path, filename: Option<String>) -> Result<()> {
    let inventory = keys::inventory(dir)?;
    for warning in &inventory.warnings {
        eprintln!("{} {warning}", Tone::Warning.stderr("Warning:"));
    }
    let selected = if let Some(name) = filename {
        let path = keys::key_path(dir, &name)?;
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
    let mut agent = agent::ensure(dir)?;
    let mut errors = Vec::new();
    for key in selected {
        match agent::load(&mut agent, &key) {
            Ok(loaded) => println!(
                "{}: {} ({})",
                key.path.display(),
                if loaded {
                    Tone::Success.stdout("Loaded in agent")
                } else {
                    Tone::Success.stdout("Already loaded")
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

fn check_agent(dir: &Path) -> Result<()> {
    println!("{}\n", Tone::Accent.stdout("ssh-key-man / doctor"));
    println!("{}", Tone::Accent.stdout("OpenSSH commands"));
    let mut missing = Vec::new();
    for binary in ["ssh", "ssh-keygen", "ssh-add", "ssh-agent", "ssh-copy-id"] {
        match which::which(binary) {
            Ok(path) => println!(
                "  {}  {binary:<12} {}",
                Tone::Success.stdout("OK     "),
                path.display()
            ),
            Err(_) => {
                println!("  {}  {binary}", Tone::Danger.stdout("MISSING"));
                missing.push(binary);
            }
        }
    }
    println!("\n{}", Tone::Accent.stdout("SSH agent"));
    if missing.contains(&"ssh-add") {
        println!(
            "  {}  Install ssh-add to check the agent.",
            Tone::Warning.stdout("SKIPPED")
        );
    } else {
        let agent = agent::ensure(dir)?;
        println!("  {}  Agent is responding", Tone::Success.stdout("OK     "));
        println!("  {:<16} {}", "Socket", agent.socket.display());
        println!(
            "  {:<16} {}",
            "Shell connection",
            dir.join("agent.sock").display()
        );
        println!("  {:<16} {}", "Loaded keys", agent.fingerprints.len());
        println!("\n{}", Tone::Accent.stdout("Shell setup"));
        println!("  Add the output of skm init to your shell startup file.");
    }
    if !missing.is_empty() {
        bail!("Could not find these commands: {}. Install the missing OpenSSH commands and make sure they are in PATH. ssh-copy-id may need a separate installation.", missing.join(", "));
    }
    Ok(())
}

fn generate_wizard(dir: &Path) -> Result<()> {
    cli::interactive::require_terminal()?;
    let use_rsa = loop {
        let answer = cli::interactive::prompt("Key type [ed25519/rsa] (ed25519): ", "ed25519")?;
        match answer.to_lowercase().as_str() {
            "ed25519" => break false,
            "rsa" | "rsa4096" => break true,
            _ => eprintln!(
                "{}",
                Tone::Warning.stderr("Choose ed25519 or rsa. RSA keys use 4096 bits.")
            ),
        }
    };
    let default = if use_rsa { "id_rsa" } else { "id_ed25519" };
    let name = cli::interactive::prompt(&format!("Filename in ~/.ssh/ ({default}): "), default)?;
    println!(
        "OpenSSH will ask for a passphrase to protect the private key. Leave it blank for an unencrypted key."
    );
    let key = keys::generate(dir, &name, use_rsa)?;
    println!(
        "{} {}\nFingerprint: {}",
        Tone::Success.stdout("Created"),
        key.path.display(),
        key.fingerprint
    );
    let registration = (|| -> Result<()> {
        let answer = cli::interactive::prompt("Add this public key to a server now? [y/N] ", "n")?;
        if answer.eq_ignore_ascii_case("y") || answer.eq_ignore_ascii_case("yes") {
            let remote = cli::interactive::prompt("Server (user@hostname): ", "")?;
            let port = cli::interactive::prompt("SSH port (22): ", "22")?
                .parse::<u16>()
                .context("Invalid SSH port. Enter a number from 1 to 65535.")?;
            keys::deploy(&key, &remote, port)?;
        }
        Ok(())
    })();
    let load = agent::ensure(dir).and_then(|mut agent| agent::load(&mut agent, &key));
    if load.is_ok() {
        println!(
            "{} {}",
            Tone::Success.stdout("Loaded in agent:"),
            key.path.display()
        );
    }
    match (registration, load) {
        (Err(a), Err(b)) => bail!("Key was saved, but adding it to the server and loading it into the agent failed.\n{a:#}\n{b:#}\nRetry loading with skm load {name}."),
        (Err(e), _) => Err(e).context("Key was saved and loaded, but remote setup did not finish. Add it later with ssh-copy-id -i <key>.pub user@hostname."),
        (_, Err(e)) => Err(e).with_context(|| format!("Key was saved, but agent loading failed. Retry with skm load {name}.")),
        _ => Ok(()),
    }
}

fn update() -> Result<()> {
    let executable = std::env::current_exe()
        .context("Could not locate this installation. Run the curl installer to update instead.")?;
    let directory = executable.parent().context(
        "Could not locate the installation folder. Run the curl installer to update instead.",
    )?;
    println!("Updating ssh-key-man {}...", env!("CARGO_PKG_VERSION"));
    let status = std::process::Command::new("sh")
        .args(["-c", include_str!("../install.sh")])
        .env("SKM_INSTALL_DIR", directory)
        .status()
        .context("Could not start the installer. Make sure sh is installed and available in PATH, then retry.")?;
    if !status.success() {
        bail!("Update did not finish. Fix the installer error above, then run skm update again.");
    }
    Ok(())
}
