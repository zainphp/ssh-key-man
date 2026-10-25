use anyhow::{bail, Context, Result};
use std::io::{self, IsTerminal, Write};

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
