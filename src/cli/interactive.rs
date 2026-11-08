use anyhow::{bail, Context, Result};
use std::io::{self, IsTerminal, Write};
use std::path::Path;

pub(crate) fn agent_hint(socket: &Path) -> Option<&'static str> {
    let shell_socket =
        std::env::var_os("SSH_AUTH_SOCK").and_then(|path| std::fs::canonicalize(path).ok());
    let loaded_socket = std::fs::canonicalize(socket).ok();
    if shell_socket.is_some() && shell_socket == loaded_socket {
        return None;
    }
    Some("Your shell is not connected to this SSH agent.\nRerun the installer to set up future terminals.\nFor this terminal, run:\nBash/Zsh: export SSH_AUTH_SOCK=\"$HOME/.ssh/agent.sock\"\nFish: set -gx SSH_AUTH_SOCK \"$HOME/.ssh/agent.sock\"")
}

pub(crate) fn print_agent_hint(socket: &Path) {
    if let Some(hint) = agent_hint(socket) {
        eprintln!(
            "{}\n{hint}",
            crate::theme::Tone::Warning.stderr("Shell setup needed:")
        );
    }
}

pub fn require_terminal() -> Result<()> {
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        bail!("This action needs an interactive terminal. Run it directly in your terminal without piping input or output.");
    }
    Ok(())
}

pub fn prompt(label: &str, default: &str) -> Result<String> {
    print!("{}", crate::theme::Tone::Accent.stdout(label));
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
