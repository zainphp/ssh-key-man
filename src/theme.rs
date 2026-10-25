use crossterm::style::{Color as TerminalColor, ContentStyle, StyledContent};
use ratatui::style::{Color, Style};
use std::io::{self, IsTerminal};

#[derive(Clone, Copy)]
pub(crate) enum Tone {
    Accent,
    Success,
    Warning,
    Danger,
}

fn color_enabled(terminal: bool, no_color: bool, dumb: bool) -> bool {
    terminal && !no_color && !dumb
}

fn enabled(terminal: bool) -> bool {
    color_enabled(
        terminal,
        std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()),
        std::env::var_os("TERM").is_some_and(|value| value == "dumb"),
    )
}

impl Tone {
    fn colors(self) -> (Color, TerminalColor) {
        match self {
            Self::Accent => (Color::Cyan, TerminalColor::DarkCyan),
            Self::Success => (Color::Green, TerminalColor::DarkGreen),
            Self::Warning => (Color::Yellow, TerminalColor::DarkYellow),
            Self::Danger => (Color::Red, TerminalColor::DarkRed),
        }
    }

    pub(crate) fn style(self) -> Style {
        if !enabled(true) {
            return Style::default();
        }
        Style::default().fg(self.colors().0)
    }

    pub(crate) fn stdout(self, text: &str) -> StyledContent<&str> {
        self.label(text, io::stdout().is_terminal())
    }

    pub(crate) fn stderr(self, text: &str) -> StyledContent<&str> {
        self.label(text, io::stderr().is_terminal())
    }

    fn label(self, text: &str, terminal: bool) -> StyledContent<&str> {
        let mut style = ContentStyle::default();
        if enabled(terminal) {
            style.foreground_color = Some(self.colors().1);
        }
        style.apply(text)
    }
}

#[cfg(test)]
mod tests {
    use super::color_enabled;

    #[test]
    fn color_requires_a_terminal_and_respects_opt_outs() {
        assert!(color_enabled(true, false, false));
        assert!(!color_enabled(false, false, false));
        assert!(!color_enabled(true, true, false));
        assert!(!color_enabled(true, false, true));
    }
}
