mod hosts;

use crate::{
    agent::{self, Agent},
    keys::{self, Key},
};
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Tabs, Wrap},
    Frame, Terminal,
};
use std::{
    io::{self, Stdout},
    path::{Path, PathBuf},
    time::Duration,
};

#[derive(Default)]
struct Form {
    rsa: bool,
    filename: String,
    field: usize,
}

enum Wizard {
    Parameters(Form),
    Register(Key),
    Server {
        key: Key,
        remote: String,
        port: String,
        field: usize,
    },
}

enum Work {
    Generate {
        name: String,
        rsa: bool,
    },
    Finish {
        key: Key,
        remote: Option<(String, u16)>,
    },
    Load(Key),
    Doctor,
    Connect(String),
}

#[derive(Default)]
struct App {
    dir: PathBuf,
    hosts: Option<hosts::Hosts>,
    hosts_active: bool,
    keys: Vec<Key>,
    agent: Option<Agent>,
    table: TableState,
    wizard: Option<Wizard>,
    message: String,
}

impl App {
    fn new(dir: &Path) -> Result<Self> {
        let mut app = Self {
            dir: dir.to_path_buf(),
            message: "The SSH agent holds unlocked keys for SSH. Use Up/Down to select a key, or G to create one.".into(),
            ..Self::default()
        };
        app.refresh(dir)?;
        Ok(app)
    }

    fn refresh(&mut self, dir: &Path) -> Result<()> {
        let inventory = keys::inventory(dir)?;
        self.keys = inventory.keys;
        self.agent = agent::current(dir);
        if !inventory.warnings.is_empty() {
            self.message = inventory.warnings.join("\n");
        }
        self.table.select(if self.keys.is_empty() {
            None
        } else {
            Some(self.table.selected().unwrap_or(0).min(self.keys.len() - 1))
        });
        Ok(())
    }

    fn input(&mut self, event: KeyEvent) -> Result<(bool, Option<Work>)> {
        if event.code == KeyCode::Char('c') && event.modifiers.contains(KeyModifiers::CONTROL) {
            return Ok((false, None));
        }
        let can_switch = self.wizard.is_none()
            && (!self.hosts_active || self.hosts.as_ref().is_some_and(|h| !h.is_editing()));
        if can_switch {
            match event.code {
                KeyCode::Char('k' | 'K') => {
                    self.hosts_active = false;
                    self.refresh(&self.dir.clone())?;
                    return Ok((true, None));
                }
                KeyCode::Char('h' | 'H') => {
                    if !self.hosts_active {
                        self.open_hosts()?;
                    }
                    return Ok((true, None));
                }
                KeyCode::Tab | KeyCode::BackTab => {
                    if self.hosts_active {
                        self.hosts_active = false;
                        self.refresh(&self.dir.clone())?;
                    } else {
                        self.open_hosts()?;
                    }
                    return Ok((true, None));
                }
                KeyCode::Char('q' | 'Q') => return Ok((false, None)),
                _ => {}
            }
        }
        if self.hosts_active {
            let hosts = self
                .hosts
                .as_mut()
                .context("Hosts tab is unavailable. Restart skm and try again.")?;
            if !hosts.is_editing() && event.code == KeyCode::Enter {
                match hosts.connection_alias() {
                    Ok(alias) => return Ok((true, Some(Work::Connect(alias)))),
                    Err(e) => hosts.message = format!("{e:#}"),
                }
                return Ok((true, None));
            }
            match hosts.input(&self.dir, event) {
                Ok(false) => {
                    self.hosts_active = false;
                    self.refresh(&self.dir.clone())?;
                }
                Ok(true) => {}
                Err(e) => hosts.message = format!("{e:#}"),
            }
            return Ok((true, None));
        }
        if let Some(wizard) = self.wizard.as_mut() {
            match wizard {
                Wizard::Parameters(form) => match event.code {
                    KeyCode::Esc => self.wizard = None,
                    KeyCode::Tab | KeyCode::BackTab => form.field = 1 - form.field,
                    KeyCode::Left | KeyCode::Right | KeyCode::Char(' ') if form.field == 0 => {
                        form.rsa = !form.rsa;
                        if form.filename == "id_ed25519" || form.filename == "id_rsa" {
                            form.filename = if form.rsa { "id_rsa" } else { "id_ed25519" }.into();
                        }
                    }
                    KeyCode::Enter => {
                        keys::key_path(Path::new("."), &form.filename)?;
                        return Ok((
                            true,
                            Some(Work::Generate {
                                name: form.filename.clone(),
                                rsa: form.rsa,
                            }),
                        ));
                    }
                    KeyCode::Char(c) if form.field == 1 && !c.is_control() => form.filename.push(c),
                    KeyCode::Backspace if form.field == 1 => {
                        form.filename.pop();
                    }
                    _ => {}
                },
                Wizard::Register(key) => match event.code {
                    KeyCode::Char('y' | 'Y') => {
                        self.wizard = Some(Wizard::Server {
                            key: key.clone(),
                            remote: String::new(),
                            port: "22".into(),
                            field: 0,
                        });
                        self.message.clear();
                    }
                    KeyCode::Enter | KeyCode::Esc | KeyCode::Char('n' | 'N') => {
                        return Ok((
                            true,
                            Some(Work::Finish {
                                key: key.clone(),
                                remote: None,
                            }),
                        ));
                    }
                    _ => {}
                },
                Wizard::Server {
                    key,
                    remote,
                    port,
                    field,
                } => match event.code {
                    KeyCode::Esc => self.wizard = Some(Wizard::Register(key.clone())),
                    KeyCode::Tab | KeyCode::BackTab => *field = 1 - *field,
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
                        return Ok((
                            true,
                            Some(Work::Finish {
                                key: key.clone(),
                                remote: Some((remote.clone(), port)),
                            }),
                        ));
                    }
                    _ => {}
                },
            }
            return Ok((true, None));
        }
        match event.code {
            KeyCode::Char('q' | 'Q') | KeyCode::Esc => return Ok((false, None)),
            KeyCode::Char('h' | 'H') => self.open_hosts()?,
            KeyCode::Char('g' | 'G') => {
                self.wizard = Some(Wizard::Parameters(Form {
                    filename: "id_ed25519".into(),
                    ..Form::default()
                }));
                self.message.clear();
            }
            KeyCode::Char('d' | 'D') => return Ok((true, Some(Work::Doctor))),
            KeyCode::Char('l' | 'L') | KeyCode::Enter => {
                if let Some(key) = self.table.selected().and_then(|i| self.keys.get(i)) {
                    return Ok((true, Some(Work::Load(key.clone()))));
                }
                self.message = "No key selected. Press G to create a key.".into();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                if !self.keys.is_empty() {
                    self.table.select(Some(
                        (self.table.selected().unwrap_or(0) + 1).min(self.keys.len() - 1),
                    ));
                }
            }
            KeyCode::Up => self
                .table
                .select(self.table.selected().map(|i| i.saturating_sub(1))),
            _ => {}
        }
        Ok((true, None))
    }

    fn open_hosts(&mut self) -> Result<()> {
        if let Some(hosts) = &mut self.hosts {
            hosts.refresh(&self.dir)?;
        } else {
            self.hosts = Some(hosts::Hosts::new(&self.dir)?);
        }
        self.hosts_active = true;
        Ok(())
    }

    fn work(&mut self, dir: &Path, work: Work) {
        let result = match work {
            Work::Connect(alias) => {
                let result = crate::process::command("ssh").and_then(|mut command| {
                    command.arg("--").arg(&alias);
                    crate::process::interactive(
                        &mut command,
                        &format!("connect to SSH alias {alias}"),
                    )
                });
                if let Some(hosts) = self.hosts.as_mut() {
                    hosts.message = match result {
                        Ok(()) => format!("SSH session to {alias} ended."),
                        Err(e) => format!("{e:#} Check the alias settings, network access, and login details, then press Enter to retry."),
                    };
                }
                return;
            }
            Work::Generate { name, rsa } => {
                println!("OpenSSH will ask for an optional passphrase to protect the private key. Input is hidden.");
                keys::generate(dir, &name, rsa).map(|key| {
                    self.message = format!(
                        "Created {}. Fingerprint: {}",
                        key.path.display(),
                        key.fingerprint
                    );
                    self.wizard = Some(Wizard::Register(key));
                })
            }
            Work::Load(key) => agent::ensure(dir)
                .and_then(|mut agent| agent::load(&mut agent, &key))
                .map(|loaded| {
                    self.message = format!(
                        "{}: {}",
                        key.path.display(),
                        if loaded {
                            "Loaded in agent"
                        } else {
                            "Already loaded"
                        }
                    );
                }),
            Work::Doctor => agent::ensure(dir).map(|agent| {
                self.message = format!(
                    "SSH agent ready. Connection: {}. Shell connection: {}",
                    agent.socket.display(),
                    dir.join("agent.sock").display()
                )
            }),
            Work::Finish { key, remote } => {
                let registration = remote
                    .as_ref()
                    .map(|(remote, port)| keys::deploy(&key, remote, *port))
                    .transpose();
                println!("Step 3 of 3: loading the key into the SSH agent.");
                let loaded = agent::ensure(dir).and_then(|mut agent| agent::load(&mut agent, &key));
                let mut messages = vec![format!("Key saved: {}", key.path.display())];
                match registration {
                    Ok(Some(())) => messages.push("Public key added to the server.".into()),
                    Ok(None) => {},
                    Err(e) => messages.push(format!("Remote setup failed: {e:#}. Retry with ssh-copy-id -i <key>.pub user@hostname.")),
                }
                match loaded {
                    Ok(_) => messages.push("Loaded in agent.".into()),
                    Err(e) => messages.push(format!(
                        "Agent loading failed: {e:#}. Retry with skm load <filename>."
                    )),
                }
                self.message = messages.join("\n");
                self.wizard = None;
                Ok(())
            }
        };
        if let Err(e) = result {
            self.message = format!("{e:#}");
        }
        if let Err(e) = self.refresh(dir) {
            self.message = format!("{e:#}");
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        if let Some(hosts) = self.hosts.as_mut().filter(|_| self.hosts_active) {
            hosts.draw(frame);
            return;
        }
        let area = frame.area();
        if area.width < 60 || area.height < 18 {
            frame.render_widget(
                Paragraph::new(
                    "Make the terminal at least 60 columns wide and 18 rows tall. Press Q to quit.",
                )
                .wrap(Wrap { trim: false }),
                area,
            );
            return;
        }
        let areas = Layout::vertical([
            Constraint::Length(3),
            Constraint::Min(4),
            Constraint::Length(5),
            Constraint::Length(2),
        ])
        .split(area);
        let status = if let Some(agent) = &self.agent {
            format!(
                "ssh-key-man | Agent ready | {} loaded",
                agent.fingerprints.len()
            )
        } else {
            "ssh-key-man | Agent unavailable | Press D to find or start one".into()
        };
        frame.render_widget(
            Paragraph::new(status).block(Block::default().borders(Borders::BOTTOM)),
            areas[0],
        );
        draw_tabs(frame, areas[0], 0);
        let sections = Layout::horizontal([Constraint::Percentage(35), Constraint::Percentage(65)])
            .split(areas[1]);
        if self.keys.is_empty() {
            frame.render_widget(
                Paragraph::new("No keys found.\nPress G to create a key.")
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
                .row_highlight_style(
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD),
                )
                .highlight_symbol("> ");
            frame.render_stateful_widget(table, sections[0], &mut self.table);
        }
        let details = self
            .table
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
        frame.render_widget(
            Paragraph::new(self.message.as_str())
                .wrap(Wrap { trim: false })
                .block(Block::default().title(" Status ").borders(Borders::TOP)),
            areas[2],
        );
        frame.render_widget(
            Paragraph::new(
                "Enter: load selected key into agent\n[G] Create key [L] Load key [D] Check agent [Q] Quit",
            ),
            areas[3],
        );
        if let Some(wizard) = &self.wizard {
            let popup = Rect::new(
                area.x + (area.width.saturating_sub(76)) / 2,
                area.y + (area.height.saturating_sub(16)) / 2,
                area.width.min(76),
                area.height.min(16),
            );
            frame.render_widget(Clear, popup);
            let (title, text) = match wizard {
                Wizard::Parameters(form) => (" Create key: step 1 of 3 ", format!("{} Key type: {}\n\n{} Filename: {}\n\nA passphrase protects your private key. OpenSSH will ask for one next.\n\nTab: choose field | Left/Right/Space: change key type\nEnter: create key | Esc: cancel\n\n{}", if form.field == 0 { ">" } else { " " }, if form.rsa { "RSA 4096" } else { "Ed25519" }, if form.field == 1 { ">" } else { " " }, form.filename, self.message)),
                Wizard::Register(_) => (" Add public key: step 2 of 3 ", format!("Add this public key to a server now? [y/N]\n\nY: enter server details\nN / Enter / Esc: skip server setup and load into agent\n\n{}", self.message)),
                Wizard::Server { remote, port, field, .. } => (" Server setup: step 2 of 3 ", format!("{} Server (user@hostname): {}\n\n{} SSH port: {}\n\nTab: choose field | Enter: add public key and load into agent\nEsc: back\n\nOpenSSH will ask you to verify the host and sign in.\n\n{}", if *field == 0 { ">" } else { " " }, remote, if *field == 1 { ">" } else { " " }, port, self.message)),
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
}

fn draw_tabs(frame: &mut Frame, header: Rect, selected: usize) {
    let row = Rect::new(header.x, header.y + 1, header.width, 1);
    let parts = Layout::horizontal([Constraint::Length(30), Constraint::Min(0)]).split(row);
    frame.render_widget(
        Tabs::new(["[K] Keys", "[H] Hosts"])
            .select(selected)
            .highlight_style(
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            ),
        parts[0],
    );
    frame.render_widget(Paragraph::new("Tab: switch tabs"), parts[1]);
}

struct Screen;
impl Screen {
    fn enter() -> Result<Terminal<CrosstermBackend<Stdout>>> {
        enable_raw_mode()
            .context("Could not enable terminal input. Run skm in an interactive terminal.")?;
        execute!(io::stdout(), EnterAlternateScreen)?;
        let mut terminal = Terminal::new(CrosstermBackend::new(io::stdout()))?;
        terminal.clear()?;
        Ok(terminal)
    }
    fn restore() {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        Self::restore();
    }
}

pub fn run(dir: &Path) -> Result<()> {
    event_loop(App::new(dir)?, dir)
}

pub fn run_hosts(dir: &Path) -> Result<()> {
    event_loop(
        App {
            dir: dir.to_path_buf(),
            hosts: Some(hosts::Hosts::new(dir)?),
            hosts_active: true,
            ..App::default()
        },
        dir,
    )
}

fn event_loop(mut app: App, dir: &Path) -> Result<()> {
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        Screen::restore();
        previous_hook(info);
    }));
    let _screen = Screen;
    let mut terminal = Screen::enter()?;
    loop {
        terminal.draw(|frame| app.draw(frame))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        if let Event::Key(key) = event::read()? {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match app.input(key) {
                Ok((false, _)) => break,
                Ok((_, Some(work))) => {
                    Screen::restore();
                    app.work(dir, work);
                    terminal = Screen::enter()?;
                }
                Ok(_) => {}
                Err(e) => app.message = format!("{e:#}"),
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    #[test]
    fn page_shortcuts_and_enter_actions_follow_the_active_page() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config"),
            "Host work\n HostName example.invalid\nHost *\n Port 22\n",
        )
        .unwrap();
        let mut app = App::new(dir.path()).unwrap();
        app.input(press(KeyCode::Char('H'))).unwrap();
        app.input(press(KeyCode::Char('h'))).unwrap();
        assert!(app.hosts_active);
        assert!(
            matches!(app.input(press(KeyCode::Enter)).unwrap().1, Some(Work::Connect(alias)) if alias == "work")
        );
        app.input(press(KeyCode::Down)).unwrap();
        assert!(app.input(press(KeyCode::Enter)).unwrap().1.is_none());
        assert!(app.hosts.as_ref().unwrap().message.contains("single alias"));
        app.input(press(KeyCode::Char('a'))).unwrap();
        app.input(press(KeyCode::Char('K'))).unwrap();
        assert!(app.hosts_active && app.hosts.as_ref().unwrap().is_editing());
        app.input(press(KeyCode::Esc)).unwrap();
        app.input(press(KeyCode::Char('K'))).unwrap();
        app.input(press(KeyCode::Char('k'))).unwrap();
        assert!(!app.hosts_active);
        app.keys.push(Key {
            path: dir.path().join("id_test"),
            algorithm: "ssh-ed25519".into(),
            fingerprint: "SHA256:test".into(),
            private: true,
        });
        app.table.select(Some(0));
        assert!(matches!(
            app.input(press(KeyCode::Enter)).unwrap().1,
            Some(Work::Load(_))
        ));
    }

    #[test]
    fn key_sections_show_the_selected_key_and_enter_hint() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path()).unwrap();
        for name in ["first", "second"] {
            app.keys.push(Key {
                path: dir.path().join(name),
                algorithm: "ssh-ed25519".into(),
                fingerprint: format!("SHA256:{name}"),
                private: true,
            });
        }
        app.table.select(Some(0));
        for (width, height) in [(60, 18), (110, 30)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| app.draw(f)).unwrap();
            let screen: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            for label in [
                "Keys in ~/.ssh/",
                "Key details",
                "Status",
                "Enter: load selected key into agent",
                "Fingerprint:",
            ] {
                assert!(screen.contains(label), "Missing {label}");
            }
        }
        app.input(press(KeyCode::Down)).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(screen.contains("SHA256:second"));
    }
    #[test]
    fn tabs_switch_preserve_selection_and_leave_forms_in_control() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("config"),
            "Host first\n HostName first.invalid\nHost second\n HostName second.invalid\n",
        )
        .unwrap();
        let mut app = App::new(dir.path()).unwrap();
        app.input(press(KeyCode::Tab)).unwrap();
        assert!(app.hosts_active);
        app.input(press(KeyCode::Down)).unwrap();
        app.input(press(KeyCode::BackTab)).unwrap();
        assert!(!app.hosts_active);
        app.input(press(KeyCode::Tab)).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        let screen: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(screen.contains("Alias: second"));
        app.input(press(KeyCode::Char('e'))).unwrap();
        assert!(app.hosts.as_ref().unwrap().is_editing());
        app.input(press(KeyCode::Tab)).unwrap();
        assert!(app.hosts_active);
        app.input(press(KeyCode::Esc)).unwrap();
        app.input(press(KeyCode::Char('d'))).unwrap();
        app.input(press(KeyCode::Tab)).unwrap();
        assert!(app.hosts_active);
        app.input(press(KeyCode::Esc)).unwrap();
        app.input(press(KeyCode::Tab)).unwrap();
        app.input(press(KeyCode::Char('g'))).unwrap();
        app.input(press(KeyCode::Tab)).unwrap();
        assert!(!app.hosts_active);
        assert!(matches!(
            app.wizard,
            Some(Wizard::Parameters(Form { field: 1, .. }))
        ));
        app.input(press(KeyCode::Esc)).unwrap();
        app.input(press(KeyCode::Tab)).unwrap();
        assert!(!app.input(press(KeyCode::Char('q'))).unwrap().0);
    }

    #[test]
    fn both_tabs_are_visible_and_active_tab_is_highlighted() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path()).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        for active in ["Keys", "Hosts"] {
            terminal.draw(|f| app.draw(f)).unwrap();
            let buffer = terminal.backend().buffer();
            let row: String = (0..80).map(|x| buffer[(x, 1)].symbol()).collect();
            assert!(
                row.contains("Keys") && row.contains("Hosts") && row.contains("Tab: switch tabs")
            );
            let column = row.find(active).unwrap() as u16;
            assert_eq!(buffer[(column, 1)].bg, Color::Cyan);
            app.input(press(KeyCode::Tab)).unwrap();
        }
    }

    #[test]
    fn empty_screen_renders_and_wizard_can_be_cancelled() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path()).unwrap();
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
        app.input(press(KeyCode::Char('g'))).unwrap();
        assert!(matches!(app.wizard, Some(Wizard::Parameters(_))));
        terminal.draw(|f| app.draw(f)).unwrap();
        app.input(press(KeyCode::Esc)).unwrap();
        assert!(app.wizard.is_none());
        assert!(!app.input(press(KeyCode::Char('q'))).unwrap().0);
    }
    #[test]
    fn generation_and_remote_validation_follow_wizard_steps() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path()).unwrap();
        app.input(press(KeyCode::Char('g'))).unwrap();
        assert!(matches!(
            app.input(press(KeyCode::Enter)).unwrap().1,
            Some(Work::Generate { rsa: false, .. })
        ));
        let key = Key {
            path: dir.path().join("id_test"),
            algorithm: "ssh-ed25519".into(),
            fingerprint: "SHA256:test".into(),
            private: true,
        };
        app.wizard = Some(Wizard::Register(key.clone()));
        assert!(matches!(
            app.input(press(KeyCode::Enter)).unwrap().1,
            Some(Work::Finish { remote: None, .. })
        ));
        app.input(press(KeyCode::Char('y'))).unwrap();
        assert!(matches!(app.wizard, Some(Wizard::Server { .. })));
        assert!(app.input(press(KeyCode::Enter)).is_err());
        app.wizard = Some(Wizard::Server {
            key,
            remote: "user@example.com".into(),
            port: "22".into(),
            field: 0,
        });
        assert!(matches!(
            app.input(press(KeyCode::Enter)).unwrap().1,
            Some(Work::Finish {
                remote: Some((_, 22)),
                ..
            })
        ));
    }
    #[test]
    fn small_terminal_does_not_panic() {
        let dir = tempfile::tempdir().unwrap();
        let mut app = App::new(dir.path()).unwrap();
        for (width, height) in [(1, 1), (40, 8), (60, 18), (120, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|f| app.draw(f)).unwrap();
        }
    }
}
