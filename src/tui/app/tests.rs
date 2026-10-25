use super::*;
use crate::tui::wizard::KeyForm;
use ratatui::{backend::TestBackend, style::Modifier, Terminal};
fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn settle(app: &mut App) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while app.refresh.is_some() {
        assert!(std::time::Instant::now() < deadline);
        app.poll_refresh();
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

fn keys_app(dir: &Path) -> App {
    let mut app = App::new(dir).unwrap();
    app.handle_key(press(KeyCode::Char('k'))).unwrap();
    settle(&mut app);
    app
}

#[test]
fn hosts_is_the_default_and_new_shortcut_follows_the_page() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = App::new(dir.path()).unwrap();
    assert!(app.page == Page::Hosts);
    app.handle_key(press(KeyCode::Char('N'))).unwrap();
    assert!(app.hosts.is_editing());
    app.handle_key(press(KeyCode::Esc)).unwrap();
    app.handle_key(press(KeyCode::Left)).unwrap();
    settle(&mut app);
    assert!(app.page == Page::Keys);
    assert_eq!(app.focus, Focus::List);
    app.handle_key(press(KeyCode::Char('n'))).unwrap();
    assert!(matches!(app.wizard, Some(KeyWizard::Parameters(_))));
    app.handle_key(press(KeyCode::Esc)).unwrap();
    app.handle_key(press(KeyCode::Right)).unwrap();
    settle(&mut app);
    assert!(app.page == Page::Hosts);
    assert_eq!(app.focus, Focus::List);
    app.handle_key(press(KeyCode::Tab)).unwrap();
    assert!(app.page == Page::Hosts);
}

#[test]
fn page_shortcuts_and_enter_actions_follow_the_active_page() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config"),
        "Host work\n HostName example.invalid\nHost *\n Port 22\n",
    )
    .unwrap();
    let mut app = keys_app(dir.path());
    app.handle_key(press(KeyCode::Char('H'))).unwrap();
    app.handle_key(press(KeyCode::Char('h'))).unwrap();
    assert!(app.page == Page::Hosts);
    assert!(
        matches!(app.handle_key(press(KeyCode::Enter)).unwrap(), InputResult::Run(Operation::Connect(alias)) if alias == "work")
    );
    app.handle_key(press(KeyCode::Down)).unwrap();
    assert!(matches!(
        app.handle_key(press(KeyCode::Enter)).unwrap(),
        InputResult::Continue
    ));
    assert!(app.hosts.message.text.contains("single alias"));
    app.handle_key(press(KeyCode::Enter)).unwrap();
    app.handle_key(press(KeyCode::Char('a'))).unwrap();
    app.handle_key(press(KeyCode::Char('K'))).unwrap();
    settle(&mut app);
    assert!(app.page == Page::Hosts && app.hosts.is_editing());
    app.handle_key(press(KeyCode::Esc)).unwrap();
    app.handle_key(press(KeyCode::Char('K'))).unwrap();
    settle(&mut app);
    app.handle_key(press(KeyCode::Char('k'))).unwrap();
    settle(&mut app);
    assert!(app.page == Page::Keys);
    app.keys.push(Key {
        path: dir.path().join("id_test"),
        algorithm: "ssh-ed25519".into(),
        fingerprint: "SHA256:test".into(),
        private: true,
    });
    app.key_selection.select(Some(0));
    assert!(matches!(
        app.handle_key(press(KeyCode::Char('l'))).unwrap(),
        InputResult::Continue
    ));
    assert!(matches!(
        app.handle_key(press(KeyCode::Char('L'))).unwrap(),
        InputResult::Continue
    ));
    assert!(matches!(
        app.handle_key(press(KeyCode::Enter)).unwrap(),
        InputResult::Run(Operation::Load(_))
    ));
}

#[test]
fn key_sections_show_the_selected_key_and_enter_hint() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = keys_app(dir.path());
    for name in ["first", "second"] {
        app.keys.push(Key {
            path: dir.path().join(name),
            algorithm: "ssh-ed25519".into(),
            fingerprint: format!("SHA256:{name}"),
            private: true,
        });
    }
    app.agent = Some(crate::agent::Agent {
        socket: dir.path().join("test.sock"),
        fingerprints: ["SHA256:first".to_owned()].into_iter().collect(),
    });
    app.key_selection.select(Some(0));
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
            "Enter: load key and close skm",
            "Fingerprint:",
        ] {
            assert!(screen.contains(label), "Missing {label}");
        }
        let buffer = terminal.backend().buffer();
        let marker = buffer
            .content
            .iter()
            .position(|cell| cell.symbol() == "●")
            .unwrap();
        let x = marker as u16 % width;
        let y = marker as u16 / width;
        assert_eq!(buffer[(x + 1, y)].symbol(), "│");
        assert_eq!(buffer[(x, y + 1)].symbol(), " ");
        assert_eq!(
            buffer[(x, y)].fg,
            crate::theme::Tone::Success
                .style()
                .fg
                .unwrap_or(ratatui::style::Color::Reset)
        );
        assert!(!buffer[(x, y)].modifier.contains(Modifier::REVERSED));
        let list: String = (0..height)
            .flat_map(|row| (0..=x).map(move |column| buffer[(column, row)].symbol()))
            .collect();
        assert!(!list.contains("Loaded in agent"));
        assert!(!list.contains("Not loaded"));
        assert!(screen.contains("Status: Loaded in agent"));
        assert!(!screen.contains(" Status "));
        assert!(!screen.contains("[L]"));
        assert!(!screen.contains("Esc: quit"));
        assert!(!screen.contains("Up/Down:"));
    }
    app.handle_key(press(KeyCode::Down)).unwrap();
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
    assert!(screen.contains("Status: Not loaded"));
}
#[test]
fn page_pickers_preserve_selection_and_leave_forms_in_control() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("config"),
        "Host first\n HostName first.invalid\nHost second\n HostName second.invalid\n",
    )
    .unwrap();
    let mut app = keys_app(dir.path());
    app.handle_key(press(KeyCode::Tab)).unwrap();
    assert!(app.page == Page::Keys);
    app.handle_key(press(KeyCode::Left)).unwrap();
    settle(&mut app);
    assert_eq!(app.focus, Focus::List);
    assert!(app.page == Page::Hosts);
    app.handle_key(press(KeyCode::Enter)).unwrap();
    app.handle_key(press(KeyCode::Down)).unwrap();
    app.handle_key(press(KeyCode::Char('k'))).unwrap();
    settle(&mut app);
    app.handle_key(press(KeyCode::Char('h'))).unwrap();
    assert_eq!(app.hosts.connection_alias().unwrap(), "second");
    app.handle_key(press(KeyCode::Char('e'))).unwrap();
    app.handle_key(press(KeyCode::Down)).unwrap();
    app.handle_key(press(KeyCode::Tab)).unwrap();
    assert!(app.page == Page::Hosts && app.hosts.is_editing());
    app.handle_key(press(KeyCode::Esc)).unwrap();
    app.handle_key(press(KeyCode::Char('d'))).unwrap();
    app.handle_key(press(KeyCode::Left)).unwrap();
    settle(&mut app);
    assert!(app.hosts.is_editing());
    app.handle_key(press(KeyCode::Esc)).unwrap();
    app.handle_key(press(KeyCode::Char('k'))).unwrap();
    settle(&mut app);
    app.handle_key(press(KeyCode::Char('g'))).unwrap();
    app.handle_key(press(KeyCode::Down)).unwrap();
    assert!(matches!(
        app.wizard,
        Some(KeyWizard::Parameters(KeyForm { field: 1, .. }))
    ));
    app.handle_key(press(KeyCode::Esc)).unwrap();
    assert!(matches!(
        app.handle_key(press(KeyCode::Esc)).unwrap(),
        InputResult::Quit
    ));
}

#[test]
fn both_tabs_are_visible_and_active_tab_is_highlighted() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = keys_app(dir.path());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    app.focus = Focus::Pages;
    for active in ["Keys", "Hosts"] {
        terminal.draw(|f| app.draw(f)).unwrap();
        let buffer = terminal.backend().buffer();
        let row: String = (0..80)
            .map(|x| buffer[(x, crate::tui::components::HEADER_HEIGHT - 3)].symbol())
            .collect();
        assert!(
            row.contains("Keys")
                && row.contains("Hosts")
                && !row.contains("Left/Right")
                && !row.contains("Esc:")
        );
        assert!(row.ends_with(concat!("v", env!("CARGO_PKG_VERSION"))));
        let gutter: String = (0..80)
            .map(|x| buffer[(x, crate::tui::components::HEADER_HEIGHT - 1)].symbol())
            .collect();
        assert!(gutter.trim().is_empty());
        let column = row.find(active).unwrap() as u16;
        assert!(buffer[(column, crate::tui::components::HEADER_HEIGHT - 3)]
            .modifier
            .contains(Modifier::REVERSED));
        app.handle_key(press(KeyCode::Right)).unwrap();
        settle(&mut app);
    }
}

#[test]
fn arrow_action_picker_runs_actions_and_notices_require_acknowledgment() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = keys_app(dir.path());
    app.keys.push(Key {
        path: dir.path().join("id_test"),
        algorithm: "ssh-ed25519".into(),
        fingerprint: "SHA256:test".into(),
        private: true,
    });
    app.key_selection.select(Some(0));
    app.handle_key(press(KeyCode::Down)).unwrap();
    assert_eq!(app.focus, Focus::Actions);
    app.handle_key(press(KeyCode::Right)).unwrap();
    settle(&mut app);
    app.handle_key(press(KeyCode::Right)).unwrap();
    settle(&mut app);
    assert_eq!(app.action_index, 0);
    app.handle_key(press(KeyCode::Left)).unwrap();
    settle(&mut app);
    assert!(matches!(
        app.handle_key(press(KeyCode::Enter)).unwrap(),
        InputResult::Continue
    ));
    assert!(matches!(
        app.wizard,
        Some(KeyWizard::Delete { confirm: false, .. })
    ));
    app.handle_key(press(KeyCode::Enter)).unwrap();
    assert!(app.wizard.is_none());
    app.message
        .set("An action failed. Try again.", Tone::Danger);
    app.handle_key(press(KeyCode::Char('h'))).unwrap();
    assert!(app.page == Page::Keys);
    app.handle_key(press(KeyCode::Enter)).unwrap();
    assert!(app.message.is_empty());
    app.handle_key(press(KeyCode::Char('h'))).unwrap();
    app.handle_key(press(KeyCode::Down)).unwrap();
    app.handle_key(press(KeyCode::Enter)).unwrap();
    assert!(app.hosts.is_editing());
    app.handle_key(press(KeyCode::Esc)).unwrap();
    assert!(matches!(
        app.handle_key(press(KeyCode::Esc)).unwrap(),
        InputResult::Quit
    ));
}

#[test]
fn deletion_requires_explicit_confirmation() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = keys_app(dir.path());
    app.keys.push(Key {
        path: dir.path().join("id_test"),
        algorithm: "ssh-ed25519".into(),
        fingerprint: "SHA256:test".into(),
        private: true,
    });
    app.key_selection.select(Some(0));
    app.handle_key(press(KeyCode::Char('d'))).unwrap();
    assert!(matches!(
        app.handle_key(press(KeyCode::Enter)).unwrap(),
        InputResult::Continue
    ));
    assert!(app.wizard.is_none());
    app.handle_key(press(KeyCode::Char('d'))).unwrap();
    app.handle_key(press(KeyCode::Down)).unwrap();
    assert!(
        matches!(app.handle_key(press(KeyCode::Enter)).unwrap(), InputResult::Run(Operation::Delete(key)) if key.path.ends_with("id_test"))
    );
}

#[test]
fn empty_screen_renders_and_wizard_can_be_cancelled() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = keys_app(dir.path());
    let mut terminal = Terminal::new(TestBackend::new(80, 24)).unwrap();
    terminal.draw(|f| app.draw(f)).unwrap();
    app.handle_key(press(KeyCode::Char('g'))).unwrap();
    assert!(matches!(app.wizard, Some(KeyWizard::Parameters(_))));
    terminal.draw(|f| app.draw(f)).unwrap();
    app.handle_key(press(KeyCode::Esc)).unwrap();
    assert!(app.wizard.is_none());
    assert!(matches!(
        app.handle_key(press(KeyCode::Esc)).unwrap(),
        InputResult::Quit
    ));
}
#[test]
fn generation_and_remote_validation_follow_wizard_steps() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = keys_app(dir.path());
    app.handle_key(press(KeyCode::Char('g'))).unwrap();
    assert!(matches!(
        app.handle_key(press(KeyCode::Enter)).unwrap(),
        InputResult::Run(Operation::Generate { rsa: false, .. })
    ));
    let key = Key {
        path: dir.path().join("id_test"),
        algorithm: "ssh-ed25519".into(),
        fingerprint: "SHA256:test".into(),
        private: true,
    };
    app.wizard = Some(KeyWizard::register(key.clone()));
    assert!(matches!(
        app.handle_key(press(KeyCode::Enter)).unwrap(),
        InputResult::Run(Operation::Finish { remote: None, .. })
    ));
    app.handle_key(press(KeyCode::Down)).unwrap();
    app.handle_key(press(KeyCode::Enter)).unwrap();
    assert!(matches!(app.wizard, Some(KeyWizard::Server { .. })));
    assert!(app.handle_key(press(KeyCode::Enter)).is_err());
    app.wizard = Some(KeyWizard::Server {
        key,
        remote: "user@example.com".into(),
        port: "22".into(),
        field: 0,
        previous_choice: false,
    });
    assert!(matches!(
        app.handle_key(press(KeyCode::Enter)).unwrap(),
        InputResult::Run(Operation::Finish {
            remote: Some((_, 22)),
            ..
        })
    ));
}
#[test]
fn small_terminal_does_not_panic() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = keys_app(dir.path());
    for (width, height) in [(1, 1), (40, 8), (60, 18), (120, 40)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| app.draw(f)).unwrap();
    }
}

#[test]
fn hidden_host_confirmation_cannot_delete_after_resize() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("config");
    let original = "Host work\n HostName example.invalid\n";
    std::fs::write(&path, original).unwrap();
    let mut app = App::new(dir.path()).unwrap();
    app.handle_key(press(KeyCode::Char('d'))).unwrap();
    let mut terminal = Terminal::new(TestBackend::new(40, 8)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    for key in [KeyCode::Char('y'), KeyCode::Enter] {
        assert!(matches!(
            app.handle_key(press(key)).unwrap(),
            InputResult::Continue
        ));
    }
    assert_eq!(std::fs::read_to_string(&path).unwrap(), original);
}

#[test]
fn long_path_keeps_full_fingerprint_visible_at_minimum_size() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = keys_app(dir.path());
    let fingerprint = format!("SHA256:{}+/x", "a".repeat(40));
    app.keys.push(Key {
        path: dir
            .path()
            .join("a-long-but-valid-subdirectory".repeat(4))
            .join("id_production_host"),
        algorithm: "ssh-ed25519".into(),
        fingerprint: fingerprint.clone(),
        private: true,
    });
    app.key_selection.select(Some(0));
    let mut terminal = Terminal::new(TestBackend::new(60, 18)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let visible: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .flat_map(|cell| cell.symbol().chars())
        .filter(|character| character.is_ascii_alphanumeric() || ":+/".contains(*character))
        .collect();
    assert!(visible.contains(&fingerprint));
}

#[test]
fn public_only_key_does_not_offer_or_run_a_load() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = keys_app(dir.path());
    app.keys.push(Key {
        path: dir.path().join("id_work.pub"),
        algorithm: "ssh-ed25519".into(),
        fingerprint: "SHA256:public".into(),
        private: false,
    });
    app.key_selection.select(Some(0));
    let mut terminal = Terminal::new(TestBackend::new(110, 30)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let visible: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(!visible.contains("Enter: load key"));
    assert!(matches!(
        app.handle_key(press(KeyCode::Enter)).unwrap(),
        InputResult::Continue
    ));
}

#[test]
fn agent_refresh_shows_busy_state_and_keeps_quit_responsive() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = keys_app(dir.path());
    let (sender, receiver) = std::sync::mpsc::channel();
    app.refresh = Some(receiver);
    let mut terminal = Terminal::new(TestBackend::new(60, 18)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(screen.contains("Reading keys and checking the SSH agent..."));
    assert!(!screen.contains("Enter: load"));
    app.handle_key(press(KeyCode::Char('n'))).unwrap();
    assert!(app.wizard.is_none());
    assert!(matches!(
        app.handle_key(press(KeyCode::Esc)).unwrap(),
        InputResult::Quit
    ));
    app.message.set("Created a key.", Tone::Success);
    sender
        .send(Ok(refresh::Snapshot {
            inventory: keys::Inventory {
                keys: Vec::new(),
                warnings: Vec::new(),
            },
            agent: None,
        }))
        .unwrap();
    app.poll_refresh();
    assert_eq!(app.message.tone, Tone::Success);
}

#[test]
fn warning_popup_keeps_instructions_visible_and_reaches_the_last_warning() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = keys_app(dir.path());
    app.message.set(
        (0..30)
            .map(|index| {
                format!("Warning {index}: check this public key file and restore its format.")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        Tone::Warning,
    );
    let mut terminal = Terminal::new(TestBackend::new(60, 18)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let screen = |terminal: &Terminal<TestBackend>| {
        terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect::<String>()
    };
    assert!(screen(&terminal).contains("Enter: continue | Esc: quit"));
    for _ in 0..30 {
        app.handle_key(press(KeyCode::PageDown)).unwrap();
    }
    terminal.draw(|frame| app.draw(frame)).unwrap();
    assert!(screen(&terminal).contains("Warning 29:"));
    assert!(screen(&terminal).contains("Enter: continue | Esc: quit"));
    app.handle_key(press(KeyCode::Enter)).unwrap();
    assert!(app.message.is_empty());
}

#[test]
fn hidden_key_confirmation_cannot_run_and_long_details_can_scroll() {
    let dir = tempfile::tempdir().unwrap();
    let mut app = keys_app(dir.path());
    app.keys.push(Key {
        path: dir
            .path()
            .join("long-directory/".repeat(30))
            .join("the-final-key-name"),
        algorithm: "ssh-ed25519".into(),
        fingerprint: format!("SHA256:{}", "b".repeat(43)),
        private: true,
    });
    app.key_selection.select(Some(0));
    let mut terminal = Terminal::new(TestBackend::new(60, 18)).unwrap();
    terminal.draw(|frame| app.draw(frame)).unwrap();
    for _ in 0..30 {
        app.handle_key(press(KeyCode::PageDown)).unwrap();
    }
    terminal.draw(|frame| app.draw(frame)).unwrap();
    let screen: String = terminal
        .backend()
        .buffer()
        .content
        .iter()
        .map(|cell| cell.symbol())
        .collect();
    assert!(screen.contains("the-final-key-name"));
    app.handle_key(press(KeyCode::Char('d'))).unwrap();
    let mut small = Terminal::new(TestBackend::new(40, 8)).unwrap();
    small.draw(|frame| app.draw(frame)).unwrap();
    for key in [KeyCode::Char('y'), KeyCode::Down, KeyCode::Enter] {
        assert!(matches!(
            app.handle_key(press(key)).unwrap(),
            InputResult::Continue
        ));
    }
    assert!(matches!(
        app.wizard,
        Some(KeyWizard::Delete { confirm: false, .. })
    ));
}
