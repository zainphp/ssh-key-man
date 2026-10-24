use super::HostPage;
use crate::tui::{
    components,
    model::{Focus, Page},
};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, Wrap},
    Frame,
};

impl HostPage {
    pub fn draw(&mut self, frame: &mut Frame, focus: Focus, action: usize) {
        let area = frame.area();
        if area.width < 60 || area.height < 18 {
            frame.render_widget(Paragraph::new("Make the terminal at least 60 columns wide and 18 rows tall. Press Esc to quit.").wrap(Wrap { trim: false }), area);
            return;
        }
        let parts = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(3),
            Constraint::Length(2),
        ])
        .split(area);
        frame.render_widget(
            Paragraph::new("ssh-key-man | Hosts in ~/.ssh/config")
                .block(Block::default().borders(Borders::BOTTOM)),
            parts[0],
        );
        components::draw_tabs(frame, parts[0], Page::Hosts, focus);
        if self.config.entries.is_empty() {
            frame.render_widget(Paragraph::new("No host aliases found. Press N to add one.\nSave a hostname, user, port, and key so you can connect with ssh <alias>.").wrap(Wrap { trim: false }), parts[1]);
        } else {
            let rows = self.config.entries.iter().map(|entry| {
                let host = &entry.host;
                let alias = if entry.read_only {
                    Cell::from(format!("{} [read-only]", host.alias))
                } else {
                    Cell::from(host.alias.as_str())
                };
                Row::new([
                    alias,
                    Cell::from(host.hostname.as_str()),
                    Cell::from(host.user.as_str()),
                    Cell::from(host.port.as_str()),
                ])
            });
            let table = Table::new(
                rows,
                [
                    Constraint::Percentage(30),
                    Constraint::Percentage(35),
                    Constraint::Percentage(20),
                    Constraint::Percentage(15),
                ],
            )
            .header(
                Row::new(["Alias", "Hostname or IP", "Username", "Port"])
                    .style(Style::default().add_modifier(Modifier::BOLD)),
            )
            .row_highlight_style(components::selection_style(focus == Focus::List))
            .highlight_symbol("> ");
            frame.render_stateful_widget(table, parts[1], &mut self.table);
        }
        components::draw_actions(frame, parts[2], Page::Hosts, focus, action);
        if self.form.is_some() || self.deleting {
            let width = area.width.min(80);
            let height = area.height.min(18);
            let popup = Rect::new(
                area.x + (area.width - width) / 2,
                area.y + (area.height - height) / 2,
                width,
                height,
            );
            frame.render_widget(Clear, popup);
            let (title, text) = if let Some(form) = &self.form {
                let mut lines = Vec::new();
                for (i, label) in [
                    "Alias",
                    "Hostname or IP",
                    "Username (optional)",
                    "Port (optional)",
                    "Key file (optional)",
                ]
                .iter()
                .enumerate()
                {
                    lines.push(format!(
                        "{} {label}: {}",
                        if i == form.field { ">" } else { " " },
                        form.values[i]
                    ));
                }
                lines.push("\nTab/Up/Down: field | Ctrl+U: clear field".into());
                lines.push("Enter: save | Esc: cancel".into());
                lines.push("Leave blank to use your SSH settings and defaults.".into());
                lines.push(format!("\n{}", self.message));
                (
                    if form.index.is_some() {
                        " Edit SSH host "
                    } else {
                        " Add SSH host "
                    },
                    lines.join("\n"),
                )
            } else {
                let alias = &self.config.entries[self.table.selected().unwrap()]
                    .host
                    .alias;
                (" Delete SSH host ", format!("Delete alias {alias} and its settings from ~/.ssh/config?\n\nA backup will be saved. No keys or server files will be deleted.\n\n{} Cancel\n{} Delete alias\n\nArrows: choose | Enter: confirm | Esc: cancel\nY: delete | N: cancel\n\n{}", if self.delete_selected { " " } else { ">" }, if self.delete_selected { ">" } else { " " }, self.message))
            };
            frame.render_widget(
                Paragraph::new(text)
                    .wrap(Wrap { trim: false })
                    .block(Block::default().title(title).borders(Borders::ALL)),
                popup,
            );
        }
    }
}
