use super::model::{Action, Focus, Page};
use crate::theme::Tone;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
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

pub(super) fn draw_tabs(frame: &mut Frame, header: Rect, page: Page, focus: Focus) {
    let row = Rect::new(header.x, header.y + 1, header.width, 1);
    frame.render_widget(
        Tabs::new(["[H] Hosts", "[K] Keys"])
            .style(Tone::Accent.style())
            .select(page.index())
            .highlight_style(selection_style(focus == Focus::Pages)),
        row,
    );
}

pub(super) fn draw_actions(frame: &mut Frame, area: Rect, page: Page, focus: Focus, action: usize) {
    let rows = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);
    let hint = match focus {
        Focus::Pages => "Enter: open page",
        Focus::Actions => "Enter: run action",
        Focus::List if page == Page::Hosts => "Enter: connect and close skm",
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

pub(super) fn draw_notice(frame: &mut Frame, message: &str) {
    let area = frame.area();
    let width = area.width.min(84);
    let height = area.height.min(14);
    let popup = Rect::new(
        area.x + (area.width - width) / 2,
        area.y + (area.height - height) / 2,
        width,
        height,
    );
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(format!("{message}\n\nEnter: continue | Esc: quit"))
            .wrap(Wrap { trim: false })
            .block(
                Block::default()
                    .title(" Message ")
                    .borders(Borders::ALL)
                    .border_style(Tone::Warning.style()),
            ),
        popup,
    );
}

pub(super) fn dialog_text(text: &str, message: &str) -> Text<'static> {
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
                    Some(Tone::Danger)
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
        let text = dialog_text(
            "> Filename: id_test\nEnter: save\n\nInvalid filename. Choose a name.",
            "Invalid filename. Choose a name.",
        );
        assert_eq!(text.lines[0].style, Tone::Accent.style());
        assert_eq!(text.lines[1].style, Style::default());
        assert_eq!(text.lines[3].style, Tone::Danger.style());
        assert!(selection_style(true)
            .add_modifier
            .contains(Modifier::REVERSED));
    }
}
