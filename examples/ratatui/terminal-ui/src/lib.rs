//! Owned-value adapter for ratatui's borrowed frames and terminal state.
use std::cell::RefCell;

use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::{
    DefaultTerminal, Frame,
    widgets::{Block, Paragraph},
};

thread_local! {
    static TERMINAL: RefCell<Option<DefaultTerminal>> = const { RefCell::new(None) };
}

pub fn start() -> Result<(), String> {
    let terminal = ratatui::try_init().map_err(|e| e.to_string())?;
    TERMINAL.with(|slot| *slot.borrow_mut() = Some(terminal));
    Ok(())
}

pub fn stop() {
    TERMINAL.with(|slot| {
        if slot.borrow_mut().take().is_some() {
            ratatui::restore();
        }
    });
}

fn render(frame: &mut Frame, count: i64) {
    let text = format!("Count: {count}\n\n+ / Up: increment\n- / Down: decrement\nq / Esc: quit");
    frame.render_widget(
        Paragraph::new(text).block(Block::bordered().title("rint + ratatui")),
        frame.area(),
    );
}

pub fn draw(count: i64) -> Result<(), String> {
    TERMINAL.with(|slot| {
        let mut slot = slot.borrow_mut();
        let terminal = slot.as_mut().ok_or("terminal is not initialized")?;
        terminal
            .draw(|frame| render(frame, count))
            .map_err(|e| e.to_string())?;
        Ok(())
    })
}

pub fn read_key() -> Result<String, String> {
    let event = event::read().map_err(|e| e.to_string())?;
    Ok(match event {
        Event::Key(key) if key.kind == KeyEventKind::Press => match key.code {
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => "q",
            KeyCode::Char('q') | KeyCode::Esc => "q",
            KeyCode::Char('+') | KeyCode::Up => "+",
            KeyCode::Char('-') | KeyCode::Down => "-",
            _ => "",
        },
        _ => "",
    }
    .into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn renders_counter_and_controls() {
        let mut terminal = Terminal::new(TestBackend::new(40, 9)).unwrap();
        terminal.draw(|frame| render(frame, 42)).unwrap();
        let buffer = terminal.backend().buffer();
        let text: String = buffer.content.iter().map(|cell| cell.symbol()).collect();
        assert!(text.contains("rint + ratatui"));
        assert!(text.contains("Count: 42"));
        assert!(text.contains("q / Esc: quit"));
    }
}
