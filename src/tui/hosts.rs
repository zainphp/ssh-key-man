use super::Focus;
use crate::hosts::{Config, Host};
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Row, Table, TableState, Wrap},
    Frame,
};
use std::path::Path;

struct Form {
    index: Option<usize>,
    values: [String; 5],
    field: usize,
}

pub(super) struct Hosts {
    config: Config,
    table: TableState,
    form: Option<Form>,
    deleting: bool,
    delete_selected: bool,
    pub message: String,
}

impl Hosts {
    pub fn connection_alias(&self) -> Result<String> {
        let entry = self
            .table
            .selected()
            .and_then(|i| self.config.entries.get(i))
            .context("No host selected. Press N to add an alias first.")?;
        crate::hosts::validate_alias(&entry.host.alias)?;
        Ok(entry.host.alias.clone())
    }

    pub fn move_selection(&mut self, down: bool) -> bool {
        super::move_selection(&mut self.table, self.config.entries.len(), down)
    }

    pub fn is_editing(&self) -> bool {
        self.form.is_some() || self.deleting
    }

    pub fn refresh(&mut self, dir: &Path) -> Result<()> {
        self.config = Config::load(dir)?;
        self.select();
        Ok(())
    }

    pub fn new(dir: &Path) -> Result<Self> {
        let mut hosts = Self {
            config: Config::load(dir)?,
            table: TableState::default(),
            form: None,
            deleting: false,
            delete_selected: false,
            message: String::new(),
        };
        hosts.select();
        Ok(hosts)
    }

    fn select(&mut self) {
        self.table.select(if self.config.entries.is_empty() {
            None
        } else {
            Some(
                self.table
                    .selected()
                    .unwrap_or(0)
                    .min(self.config.entries.len() - 1),
            )
        });
    }

    fn saved(
        &mut self,
        dir: &Path,
        backup: Option<std::path::PathBuf>,
        message: String,
    ) -> Result<()> {
        self.config = Config::load(dir)?;
        self.form = None;
        self.deleting = false;
        self.select();
        self.message = format!(
            "{message}{}",
            backup
                .map(|p| format!(" Backup: {}", p.display()))
                .unwrap_or_default()
        );
        Ok(())
    }

    pub fn input(&mut self, dir: &Path, event: KeyEvent) -> Result<bool> {
        if let Some(form) = self.form.as_mut() {
            match event.code {
                KeyCode::Esc => {
                    self.form = None;
                    self.message.clear();
                }
                KeyCode::Tab | KeyCode::Down => form.field = (form.field + 1) % 5,
                KeyCode::BackTab | KeyCode::Up => form.field = (form.field + 4) % 5,
                KeyCode::Backspace => {
                    form.values[form.field].pop();
                }
                KeyCode::Char('u') if event.modifiers.contains(KeyModifiers::CONTROL) => {
                    form.values[form.field].clear()
                }
                KeyCode::Char(c)
                    if !c.is_control() && !event.modifiers.contains(KeyModifiers::CONTROL) =>
                {
                    form.values[form.field].push(c)
                }
                KeyCode::Enter => {
                    let saved_index = form.index.unwrap_or(0);
                    let [alias, hostname, user, port, identity_file] = form.values.clone();
                    let host = Host {
                        alias,
                        hostname,
                        user,
                        port,
                        identity_file,
                    };
                    let backup = self.config.save(dir, form.index, &host)?;
                    self.saved(
                        dir,
                        backup,
                        format!("Saved {}. Connect with ssh {}.", host.alias, host.alias),
                    )?;
                    self.table.select(Some(saved_index));
                }
                _ => {}
            }
            return Ok(true);
        }
        if self.deleting {
            match event.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter
                    if self.delete_selected || matches!(event.code, KeyCode::Char('y' | 'Y')) =>
                {
                    let index = self
                        .table
                        .selected()
                        .context("No host selected. Choose a host first.")?;
                    let alias = self.config.entries[index].host.alias.clone();
                    let backup = self.config.delete(dir, index)?;
                    self.saved(dir, backup, format!("Deleted alias {alias}."))?;
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                    self.delete_selected = !self.delete_selected
                }
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('n' | 'N') => self.deleting = false,
                _ => {}
            }
            return Ok(true);
        }
        match event.code {
            KeyCode::Esc => return Ok(false),
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_selection(true);
            }
            KeyCode::Up => {
                self.move_selection(false);
            }
            KeyCode::Char('n' | 'N' | 'a' | 'A') => {
                self.form = Some(Form {
                    index: None,
                    values: Default::default(),
                    field: 0,
                });
                self.message.clear();
            }
            KeyCode::Char('e' | 'E' | 'd' | 'D') => {
                let index = self
                    .table
                    .selected()
                    .context("No host selected. Press N to add an alias.")?;
                let entry = &self.config.entries[index];
                if entry.read_only {
                    anyhow::bail!("This entry matches multiple hosts or has repeated settings. Edit it directly in ~/.ssh/config.");
                }
                if matches!(event.code, KeyCode::Char('d' | 'D')) {
                    self.deleting = true;
                    self.delete_selected = false;
                } else {
                    let host = &entry.host;
                    self.form = Some(Form {
                        index: Some(index),
                        values: [
                            host.alias.clone(),
                            host.hostname.clone(),
                            host.user.clone(),
                            host.port.clone(),
                            host.identity_file.clone(),
                        ],
                        field: 0,
                    });
                }
                self.message.clear();
            }
            _ => {}
        }
        Ok(true)
    }

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
        super::draw_tabs(frame, parts[0], 0, focus);
        if self.config.entries.is_empty() {
            frame.render_widget(Paragraph::new("No host aliases found. Press N to add one.\nSave a hostname, user, port, and key so you can connect with ssh <alias>.").wrap(Wrap { trim: false }), parts[1]);
        } else {
            let rows = self.config.entries.iter().map(|entry| {
                let host = &entry.host;
                Row::new(vec![
                    format!(
                        "{}{}",
                        host.alias,
                        if entry.read_only { " [read-only]" } else { "" }
                    ),
                    host.hostname.clone(),
                    host.user.clone(),
                    host.port.clone(),
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
            .row_highlight_style(super::selection_style(focus == Focus::List))
            .highlight_symbol("> ");
            frame.render_stateful_widget(table, parts[1], &mut self.table);
        }
        super::draw_actions(frame, parts[2], true, focus, action);
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

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;
    use ratatui::{backend::TestBackend, Terminal};
    use std::fs;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn new_host_is_selected_even_when_created_from_the_last_row() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("config"),
            "Host first\n HostName first.invalid\nHost second\n HostName second.invalid\n",
        )
        .unwrap();
        let mut view = Hosts::new(dir.path()).unwrap();
        view.move_selection(true);
        view.input(dir.path(), press(KeyCode::Char('n'))).unwrap();
        view.form.as_mut().unwrap().values = [
            "new-host".into(),
            "new.invalid".into(),
            String::new(),
            String::new(),
            String::new(),
        ];
        view.input(dir.path(), press(KeyCode::Enter)).unwrap();
        assert_eq!(view.connection_alias().unwrap(), "new-host");
    }

    #[test]
    fn form_creates_edits_and_confirms_deletion() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = Hosts::new(dir.path()).unwrap();
        view.input(dir.path(), press(KeyCode::Char('n'))).unwrap();
        assert!(view.input(dir.path(), press(KeyCode::Enter)).is_err());
        let form = view.form.as_mut().unwrap();
        form.values = [
            "work".into(),
            "example.invalid".into(),
            "alice".into(),
            "2222".into(),
            "~/.ssh/id_work".into(),
        ];
        view.input(dir.path(), press(KeyCode::Enter)).unwrap();
        assert_eq!(view.config.entries[0].host.alias, "work");
        assert!(view.message.contains("ssh work"));
        view.input(dir.path(), press(KeyCode::Char('e'))).unwrap();
        view.input(
            dir.path(),
            KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
        )
        .unwrap();
        for c in "office".chars() {
            view.input(dir.path(), press(KeyCode::Char(c))).unwrap();
        }
        view.input(dir.path(), press(KeyCode::Enter)).unwrap();
        assert_eq!(view.config.entries[0].host.alias, "office");
        view.input(dir.path(), press(KeyCode::Char('d'))).unwrap();
        view.input(dir.path(), press(KeyCode::Enter)).unwrap();
        assert_eq!(view.config.entries.len(), 1);
        view.input(dir.path(), press(KeyCode::Char('d'))).unwrap();
        view.input(dir.path(), press(KeyCode::Down)).unwrap();
        view.input(dir.path(), press(KeyCode::Enter)).unwrap();
        assert!(view.config.entries.is_empty());
        assert!(fs::read_to_string(dir.path().join("config"))
            .unwrap()
            .trim()
            .is_empty());
    }

    #[test]
    fn renders_empty_list_form_and_confirmation_at_supported_sizes() {
        let dir = tempfile::tempdir().unwrap();
        let mut view = Hosts::new(dir.path()).unwrap();
        for (width, height) in [(1, 1), (40, 8), (60, 18), (110, 30)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| view.draw(f, Focus::List, 0)).unwrap();
            view.input(dir.path(), press(KeyCode::Char('n'))).unwrap();
            terminal.draw(|f| view.draw(f, Focus::List, 0)).unwrap();
            view.input(dir.path(), press(KeyCode::Esc)).unwrap();
        }
        assert!(!view.input(dir.path(), press(KeyCode::Esc)).unwrap());
    }
}
