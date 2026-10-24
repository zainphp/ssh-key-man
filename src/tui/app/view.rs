use super::App;
use crate::tui::{
    components::{draw_actions, draw_notice, draw_tabs, selection_style},
    model::{Focus, Page},
};
use ratatui::{
    layout::{Constraint, Layout},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table, Wrap},
    Frame,
};

impl App {
    pub fn draw(&mut self, frame: &mut Frame) {
        if self.page == Page::Hosts {
            self.hosts.draw(frame, self.focus, self.action_index);
            if !self.hosts.is_editing() && !self.hosts.message.is_empty() {
                draw_notice(frame, &self.hosts.message);
            }
            return;
        }
        let area = frame.area();
        if area.width < 60 || area.height < 18 {
            frame.render_widget(
                Paragraph::new(
                    "Make the terminal at least 60 columns wide and 18 rows tall. Press Esc to quit.",
                )
                .wrap(Wrap { trim: false }),
                area,
            );
            return;
        }
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(2),
        ])
        .split(area);
        let status = if let Some(agent) = &self.agent {
            format!(
                "ssh-key-man | Agent ready | {} loaded",
                agent.fingerprints.len()
            )
        } else {
            "ssh-key-man | No active agent connection".into()
        };
        frame.render_widget(
            Paragraph::new(status).block(Block::default().borders(Borders::BOTTOM)),
            areas[0],
        );
        draw_tabs(frame, areas[0], Page::Keys, self.focus);
        let sections = Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(areas[1]);
        if self.keys.is_empty() {
            frame.render_widget(
                Paragraph::new("No keys found.\nPress N to create a key.")
                    .wrap(Wrap { trim: false })
                    .block(Block::default().title(" Keys ").borders(Borders::ALL)),
                sections[0],
            );
        } else {
            let rows = self.keys.iter().map(|key| {
                let loaded = self
                    .agent
                    .as_ref()
                    .is_some_and(|a| a.fingerprints.contains(&key.fingerprint));
                Row::new(vec![Cell::from(format!(
                    "{}\n{}",
                    key.path.file_name().unwrap_or_default().to_string_lossy(),
                    if loaded {
                        "Loaded in agent"
                    } else if !key.private {
                        "Public key only"
                    } else {
                        "Not loaded"
                    }
                ))])
                .height(2)
            });
            let table = Table::new(rows, [Constraint::Min(0)])
                .block(
                    Block::default()
                        .title(" Keys in ~/.ssh/ ")
                        .borders(Borders::ALL),
                )
                .row_highlight_style(selection_style(self.focus == Focus::List))
                .highlight_symbol("> ");
            frame.render_stateful_widget(table, sections[0], &mut self.key_selection);
        }
        let details = self
            .key_selection
            .selected()
            .and_then(|i| self.keys.get(i))
            .map(|key| {
                format!(
                    "File: {}\nKey type: {}\nFingerprint: {}",
                    key.path.display(),
                    key.algorithm,
                    key.fingerprint
                )
            })
            .unwrap_or_else(|| "Select a key to see its key type and fingerprint.".into());
        frame.render_widget(
            Paragraph::new(details).wrap(Wrap { trim: false }).block(
                Block::default()
                    .title(" Key details ")
                    .borders(Borders::ALL),
            ),
            sections[1],
        );
        draw_actions(frame, areas[2], Page::Keys, self.focus, self.action_index);
        if self.wizard.is_none() && !self.message.is_empty() {
            draw_notice(frame, &self.message);
        }
        if let Some(wizard) = &self.wizard {
            wizard.draw(frame, &self.message);
        }
    }
}
