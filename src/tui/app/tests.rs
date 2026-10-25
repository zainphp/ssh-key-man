use super::*;
use crate::tui::wizard::KeyForm;
use ratatui::{backend::TestBackend, style::Modifier, Terminal};
fn press(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn keys_app(dir: &Path) -> App {
    let mut app = App::new(dir).unwrap();
    app.handle_key(press(KeyCode::Char('k'))).unwrap();
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
    assert!(app.page == Page::Keys);
    assert_eq!(app.focus, Focus::List);
    app.handle_key(press(KeyCode::Char('n'))).unwrap();
    assert!(matches!(app.wizard, Some(KeyWizard::Parameters(_))));
    app.handle_key(press(KeyCode::Esc)).unwrap();
    app.handle_key(press(KeyCode::Right)).unwrap();
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
    assert!(app.hosts.message.contains("single alias"));
    app.handle_key(press(KeyCode::Enter)).unwrap();
    app.handle_key(press(KeyCode::Char('a'))).unwrap();
    app.handle_key(press(KeyCode::Char('K'))).unwrap();
    assert!(app.page == Page::Hosts && app.hosts.is_editing());
    app.handle_key(press(KeyCode::Esc)).unwrap();
    app.handle_key(press(KeyCode::Char('K'))).unwrap();
    app.handle_key(press(KeyCode::Char('k'))).unwrap();
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
    assert_eq!(app.focus, Focus::List);
    assert!(app.page == Page::Hosts);
    app.handle_key(press(KeyCode::Enter)).unwrap();
    app.handle_key(press(KeyCode::Down)).unwrap();
    app.handle_key(press(KeyCode::Char('k'))).unwrap();
    app.handle_key(press(KeyCode::Char('h'))).unwrap();
    assert_eq!(app.hosts.connection_alias().unwrap(), "second");
    app.handle_key(press(KeyCode::Char('e'))).unwrap();
    app.handle_key(press(KeyCode::Down)).unwrap();
    app.handle_key(press(KeyCode::Tab)).unwrap();
    assert!(app.page == Page::Hosts && app.hosts.is_editing());
    app.handle_key(press(KeyCode::Esc)).unwrap();
    app.handle_key(press(KeyCode::Char('d'))).unwrap();
    app.handle_key(press(KeyCode::Left)).unwrap();
    assert!(app.hosts.is_editing());
    app.handle_key(press(KeyCode::Esc)).unwrap();
    app.handle_key(press(KeyCode::Char('k'))).unwrap();
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
    app.handle_key(press(KeyCode::Right)).unwrap();
    assert_eq!(app.action_index, 0);
    app.handle_key(press(KeyCode::Left)).unwrap();
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
    app.message = "An action failed. Try again.".into();
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
