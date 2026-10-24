use super::model::Operation;
use crate::keys::{self, Key};
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use std::path::Path;

#[derive(Default)]
pub(super) struct KeyForm {
    pub(super) rsa: bool,
    pub(super) filename: String,
    pub(super) field: usize,
}

pub(super) enum KeyWizard {
    Parameters(KeyForm),
    Register {
        key: Key,
        selected: bool,
    },
    Delete {
        key: Key,
        confirm: bool,
    },
    Server {
        key: Key,
        remote: String,
        port: String,
        field: usize,
        previous_choice: bool,
    },
}

pub(super) enum DialogResult {
    Continue,
    Cancel,
    Run(Operation),
}

impl KeyWizard {
    pub fn new() -> Self {
        Self::Parameters(KeyForm {
            filename: "id_ed25519".into(),
            ..KeyForm::default()
        })
    }

    pub fn register(key: Key) -> Self {
        Self::Register {
            key,
            selected: false,
        }
    }

    pub fn delete(key: Key) -> Self {
        Self::Delete {
            key,
            confirm: false,
        }
    }

    pub fn handle_key(&mut self, event: KeyEvent, message: &mut String) -> Result<DialogResult> {
        match self {
            Self::Delete { key, confirm } => match event.code {
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                    *confirm = !*confirm
                }
                KeyCode::Char('y' | 'Y') | KeyCode::Enter
                    if *confirm || matches!(event.code, KeyCode::Char('y' | 'Y')) =>
                {
                    return Ok(DialogResult::Run(Operation::Delete(key.clone())))
                }
                KeyCode::Esc | KeyCode::Enter | KeyCode::Char('n' | 'N') => {
                    return Ok(DialogResult::Cancel)
                }
                _ => {}
            },
            Self::Parameters(form) => match event.code {
                KeyCode::Esc => return Ok(DialogResult::Cancel),
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                    form.field = 1 - form.field
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 0 => {
                    form.rsa = !form.rsa;
                    if form.filename == "id_ed25519" || form.filename == "id_rsa" {
                        form.filename = if form.rsa { "id_rsa" } else { "id_ed25519" }.into();
                    }
                }
                KeyCode::Enter => {
                    keys::key_path(Path::new("."), &form.filename)?;
                    return Ok(DialogResult::Run(Operation::Generate {
                        name: form.filename.clone(),
                        rsa: form.rsa,
                    }));
                }
                KeyCode::Char(c) if form.field == 1 && !c.is_control() => form.filename.push(c),
                KeyCode::Backspace if form.field == 1 => {
                    form.filename.pop();
                }
                _ => {}
            },
            Self::Register { key, selected } => match event.code {
                KeyCode::Char('y' | 'Y') | KeyCode::Enter
                    if *selected || matches!(event.code, KeyCode::Char('y' | 'Y')) =>
                {
                    *self = Self::Server {
                        key: key.clone(),
                        remote: String::new(),
                        port: "22".into(),
                        field: 0,
                        previous_choice: *selected,
                    };
                    message.clear();
                }
                KeyCode::Left | KeyCode::Right | KeyCode::Up | KeyCode::Down => {
                    *selected = !*selected
                }
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                    return Ok(DialogResult::Run(Operation::Finish {
                        key: key.clone(),
                        remote: None,
                    }))
                }
                _ => {}
            },
            Self::Server {
                key,
                remote,
                port,
                field,
                previous_choice,
            } => match event.code {
                KeyCode::Esc => {
                    *self = Self::Register {
                        key: key.clone(),
                        selected: *previous_choice,
                    }
                }
                KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down => {
                    *field = 1 - *field
                }
                KeyCode::Backspace => {
                    if *field == 0 {
                        remote.pop();
                    } else {
                        port.pop();
                    }
                }
                KeyCode::Char(c) if !c.is_control() => {
                    if *field == 0 {
                        remote.push(c);
                    } else {
                        port.push(c);
                    }
                }
                KeyCode::Enter => {
                    keys::validate_remote(remote)?;
                    let port = port
                        .parse::<u16>()
                        .ok()
                        .filter(|p| *p > 0)
                        .context("Invalid SSH port. Enter a number from 1 to 65535.")?;
                    return Ok(DialogResult::Run(Operation::Finish {
                        key: key.clone(),
                        remote: Some((remote.clone(), port)),
                    }));
                }
                _ => {}
            },
        }
        Ok(DialogResult::Continue)
    }

    pub fn draw(&self, frame: &mut Frame, message: &str) {
        let area = frame.area();
        let height = if matches!(self, Self::Delete { .. }) {
            20
        } else {
            16
        };
        let popup = Rect::new(
            area.x + area.width.saturating_sub(76) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            area.width.min(76),
            area.height.min(height),
        );
        frame.render_widget(Clear, popup);
        let (title, text) = match self {
            Self::Delete { key, confirm } => {
                let files = if key.private {
                    format!("Private key: {}\nPublic key: {} (if present)\n\nAlso unloads the key from the current agent, if available.", key.path.display(), keys::public_path(&key.path).display())
                } else { format!("Public key: {}", key.path.display()) };
                (" Delete key ", format!("{files}\n\nThis cannot be undone. Keys added to servers stay there.\n\n{} Cancel\n{} Delete files\n\nEnter: confirm | Esc: cancel", if *confirm { " " } else { ">" }, if *confirm { ">" } else { " " }))
            }
            Self::Parameters(form) => (" Create key: step 1 of 3 ", format!("{} Key type: {}\n\n{} Filename: {}\n\nA passphrase protects your private key. OpenSSH will ask for one next.\n\nUp/Down: field | Left/Right/Space: key type\nEnter: create key | Esc: cancel\n\n{}", if form.field == 0 { ">" } else { " " }, if form.rsa { "RSA 4096" } else { "Ed25519" }, if form.field == 1 { ">" } else { " " }, form.filename, message)),
            Self::Register { selected, .. } => (" Add public key: step 2 of 3 ", format!("Add this public key to a server now? [y/N]\n\n{} Add public key to server\n{} Skip server setup\n\nArrows: choose | Enter: continue\nY: add | N / Esc: skip\n\n{}", if *selected { ">" } else { " " }, if *selected { " " } else { ">" }, message)),
            Self::Server { remote, port, field, .. } => (" Server setup: step 2 of 3 ", format!("{} Server (user@hostname): {}\n\n{} SSH port: {}\n\nUp/Down: field | Enter: add public key and load into agent\nEsc: back\n\nOpenSSH will ask you to verify the host and sign in.\n\n{}", if *field == 0 { ">" } else { " " }, remote, if *field == 1 { ">" } else { " " }, port, message)),
        };
        frame.render_widget(
            Paragraph::new(text).wrap(Wrap { trim: false }).block(
                Block::default()
                    .title(title)
                    .borders(Borders::ALL)
                    .border_style(Style::default().add_modifier(Modifier::BOLD)),
            ),
            popup,
        );
    }
}
