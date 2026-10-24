#[cfg(test)]
mod tests;
mod view;

use crate::hosts::{Config, Host};
use crate::tui::{components, model::Action};
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::TableState;
use std::path::Path;

struct HostForm {
    index: Option<usize>,
    values: [String; 5],
    field: usize,
}

pub(super) struct HostPage {
    config: Config,
    table: TableState,
    form: Option<HostForm>,
    deleting: bool,
    delete_selected: bool,
    pub message: String,
}

impl HostPage {
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
        components::move_selection(&mut self.table, self.config.entries.len(), down)
    }

    pub fn is_editing(&self) -> bool {
        self.form.is_some() || self.deleting
    }

    pub fn refresh(&mut self, dir: &Path) -> Result<()> {
        self.config = Config::load(dir)?;
        self.update_selection();
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
        hosts.update_selection();
        Ok(hosts)
    }

    fn update_selection(&mut self) {
        components::clamp_selection(&mut self.table, self.config.entries.len());
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
        self.update_selection();
        self.message = format!(
            "{message}{}",
            backup
                .map(|p| format!(" Backup: {}", p.display()))
                .unwrap_or_default()
        );
        Ok(())
    }

    pub fn handle_key(&mut self, dir: &Path, event: KeyEvent) -> Result<()> {
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
            return Ok(());
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
            return Ok(());
        }
        Ok(())
    }

    pub fn handle_action(&mut self, action: Action) -> Result<()> {
        match action {
            Action::New => {
                self.form = Some(HostForm {
                    index: None,
                    values: Default::default(),
                    field: 0,
                });
                self.message.clear();
            }
            Action::Edit | Action::Delete => {
                let index = self
                    .table
                    .selected()
                    .context("No host selected. Press N to add an alias.")?;
                let entry = &self.config.entries[index];
                if entry.read_only {
                    anyhow::bail!("This entry matches multiple hosts or has repeated settings. Edit it directly in ~/.ssh/config.");
                }
                if matches!(action, Action::Delete) {
                    self.deleting = true;
                    self.delete_selected = false;
                } else {
                    let host = &entry.host;
                    self.form = Some(HostForm {
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
        }
        Ok(())
    }
}
