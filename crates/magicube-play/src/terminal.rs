use std::io::{self, Stdout};
use std::sync::atomic::{AtomicBool, Ordering};

use crossterm::cursor::{Hide, Show};
use crossterm::execute;
use crossterm::style::ResetColor;
use crossterm::terminal::{
    DisableLineWrap, EnableLineWrap, EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode,
    enable_raw_mode,
};

static ACTIVE: AtomicBool = AtomicBool::new(false);

pub struct TerminalSession {
    pub stdout: Stdout,
}

impl TerminalSession {
    pub fn enter() -> io::Result<Self> {
        let previous_hook = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            // Restore before printing the panic so the message stays visible.
            restore();
            previous_hook(info);
        }));

        enable_raw_mode()?;
        ACTIVE.store(true, Ordering::SeqCst);
        let mut session = Self {
            stdout: io::stdout(),
        };
        // The guard also restores the terminal if setup fails partway through.
        execute!(session.stdout, EnterAlternateScreen, Hide, DisableLineWrap)?;
        Ok(session)
    }
}

impl Drop for TerminalSession {
    fn drop(&mut self) {
        restore();
    }
}

fn restore() {
    if ACTIVE.swap(false, Ordering::SeqCst) {
        // Attempt both restorations even if the output device has failed.
        let _ = disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            ResetColor,
            Show,
            EnableLineWrap,
            LeaveAlternateScreen
        );
    }
}
