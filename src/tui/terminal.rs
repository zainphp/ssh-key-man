use anyhow::{Context, Result};
use crossterm::{
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};

#[derive(Default)]
pub(super) struct TerminalSession {
    active: bool,
}
impl TerminalSession {
    pub fn enter(&mut self) -> Result<Terminal<CrosstermBackend<Stdout>>> {
        self.active = true;
        enable_raw_mode()
            .context("Could not enable terminal input. Run skm in an interactive terminal.")?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        terminal.clear()?;
        Ok(terminal)
    }
    pub fn restore() {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }

    pub fn leave(&mut self) {
        if self.active {
            Self::restore();
            self.active = false;
        }
    }
}
impl Drop for TerminalSession {
    fn drop(&mut self) {
        self.leave();
    }
}
