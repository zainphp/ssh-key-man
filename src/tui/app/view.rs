use super::App;
use crate::theme::Tone;
use crate::tui::{
    components::{draw_actions, draw_header, selection_style, HEADER_HEIGHT},
    model::{Focus, Page},
};
use ratatui::{
    layout::{Constraint, Layout},
    style::Modifier,
    text::{Line, Span, Text},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

impl App {
    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        self.resize(area);
        frame.render_widget(Block::default().style(crate::theme::base_style()), area);
        if area.width < 60 || area.height < 18 {
            frame.render_widget(Paragraph::new("Make the terminal at least 60 columns wide and 18 rows tall. Press Esc to quit.").wrap(Wrap { trim: false }), area);
            return;
        }
        if self.page == Page::Hosts {
            self.hosts.draw(frame, self.focus, self.action_index);
            if !self.hosts.is_editing() && !self.hosts.message.is_empty() {
                self.hosts.message.draw(frame);
            }
            return;
        }
        let areas = Layout::vertical([
            Constraint::Length(HEADER_HEIGHT),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(area);
        draw_header(frame, areas[0], Page::Keys, self.focus);
        if self.refresh.is_some() {
            frame.render_widget(
                Paragraph::new(
                    "Reading keys and checking the SSH agent...
Esc: quit",
                )
                .wrap(Wrap { trim: false }),
                areas[1],
            );
            return;
        }
        let sections = Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(areas[1]);
        let is_loaded = |key: &crate::keys::Key| {
            self.agent
                .as_ref()
                .is_some_and(|agent| agent.fingerprints.contains(&key.fingerprint))
        };
        if self.keys.is_empty() {
            frame.render_widget(
                Paragraph::new("No keys found.\nPress N to create a key.")
                    .wrap(Wrap { trim: false })
                    .block(Block::default().title(" Keys ").borders(Borders::ALL)),
                sections[0],
            );
        } else {
            let rows = self.keys.iter().map(|key| {
                Row::new([
                    Cell::from(key.path.file_name().unwrap_or_default().to_string_lossy()),
                    Cell::from(if is_loaded(key) { "●" } else { "" }).style(Tone::Success.style()),
                ])
            });
            let table = Table::new(rows, [Constraint::Min(0), Constraint::Length(1)])
                .block(
                    Block::default()
                        .title(" Keys in ~/.ssh/ ")
                        .borders(Borders::ALL),
                )
                .row_highlight_style(selection_style(self.focus == Focus::List))
                .highlight_symbol("> ");
            frame.render_stateful_widget(table, sections[0], &mut self.key_selection);
            let indicator_column = sections[0].right() - 2;
            for y in sections[0].y + 1..sections[0].bottom() - 1 {
                let cell = &mut frame.buffer_mut()[(indicator_column, y)];
                if cell.symbol() == "●" {
                    cell.set_style(Tone::Success.style().remove_modifier(Modifier::REVERSED));
                }
            }
        }
        let details = self
            .key_selection
            .selected()
            .and_then(|i| self.keys.get(i))
            .map(|key| {
                let (status, tone) = if is_loaded(key) {
                    ("Loaded in agent", Tone::Success)
                } else if !key.private {
                    ("Public key only", Tone::Warning)
                } else {
                    ("Not loaded", Tone::Warning)
                };
                let path = key
                    .path
                    .strip_prefix(&self.directory)
                    .map(|name| format!("~/.ssh/{}", name.display()))
                    .unwrap_or_else(|_| key.path.display().to_string());
                Text::from(vec![
                    Line::from(vec![
                        Span::styled("Status: ", Tone::Accent.style()),
                        Span::styled(status, tone.style()),
                    ]),
                    Line::from(vec![
                        Span::styled("Key type: ", Tone::Accent.style()),
                        Span::raw(key.algorithm.as_str()),
                    ]),
                    Line::from(vec![
                        Span::styled("Fingerprint: ", Tone::Accent.style()),
                        Span::raw(key.fingerprint.as_str()),
                    ]),
                    Line::from(vec![
                        Span::styled("File: ", Tone::Accent.style()),
                        Span::raw(path),
                    ]),
                ])
            })
            .unwrap_or_else(|| Text::from("Select a key to see its key type and fingerprint."));
        let mut block = Block::default()
            .title(" Key details ")
            .borders(Borders::ALL);
        let inner = block.inner(sections[1]);
        let overflowing = self.detail_scroll.draw(frame, inner, details);
        if overflowing {
            block = block.title_bottom(" PgUp/PgDn: scroll ");
        }
        frame.render_widget(block, sections[1]);
        let public_only = self.selected_key().is_some_and(|key| !key.private);
        draw_actions(
            frame,
            areas[2],
            Page::Keys,
            self.focus,
            self.action_index,
            public_only,
        );
        if self.wizard.is_none() && !self.message.is_empty() {
            self.message.draw(frame);
        }
        if let Some(wizard) = &self.wizard {
            wizard.draw(frame, &self.message, &mut self.dialog_scroll);
        }
    }
}
