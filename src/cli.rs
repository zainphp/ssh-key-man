use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    version,
    about = "Create SSH keys, register servers, and manage your SSH agent"
)]
pub struct Cli {
    #[command(subcommand)]
    pub command: Option<Commands>,
}

#[derive(Subcommand)]
pub enum Commands {
    /// Create a key, optionally register it on a server, and load it into the agent
    Gen,
    /// Load a key from ~/.ssh (loads all private keys when no filename is given)
    Load { filename: Option<String> },
    /// Find or start an SSH agent and repair the stable socket link
    Doctor,
    /// Print shell configuration; add the output to your shell startup file
    Init {
        #[arg(value_enum, default_value = "bash")]
        shell: Shell,
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
