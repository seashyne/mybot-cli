use std::io::{Stdout, stdout};

use crossterm::event::{DisableBracketedPaste, EnableBracketedPaste};
use crossterm::execute;
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;

/// Restores the terminal on drop, including on early returns and panics.
pub struct TerminalGuard;

impl TerminalGuard {
    pub fn enter() -> std::io::Result<Self> {
        enable_raw_mode()?;
        execute!(stdout(), EnterAlternateScreen, EnableBracketedPaste)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), DisableBracketedPaste, LeaveAlternateScreen);
    }
}

pub fn install_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(stdout(), DisableBracketedPaste, LeaveAlternateScreen);
        hook(info);
    }));
}

pub fn terminal() -> std::io::Result<Terminal<CrosstermBackend<Stdout>>> {
    Terminal::new(CrosstermBackend::new(stdout()))
}
