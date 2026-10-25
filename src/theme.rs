use crossterm::style::{Color as TerminalColor, ContentStyle, StyledContent};
use ratatui::style::{Color, Style};
use std::io::{self, IsTerminal};

const BACKGROUND: (u8, u8, u8) = (16, 27, 33);
const FOREGROUND: (u8, u8, u8) = (237, 244, 243);
const ACCENT: (u8, u8, u8) = (116, 222, 212);
const HEADER_LIGHT: (u8, u8, u8) = (176, 238, 230);
const HEADER_BLUE: (u8, u8, u8) = (168, 199, 255);

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) enum Tone {
    Accent,
    Success,
    #[default]
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

fn rgb((r, g, b): (u8, u8, u8)) -> Color {
    Color::Rgb(r, g, b)
}

impl Tone {
    fn color(self) -> (u8, u8, u8) {
        match self {
            Self::Accent => ACCENT,
            Self::Success => (146, 221, 155),
            Self::Warning => (235, 202, 118),
            Self::Danger => (255, 146, 146),
        }
    }

    pub(crate) fn style(self) -> Style {
        color_style(rgb(self.color()))
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
            let (r, g, b) = self.color();
            let (br, bg, bb) = BACKGROUND;
            style.foreground_color = Some(TerminalColor::Rgb { r, g, b });
            style.background_color = Some(TerminalColor::Rgb {
                r: br,
                g: bg,
                b: bb,
            });
        }
        style.apply(text)
    }
}

pub(crate) fn base_style() -> Style {
    if enabled(true) {
        Style::default().fg(rgb(FOREGROUND)).bg(rgb(BACKGROUND))
    } else {
        Style::default()
    }
}

pub(crate) fn color_style(color: Color) -> Style {
    if enabled(true) {
        let color = match color {
            Color::LightCyan => rgb(HEADER_LIGHT),
            Color::Cyan => rgb(ACCENT),
            Color::LightBlue => rgb(HEADER_BLUE),
            other => other,
        };
        base_style().fg(color)
    } else {
        Style::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_requires_a_terminal_and_respects_opt_outs() {
        assert!(color_enabled(true, false, false));
        assert!(!color_enabled(false, false, false));
        assert!(!color_enabled(true, true, false));
        assert!(!color_enabled(true, false, true));
    }

    #[test]
    fn all_text_colors_meet_normal_text_contrast_on_their_background() {
        fn luminance((r, g, b): (u8, u8, u8)) -> f64 {
            let linear = |channel: u8| {
                let value = f64::from(channel) / 255.0;
                if value <= 0.04045 {
                    value / 12.92
                } else {
                    ((value + 0.055) / 1.055).powf(2.4)
                }
            };
            0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)
        }
        for foreground in [
            FOREGROUND,
            ACCENT,
            HEADER_LIGHT,
            HEADER_BLUE,
            Tone::Success.color(),
            Tone::Warning.color(),
            Tone::Danger.color(),
        ] {
            let ratio = (luminance(foreground) + 0.05) / (luminance(BACKGROUND) + 0.05);
            assert!(ratio >= 4.5, "{foreground:?} has contrast {ratio}");
        }
    }
}
