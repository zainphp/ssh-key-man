mod operations;
#[cfg(test)]
mod tests;
mod view;

use super::{
    components::{clamp_selection, move_selection},
    hosts::HostPage,
    model::{Action, Focus, InputResult, Operation, Page},
    wizard::{DialogResult, KeyWizard},
};
use crate::{
    agent::{self, Agent},
    keys::{self, Key},
};
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::widgets::TableState;
use std::path::{Path, PathBuf};

pub(super) struct App {
    directory: PathBuf,
    page: Page,
    focus: Focus,
    action_index: usize,
    hosts: HostPage,
    keys: Vec<Key>,
    agent: Option<Agent>,
    key_selection: TableState,
    wizard: Option<KeyWizard>,
    message: String,
}

impl App {
    pub fn new(directory: &Path) -> Result<Self> {
        Ok(Self {
            directory: directory.to_path_buf(),
            page: Page::Hosts,
            focus: Focus::List,
            action_index: 0,
            hosts: HostPage::new(directory)?,
            keys: Vec::new(),
            agent: None,
            key_selection: TableState::default(),
            wizard: None,
            message: String::new(),
        })
    }

    fn refresh_keys(&mut self) -> Result<()> {
        let mut inventory = keys::inventory(&self.directory)?;
        self.keys = inventory.keys;
        self.agent = match agent::current(&self.directory) {
            Ok(agent) => agent,
            Err(error) => {
                inventory.warnings.push(format!("{error:#}"));
                None
            }
        };
        if !inventory.warnings.is_empty() {
            self.message = inventory.warnings.join("\n");
        }
        clamp_selection(&mut self.key_selection, self.keys.len());
        Ok(())
    }

    fn select_page(&mut self, page: Page) -> Result<()> {
        self.action_index = 0;
        match page {
            Page::Keys => {
                self.page = page;
                self.refresh_keys()?;
            }
            Page::Hosts => {
                if self.page != page {
                    self.hosts.refresh(&self.directory)?;
                }
                self.page = page;
            }
        }
        Ok(())
    }

    pub fn notice_mut(&mut self) -> &mut String {
        match self.page {
            Page::Hosts => &mut self.hosts.message,
            Page::Keys => &mut self.message,
        }
    }

    pub fn handle_key(&mut self, event: KeyEvent) -> Result<InputResult> {
        if event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok(InputResult::Quit);
        }
        if self.page == Page::Keys {
            if let Some(wizard) = &mut self.wizard {
                return Ok(match wizard.handle_key(event, &mut self.message)? {
                    DialogResult::Continue => InputResult::Continue,
                    DialogResult::Cancel => {
                        self.wizard = None;
                        InputResult::Continue
                    }
                    DialogResult::Run(operation) => InputResult::Run(operation),
                });
            }
        } else if self.hosts.is_editing() {
            if let Err(error) = self.hosts.handle_key(&self.directory, event) {
                self.hosts.message = format!("{error:#}");
            }
            return Ok(InputResult::Continue);
        }
        if event.code == KeyCode::Esc {
            return Ok(InputResult::Quit);
        }
        let notice = self.notice_mut();
        if !notice.is_empty() {
            if event.code == KeyCode::Enter {
                notice.clear();
            }
            return Ok(InputResult::Continue);
        }
        match event.code {
            KeyCode::Char('k' | 'K') | KeyCode::Char('h' | 'H') => {
                let page = if matches!(event.code, KeyCode::Char('k' | 'K')) {
                    Page::Keys
                } else {
                    Page::Hosts
                };
                self.focus = Focus::List;
                self.select_page(page)?;
                return Ok(InputResult::Continue);
            }
            KeyCode::Tab | KeyCode::BackTab => return Ok(InputResult::Continue),
            _ => {}
        }
        match (self.focus, event.code) {
            (Focus::Pages | Focus::List, KeyCode::Left | KeyCode::Right) => {
                self.select_page(self.page.other())?
            }
            (Focus::Pages, KeyCode::Enter | KeyCode::Down) | (Focus::Actions, KeyCode::Up) => {
                self.focus = Focus::List
            }
            (Focus::Pages, KeyCode::Up) => self.focus = Focus::Actions,
            (Focus::Actions, KeyCode::Down) => self.focus = Focus::Pages,
            (Focus::List, KeyCode::Up | KeyCode::Down) => {
                let down = event.code == KeyCode::Down;
                let moved = match self.page {
                    Page::Hosts => self.hosts.move_selection(down),
                    Page::Keys => move_selection(&mut self.key_selection, self.keys.len(), down),
                };
                if !moved {
                    self.focus = if down { Focus::Actions } else { Focus::Pages };
                }
            }
            (Focus::Actions, KeyCode::Left | KeyCode::Right) => {
                let count = self.page.actions().len();
                self.action_index = (self.action_index
                    + if event.code == KeyCode::Right {
                        1
                    } else {
                        count - 1
                    })
                    % count;
            }
            (Focus::Actions, KeyCode::Enter) => {
                self.focus = Focus::List;
                return self.handle_action(self.page.actions()[self.action_index]);
            }
            (_, KeyCode::Char(_)) => {
                self.focus = Focus::List;
                if let Some(action) = self.page.shortcut(event.code) {
                    return self.handle_action(action);
                }
                if event.code == KeyCode::Char('j') {
                    match self.page {
                        Page::Hosts => {
                            self.hosts.move_selection(true);
                        }
                        Page::Keys => {
                            move_selection(&mut self.key_selection, self.keys.len(), true);
                        }
                    }
                }
            }
            (Focus::List, KeyCode::Enter) => match self.page {
                Page::Hosts => match self.hosts.connection_alias() {
                    Ok(alias) => return Ok(InputResult::Run(Operation::Connect(alias))),
                    Err(error) => self.hosts.message = format!("{error:#}"),
                },
                Page::Keys => {
                    if let Some(key) = self.selected_key() {
                        return Ok(InputResult::Run(Operation::Load(key.clone())));
                    }
                    self.message = "No key selected. Press N to create a key.".into();
                }
            },
            _ => {}
        }
        Ok(InputResult::Continue)
    }

    fn selected_key(&self) -> Option<&Key> {
        self.key_selection
            .selected()
            .and_then(|index| self.keys.get(index))
    }

    fn handle_action(&mut self, action: Action) -> Result<InputResult> {
        match self.page {
            Page::Hosts => {
                if let Err(error) = self.hosts.handle_action(action) {
                    self.hosts.message = format!("{error:#}");
                }
            }
            Page::Keys => {
                self.wizard = match action {
                    Action::New => Some(KeyWizard::new()),
                    Action::Delete => Some(KeyWizard::delete(
                        self.selected_key()
                            .context("No key selected. Select a key before deleting it.")?
                            .clone(),
                    )),
                    Action::Edit => return Ok(InputResult::Continue),
                };
                self.message.clear();
            }
        }
        Ok(InputResult::Continue)
    }
}
