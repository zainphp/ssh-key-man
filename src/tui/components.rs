use super::model::{Action, Focus, Page};
use super::{notice::Notice, scroll::Scroll};
use crate::theme::{base_style, color_style, Tone};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph, TableState, Tabs, Wrap},
    Frame,
};

pub(super) fn selection_style(focused: bool) -> Style {
    let style = Style::default().add_modifier(Modifier::BOLD);
    if focused {
        style.add_modifier(Modifier::REVERSED)
    } else {
        style
    }
}

pub(super) fn clamp_selection(table: &mut TableState, count: usize) {
    table.select(if count == 0 {
        None
    } else {
        Some(table.selected().unwrap_or(0).min(count - 1))
    });
}

pub(super) fn move_selection(table: &mut TableState, count: usize, down: bool) -> bool {
    if count == 0 {
        return false;
    }
    let previous = table.selected().unwrap_or(0);
    let next = if down {
        (previous + 1).min(count - 1)
    } else {
        previous.saturating_sub(1)
    };
    table.select(Some(next));
    next != previous
}

pub(super) const HEADER_HEIGHT: u16 = 7;
const WORDMARK: &str = r" __   __  |  |      | /   ___ \  /      |\/|  /\  |\ |
(__  (__  |__|  --  |<   |__   \/   --  |  | /__\ | \|
 __)  __) |  |      | \  |___  /        |  | |  | |  |";

pub(super) fn draw_header(frame: &mut Frame, header: Rect, page: Page, focus: Focus) {
    let header = Rect::new(
        header.x,
        header.y,
        header.width,
        header.height.saturating_sub(1),
    );
    let wordmark: Vec<Line> = WORDMARK
        .lines()
        .map(|row| {
            let (ssh, rest) = row.split_at(20);
            let (key, man) = rest.split_at(20);
            Line::from(vec![
                Span::styled(ssh, color_style(Color::LightCyan)),
                Span::styled(key, color_style(Color::Cyan)),
                Span::styled(man, color_style(Color::LightBlue)),
            ])
        })
        .collect();
    frame.render_widget(
        Paragraph::new(wordmark)
            .style(Style::default().add_modifier(Modifier::BOLD))
            .block(Block::default().borders(Borders::BOTTOM)),
        header,
    );
    let row = Rect::new(
        header.x,
        header.y + header.height.saturating_sub(2),
        header.width,
        1,
    );
    let version = concat!("v", env!("CARGO_PKG_VERSION"));
    let columns =
        Layout::horizontal([Constraint::Min(0), Constraint::Length(version.len() as u16)])
            .split(row);
    frame.render_widget(
        Tabs::new(["[H] Hosts", "[K] Keys"])
            .style(Tone::Accent.style())
            .select(page.index())
            .highlight_style(selection_style(focus == Focus::Pages)),
        columns[0],
    );
    frame.render_widget(Paragraph::new(version), columns[1]);
}

pub(super) fn draw_actions(
    frame: &mut Frame,
    area: Rect,
    page: Page,
    focus: Focus,
    action: usize,
    public_only: bool,
) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
    let hint = match focus {
        Focus::Pages => "Enter: open page",
        Focus::Actions => "Enter: run action",
        Focus::List if page == Page::Hosts => "Enter: connect and close skm",
        Focus::List if public_only => "Public key only: select a private key to load",
        Focus::List => "Enter: load key and close skm",
    };
    frame.render_widget(Paragraph::new(hint).style(Tone::Accent.style()), rows[0]);
    let labels = page.actions().iter().map(|action| {
        let tone = if matches!(action, Action::Delete) {
            Tone::Danger
        } else {
            Tone::Accent
        };
        Line::from(Span::styled(action.label(page), tone.style()))
    });
    frame.render_widget(
        Tabs::new(labels)
            .select(action)
            .highlight_style(selection_style(focus == Focus::Actions)),
        rows[1],
    );
}

pub(super) fn draw_dialog(
    frame: &mut Frame,
    popup: Rect,
    block: Block,
    text: Text,
    footer: &str,
    scroll: &mut Scroll,
) {
    frame.render_widget(Clear, popup);
    let inner = block.inner(popup);
    frame.render_widget(block.style(base_style()), popup);
    let footer = Paragraph::new(footer).wrap(Wrap { trim: false });
    let height = footer
        .line_count(inner.width)
        .min(usize::from(inner.height)) as u16;
    let parts = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(height),
    ])
    .split(inner);
    scroll.draw(frame, parts[0], text);
    frame.render_widget(footer, parts[2]);
}

pub(super) fn dialog_text(text: &str, notice: &Notice) -> Text<'static> {
    let message = &notice.text;
    let message_start = if message.is_empty() {
        text.len()
    } else {
        text.rfind(message).unwrap_or(text.len())
    };
    let mut offset = 0;
    Text::from(
        text.split('\n')
            .map(|line| {
                let tone = if offset >= message_start {
                    Some(notice.tone)
                } else if line.starts_with("> ") {
                    Some(Tone::Accent)
                } else {
                    None
                };
                offset += line.len() + 1;
                Line::styled(
                    line.to_owned(),
                    tone.map_or_else(Style::default, Tone::style),
                )
            })
            .collect::<Vec<_>>(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dialogs_keep_focus_and_errors_distinct() {
        let mut notice = Notice::default();
        notice.set("Invalid filename. Choose a name.", Tone::Danger);
        let text = dialog_text(
            "> Filename: id_test\nEnter: save\n\nInvalid filename. Choose a name.",
            &notice,
        );
        assert_eq!(text.lines[0].style, Tone::Accent.style());
        assert_eq!(text.lines[1].style, Style::default());
        assert_eq!(text.lines[3].style, Tone::Danger.style());
        assert!(selection_style(true)
            .add_modifier
            .contains(Modifier::REVERSED));
    }

    use ratatui::{backend::TestBackend, Terminal};

    #[test]
    fn successful_wizard_feedback_uses_its_success_tone() {
        let mut notice = Notice::default();
        notice.set("Created a key.", Tone::Success);
        let text = dialog_text("Choose server setup.\nCreated a key.", &notice);
        assert_eq!(text.lines[1].style, Tone::Success.style());
        assert_eq!(notice.tone, Tone::Success);
        if crate::theme::base_style().fg.is_some() {
            assert_ne!(text.lines[1].style, Tone::Danger.style());
        }
    }

    #[test]
    fn long_dialog_text_can_scroll_without_hiding_its_footer() {
        let mut terminal = Terminal::new(TestBackend::new(60, 18)).unwrap();
        let mut scroll = Scroll::default();
        let body = (0..40)
            .map(|index| format!("Field detail {index}"))
            .collect::<Vec<_>>()
            .join("\n");
        let draw = |frame: &mut Frame, scroll: &mut Scroll| {
            draw_dialog(
                frame,
                frame.area(),
                Block::default().title(" Test ").borders(Borders::ALL),
                Text::from(body.as_str()),
                "Enter: save | Esc: cancel | PgUp/PgDn: scroll",
                scroll,
            )
        };
        terminal.draw(|frame| draw(frame, &mut scroll)).unwrap();
        for _ in 0..40 {
            scroll.handle_key(crossterm::event::KeyCode::PageDown);
        }
        terminal.draw(|frame| draw(frame, &mut scroll)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(screen.contains("Field detail 39"));
        assert!(screen.contains("Enter: save | Esc: cancel"));
    }
}
