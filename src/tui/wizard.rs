use super::{
    components::{dialog_text, draw_dialog},
    model::Operation,
    notice::Notice,
    scroll::Scroll,
};
use crate::keys::{self, Key};
use crate::theme::Tone;
use anyhow::{Context, Result};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::{
    layout::Rect,
    widgets::{Block, Borders},
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

    pub fn handle_key(&mut self, event: KeyEvent, message: &mut Notice) -> Result<DialogResult> {
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

    pub fn draw(&self, frame: &mut Frame, notice: &Notice, scroll: &mut Scroll) {
        let area = frame.area();
        let height = if matches!(self, Self::Delete { .. }) {
            20
        } else {
            18
        };
        let popup = Rect::new(
            area.x + area.width.saturating_sub(76) / 2,
            area.y + area.height.saturating_sub(height) / 2,
            area.width.min(76),
            area.height.min(height),
        );
        let message = &notice.text;
        let (title, text, footer) = match self {
            Self::Delete { key, confirm } => {
                let files = if key.private {
                    format!(
                        "Private key: {}
Public key: {} (if present)

Also unloads the key from the current agent, if available.",
                        key.path.display(),
                        keys::public_path(&key.path).display()
                    )
                } else {
                    format!("Public key: {}", key.path.display())
                };
                (
                    " Delete key ",
                    format!(
                        "{files}

This cannot be undone. Keys added to servers stay there.

{} Cancel
{} Delete files",
                        if *confirm { " " } else { ">" },
                        if *confirm { ">" } else { " " }
                    ),
                    "Arrows: choose | Enter: confirm | Esc: cancel
PgUp/PgDn: scroll",
                )
            }
            Self::Parameters(form) => (
                " Create key: step 1 of 3 ",
                format!(
                    "{} Key type: {}

{} Filename: {}

A passphrase protects your private key. OpenSSH will ask for one next.

{message}",
                    if form.field == 0 { ">" } else { " " },
                    if form.rsa { "RSA 4096" } else { "Ed25519" },
                    if form.field == 1 { ">" } else { " " },
                    form.filename
                ),
                "Up/Down: field | Left/Right/Space: key type
Enter: create key | Esc: cancel | PgUp/PgDn: scroll",
            ),
            Self::Register { selected, .. } => (
                " Add public key: step 2 of 3 ",
                format!(
                    "Add this public key to a server now? [y/N]

{} Add public key to server
{} Skip server setup

{message}",
                    if *selected { ">" } else { " " },
                    if *selected { " " } else { ">" }
                ),
                "Arrows: choose | Enter: continue
Y: add | N / Esc: skip | PgUp/PgDn: scroll",
            ),
            Self::Server {
                remote,
                port,
                field,
                ..
            } => (
                " Server setup: step 2 of 3 ",
                format!(
                    "{} Server (user@hostname): {remote}

{} SSH port: {port}

OpenSSH will ask you to verify the host and sign in.

{message}",
                    if *field == 0 { ">" } else { " " },
                    if *field == 1 { ">" } else { " " }
                ),
                "Up/Down: field | Enter: add public key and load into agent
Esc: back | PgUp/PgDn: scroll",
            ),
        };
        draw_dialog(
            frame,
            popup,
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(if matches!(self, Self::Delete { .. }) {
                    Tone::Danger.style()
                } else {
                    Tone::Accent.style()
                }),
            dialog_text(&text, notice),
            footer,
            scroll,
        );
    }
}
