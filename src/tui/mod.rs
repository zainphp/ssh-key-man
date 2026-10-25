mod app;
mod components;
mod hosts;
mod model;
mod notice;
mod scroll;
mod terminal;
mod wizard;

use anyhow::Result;
use app::App;
use crossterm::event::{self, Event, KeyEventKind};
use model::{InputResult, RunState};
use std::{path::Path, time::Duration};
use terminal::TerminalSession;

pub fn run(directory: &Path) -> Result<()> {
    let mut app = App::new(directory)?;
    let previous_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        TerminalSession::restore();
        previous_hook(info);
    }));
    let mut screen = TerminalSession::default();
    let mut terminal = screen.enter()?;
    loop {
        app.poll_refresh();
        terminal.draw(|frame| app.draw(frame))?;
        if !event::poll(Duration::from_millis(250))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind != KeyEventKind::Press {
            continue;
        }
        let size = terminal.size()?;
        app.resize(ratatui::layout::Rect::new(0, 0, size.width, size.height));
        match app.handle_key(key) {
            Ok(InputResult::Quit) => break,
            Ok(InputResult::Run(operation)) => {
                screen.leave();
                if matches!(app.run_operation(operation)?, RunState::Finished) {
                    return Ok(());
                }
                terminal = screen.enter()?;
            }
            Ok(InputResult::Continue) => {}
            Err(error) => app
                .notice_mut()
                .set(format!("{error:#}"), crate::theme::Tone::Danger),
        }
    }
    Ok(())
}
