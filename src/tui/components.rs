use super::model::{Focus, Page};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, TableState, Tabs, Wrap},
    Frame,
};

pub(super) fn selection_style(focused: bool) -> Style {
    let style = Style::default().add_modifier(Modifier::BOLD);
    if focused {
        style.fg(Color::Black).bg(Color::Cyan)
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
    frame.render_widget(Paragraph::new(hint), rows[0]);
    let labels = page.actions().iter().map(|action| action.label(page));
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
            .block(Block::default().title(" Message ").borders(Borders::ALL)),
        popup,
    );
}
