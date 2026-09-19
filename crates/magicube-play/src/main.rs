mod app;
mod cli;
mod render;
mod solutions;
mod terminal;

use std::env;
use std::error::Error;
use std::fs;
use std::io::{self, IsTerminal};
use std::process::ExitCode;
use std::time::Duration;

use crossterm::event::{self, Event};
use magicube_solver::{GameState, GameStatus};

use app::{App, Command, Notification};
use cli::Options;
use terminal::TerminalSession;

const DEFAULT_LEVEL: &str = include_str!("../../../data/level-manual-labels/1.txt");
const SHOT_RECOVERY_DELAY: Duration = Duration::from_millis(120);
const HELP: &str = "Usage: magicube-play [--solutions-dir PATH] [LEVEL.txt]

Play an ASCII level using the shared Magicube simulator.
Without a path, loads the bundled first level.
Movement, firing and waiting advance one update. Aiming pauses time.
Successful shots automatically advance one recovery update after a brief pause.

  Left / A          Move or push left
  Right / D         Move or push right
  Z                 Jump
  Down / S / .      Wait (advance one update)
  X                 Aim / cancel; Left or Right fires while aiming
  U / Backspace     Undo one action (shot and recovery together)
  R                 Restart the level
  P                 Save current inputs (including partial attempts)
  Q / Esc / Ctrl-C  Quit
  -h / --help       Show this help

Winning automatically saves your solution. P also saves partial attempts.
Saves use your platform's user data directory under magicube/solutions.
Use --solutions-dir PATH to choose another directory. No directory is created
until a save. The saved filename is shown in-game and printed when you quit.
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("magicube-play: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let options = Options::parse(env::args_os().skip(1))?;
    if options.help {
        print!("{HELP}");
        return Ok(());
    }
    // Load and validate before taking over the terminal so errors print normally.
    let (map, name) = match options.level.as_ref() {
        Some(path) => {
            let map = fs::read_to_string(path).map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", path.display()))
            })?;
            (map, path.display().to_string())
        }
        None => (DEFAULT_LEVEL.to_owned(), "Level 1 (bundled)".to_owned()),
    };
    let state = GameState::from_ascii(&map)?;
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            io::Error::other("interactive mode requires a terminal on stdin and stdout").into(),
        );
    }

    let mut app = App::new(state);
    let mut terminal = TerminalSession::enter()?;
    let mut last_saved_path = None;
    let mut previous_status = app.state.status();
    let mut save_requested = false;
    'game: loop {
        let status = app.state.status();
        // Save once on entering victory, including wins during automatic recovery.
        // Redraws and ignored inputs must not create duplicate files or retries.
        if save_requested || (status == GameStatus::Won && previous_status != GameStatus::Won) {
            app.notification = Some(
                match solutions::save(&app, &name, &map, options.solutions_dir.as_deref()) {
                    Ok(path) => {
                        let text = format!("Saved: {}", path.display());
                        last_saved_path = Some(path);
                        Notification { text, error: false }
                    }
                    Err(error) => Notification {
                        text: format!("Save failed: {error}. Press P to retry."),
                        error: true,
                    },
                },
            );
        }
        save_requested = false;
        previous_status = status;
        let size = crossterm::terminal::size()?;
        render::draw(&mut terminal.stdout, size, &app, &name)?;
        if app.is_recovering() {
            // Display the firing frame before advancing recovery. Keys stay
            // queued in the terminal and are handled after recovery completes.
            std::thread::sleep(SHOT_RECOVERY_DELAY);
            app.advance_recovery();
            continue;
        }
        loop {
            // Poll blocks until input or timeout. Check dimensions on timeout
            // too, since some terminals miss resize notifications. Neither
            // a timeout nor a resize advances the simulation.
            if !event::poll(Duration::from_millis(250))? {
                if crossterm::terminal::size()? != size {
                    break;
                }
                continue;
            }
            match event::read()? {
                Event::Key(key) => match app::command_for_key(key) {
                    Some(Command::Quit) => break 'game,
                    Some(Command::Save) => {
                        save_requested = true;
                        break;
                    }
                    Some(command) => {
                        app.apply(command);
                        break;
                    }
                    None => {}
                },
                Event::Resize(_, _) => break,
                _ => {}
            }
        }
    }
    drop(terminal);
    if let Some(path) = last_saved_path {
        println!("Saved solution: {}", path.display());
    }
    Ok(())
}
