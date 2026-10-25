use super::{components::draw_dialog, scroll::Scroll};
use crate::theme::Tone;
use crossterm::event::KeyCode;
use ratatui::{
    layout::Rect,
    text::Text,
    widgets::{Block, Borders},
    Frame,
};

#[derive(Default)]
pub(super) struct Notice {
    pub text: String,
    pub tone: Tone,
    scroll: Scroll,
}

impl Notice {
    pub fn set(&mut self, text: impl Into<String>, tone: Tone) {
        *self = Self {
            text: text.into(),
            tone,
            scroll: Scroll::default(),
        };
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    pub fn is_empty(&self) -> bool {
        self.text.is_empty()
    }

    pub fn handle_key(&mut self, key: KeyCode) {
        let key = match key {
            KeyCode::Up => KeyCode::PageUp,
            KeyCode::Down => KeyCode::PageDown,
            other => other,
        };
        self.scroll.handle_key(key);
        if key == KeyCode::Enter {
            self.clear();
        }
    }

    pub fn draw(&mut self, frame: &mut Frame) {
        let area = frame.area();
        let width = area.width.min(84);
        let height = area.height.min(20);
        let popup = Rect::new(
            area.x + (area.width - width) / 2,
            area.y + (area.height - height) / 2,
            width,
            height,
        );
        draw_dialog(
            frame,
            popup,
            Block::default()
                .title(" Message ")
                .borders(Borders::ALL)
                .border_style(self.tone.style()),
            Text::styled(self.text.as_str(), self.tone.style()),
            "Enter: continue | Esc: quit\nUp/Down or PgUp/PgDn: scroll",
            &mut self.scroll,
        );
    }
}
