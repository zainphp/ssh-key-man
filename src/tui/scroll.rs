use crossterm::event::KeyCode;
use ratatui::{
    layout::Rect,
    text::Text,
    widgets::{Paragraph, Wrap},
    Frame,
};

#[derive(Default)]
pub(super) struct Scroll {
    offset: u16,
    maximum: u16,
    page: u16,
    follow_focus: bool,
}

impl Scroll {
    pub fn handle_key(&mut self, key: KeyCode) -> bool {
        self.offset = match key {
            KeyCode::PageDown => self.offset.saturating_add(self.page.max(1)),
            KeyCode::PageUp => self.offset.saturating_sub(self.page.max(1)),
            _ => return false,
        }
        .min(self.maximum);
        self.follow_focus = false;
        true
    }

    pub fn follow_focus(&mut self) {
        self.follow_focus = true;
    }

    pub fn draw(&mut self, frame: &mut Frame, area: Rect, text: Text) -> bool {
        let paragraph = Paragraph::new(text.clone()).wrap(Wrap { trim: false });
        self.maximum = paragraph
            .line_count(area.width)
            .saturating_sub(usize::from(area.height))
            .min(usize::from(u16::MAX)) as u16;
        self.page = area.height.saturating_sub(1).max(1);
        if self.follow_focus {
            if let Some(index) = text
                .lines
                .iter()
                .position(|line| line.to_string().starts_with("> "))
            {
                let before = Paragraph::new(Text::from(text.lines[..index].to_vec()))
                    .wrap(Wrap { trim: false })
                    .line_count(area.width) as u16;
                let height = Paragraph::new(text.lines[index].clone())
                    .wrap(Wrap { trim: false })
                    .line_count(area.width) as u16;
                if before < self.offset {
                    self.offset = before;
                } else if before.saturating_add(height) > self.offset.saturating_add(area.height) {
                    self.offset = before.saturating_add(height).saturating_sub(area.height);
                }
            }
            self.follow_focus = false;
        }
        self.offset = self.offset.min(self.maximum);
        frame.render_widget(paragraph.scroll((self.offset, 0)), area);
        self.maximum > 0
    }
}
