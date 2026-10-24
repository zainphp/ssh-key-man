#[cfg(not(unix))]
compile_error!("ssh-key-man requires Unix sockets. Use Linux, macOS, or WSL.");

pub mod agent;
pub mod cli;
mod commands;
pub mod hosts;
pub mod keys;
mod process;
mod tui;

use clap::Parser;
use std::process::ExitCode;

pub fn run() -> ExitCode {
    match commands::dispatch(cli::Cli::parse()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("Error: {error:#}");
            ExitCode::FAILURE
        }
    }
}
