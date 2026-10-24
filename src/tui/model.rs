use crate::keys::Key;
use crossterm::event::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) enum Page {
    Hosts,
    Keys,
}

impl Page {
    pub fn other(self) -> Self {
        match self {
            Self::Hosts => Self::Keys,
            Self::Keys => Self::Hosts,
        }
    }

    pub fn index(self) -> usize {
        match self {
            Self::Hosts => 0,
            Self::Keys => 1,
        }
    }

    pub fn actions(self) -> &'static [Action] {
        match self {
            Self::Hosts => &[Action::New, Action::Edit, Action::Delete],
            Self::Keys => &[Action::New, Action::Delete],
        }
    }

    pub fn shortcut(self, key: KeyCode) -> Option<Action> {
        match key {
            KeyCode::Char('n' | 'N') => Some(Action::New),
            KeyCode::Char('g' | 'G') if self == Self::Keys => Some(Action::New),
            KeyCode::Char('a' | 'A') if self == Self::Hosts => Some(Action::New),
            KeyCode::Char('e' | 'E') if self == Self::Hosts => Some(Action::Edit),
            KeyCode::Char('d' | 'D') => Some(Action::Delete),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) enum Action {
    New,
    Edit,
    Delete,
}

impl Action {
    pub fn label(self, page: Page) -> &'static str {
        match self {
            Self::New if page == Page::Hosts => "[N] New host",
            Self::New => "[N] New key",
            Self::Edit => "[E] Edit",
            Self::Delete => "[D] Delete",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(super) enum Focus {
    Pages,
    #[default]
    List,
    Actions,
}

#[derive(Debug)]
pub(super) enum Operation {
    Generate {
        name: String,
        rsa: bool,
    },
    Finish {
        key: Key,
        remote: Option<(String, u16)>,
    },
    Load(Key),
    Delete(Key),
    Connect(String),
}

#[derive(Debug)]
pub(super) enum InputResult {
    Continue,
    Quit,
    Run(Operation),
}

pub(super) enum RunState {
    Continue,
    Finished,
}
