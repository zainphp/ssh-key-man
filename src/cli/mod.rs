pub mod interactive;
pub(crate) mod shell;

use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(version, about = "Manage SSH keys, host aliases, and your SSH agent")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a key, optionally add its public key to a server, and load it into the SSH agent
    Gen,
    /// Download and install the latest release in the current installation folder
    Update,
    /// Add, edit, or delete SSH host aliases in ~/.ssh/config
    Hosts,
    /// Load a key into the SSH agent (loads all private keys in ~/.ssh when no filename is given)
    Load { filename: Option<String> },
    /// Check the SSH agent, start one if needed, and set up the shell connection
    Doctor,
    /// Print SSH agent settings, or add them to shell startup files with --install
    Init {
        #[arg(value_enum)]
        shell: Option<Shell>,
        /// Add settings to startup files (detects your shell from SHELL when omitted)
        #[arg(long)]
        install: bool,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum Shell {
    Bash,
    Zsh,
    Fish,
}

pub fn shell_init(shell: Shell) -> &'static str {
    match shell {
        Shell::Bash | Shell::Zsh => "export SSH_AUTH_SOCK=\"$HOME/.ssh/agent.sock\"",
        Shell::Fish => "set -gx SSH_AUTH_SOCK \"$HOME/.ssh/agent.sock\"",
    }
}
