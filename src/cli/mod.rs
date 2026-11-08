pub mod interactive;

use clap::{Parser, Subcommand};

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
}
