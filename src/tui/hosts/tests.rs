use super::*;
use crate::tui::model::Focus;
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
    let mut view = HostPage::new(dir.path()).unwrap();
    view.move_selection(true);
    view.handle_action(Action::New).unwrap();
    view.form.as_mut().unwrap().values = [
        "new-host".into(),
        "new.invalid".into(),
        String::new(),
        String::new(),
        String::new(),
    ];
    view.handle_key(dir.path(), press(KeyCode::Enter)).unwrap();
    assert_eq!(view.connection_alias().unwrap(), "new-host");
}

#[test]
fn form_creates_edits_and_confirms_deletion() {
    let dir = tempfile::tempdir().unwrap();
    let mut view = HostPage::new(dir.path()).unwrap();
    view.handle_action(Action::New).unwrap();
    assert!(view.handle_key(dir.path(), press(KeyCode::Enter)).is_err());
    let form = view.form.as_mut().unwrap();
    form.values = [
        "work".into(),
        "example.invalid".into(),
        "alice".into(),
        "2222".into(),
        "~/.ssh/id_work".into(),
    ];
    view.handle_key(dir.path(), press(KeyCode::Enter)).unwrap();
    assert_eq!(view.config.entries[0].host.alias, "work");
    assert!(view.message.text.contains("ssh work"));
    view.handle_action(Action::Edit).unwrap();
    view.handle_key(
        dir.path(),
        KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL),
    )
    .unwrap();
    for c in "office".chars() {
        view.handle_key(dir.path(), press(KeyCode::Char(c)))
            .unwrap();
    }
    view.handle_key(dir.path(), press(KeyCode::Enter)).unwrap();
    assert_eq!(view.config.entries[0].host.alias, "office");
    view.handle_action(Action::Delete).unwrap();
    view.handle_key(dir.path(), press(KeyCode::Enter)).unwrap();
    assert_eq!(view.config.entries.len(), 1);
    view.handle_action(Action::Delete).unwrap();
    view.handle_key(dir.path(), press(KeyCode::Down)).unwrap();
    view.handle_key(dir.path(), press(KeyCode::Enter)).unwrap();
    assert!(view.config.entries.is_empty());
    assert!(fs::read_to_string(dir.path().join("config"))
        .unwrap()
        .trim()
        .is_empty());
}

#[test]
fn renders_empty_list_form_and_confirmation_at_supported_sizes() {
    let dir = tempfile::tempdir().unwrap();
    let mut view = HostPage::new(dir.path()).unwrap();
    for (width, height) in [(1, 1), (40, 8), (60, 18), (110, 30)] {
        let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
        terminal.draw(|f| view.draw(f, Focus::List, 0)).unwrap();
        view.handle_action(Action::New).unwrap();
        terminal.draw(|f| view.draw(f, Focus::List, 0)).unwrap();
        view.handle_key(dir.path(), press(KeyCode::Esc)).unwrap();
    }
    assert!(!view.is_editing());
}
