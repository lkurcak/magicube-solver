mod app;
mod cli;
mod level_select;
mod render;
mod replay;
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

use app::{App, Command, Notification, RestartHold};
use cli::Options;
use terminal::TerminalSession;

#[derive(Debug, Clone, Copy)]
struct BundledLevel {
    id: &'static str,
    name: &'static str,
    map: Option<&'static str>,
    issues: &'static [&'static str],
}

impl BundledLevel {
    fn is_clean(self) -> bool {
        self.map.is_some() && self.issues.is_empty()
    }

    fn playable_map(self) -> io::Result<&'static str> {
        self.map.filter(|_| self.is_clean()).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("Level {} is corrupted.", self.id),
            )
        })
    }
}

include!(concat!(env!("OUT_DIR"), "/bundled_levels.rs"));
const SHOT_RECOVERY_DELAY: Duration = Duration::from_millis(120);
const HELP: &str = "Usage: magicube-play [--solutions-dir PATH] [--airborne-shooting] [--airborne-pushing] [LEVEL.txt]
       magicube-play --replay SOLUTION.json
       magicube-play --solve [LEVEL.txt]

Play an ASCII level using the shared Magicube simulator.
Without a path, opens an interactive selector for the bundled levels.
In the selector: Enter plays, S opens a solver replay, and R browses saved replays
for the highlighted level. Closing a replay returns to its menu.
F toggles airborne shooting and P toggles airborne pushing in the selector.
Both are off by default: aiming, shooting and pushing require ground support.
--airborne-shooting and --airborne-pushing enable them during jumps and falls
for manual play. Solver launches always use the default grounded-only rules.
Movement, firing and waiting advance one update. Aiming pauses time.
Successful shots automatically advance one recovery update after a brief pause.

  Left / A          Move or push left
  Right / D         Move or push right
  Z                 Jump
  S / .             Wait (advance one update)
  X                 Aim / cancel; Left or Right fires while aiming
  Down / U / Backspace
                    Undo one action (shot and recovery together)
  Hold Up / R       Restart the level
  P                 Save current inputs (including partial attempts)
  Q / Esc / Ctrl-C  Quit
  -h / --help       Show this help

Winning automatically saves your solution. P also saves partial attempts.
Saves use your platform's user data directory under magicube/solutions.
Use --solutions-dir PATH to choose another directory. No directory is created
until a save. The saved filename is shown in-game and printed when you quit.
The selector's saved-replay browser uses this same directory.

Replay a saved attempt with --replay (uses its embedded map), or find and replay
a shortest solution with --solve. With no level file, --solve opens the selector.
The solver searches up to one million states and reports if that limit is reached.
Replay starts paused at step 0. Each step is one recorded input, including
aiming and recovery waits. Partial attempts and game-over saves also work.

  Left/Right / A/D   Back/forward one input
  Up/Down / PgUp/PgDn
                    Back/forward ten inputs
  Home / End        Jump to start/end
  Space             Play/pause at four inputs per second
  Q / Esc / Ctrl-C  Quit

Seeking pauses playback. Playback stops at the end; Space there starts again.
Replay never saves or modifies the input sequence.
New saves record both shooting and pushing settings. Older saves retain their
original rules: version 1 allows both in midair; version 2 allows airborne pushing.
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
    let mut play_settings = options.settings;
    // Validate saved data before entering raw mode, including its final outcome.
    let saved_replay = options.replay.as_deref().map(solutions::load).transpose()?;
    // Explicit files still skip the selector and launch directly.
    let custom_level = match options.level.as_ref() {
        Some(path) => {
            let map = fs::read_to_string(path).map_err(|error| {
                io::Error::new(error.kind(), format!("{}: {error}", path.display()))
            })?;
            Some((map, path.display().to_string()))
        }
        None => None,
    };
    if !io::stdin().is_terminal() || !io::stdout().is_terminal() {
        return Err(
            io::Error::other("interactive mode requires a terminal on stdin and stdout").into(),
        );
    }

    let mut terminal = TerminalSession::enter()?;
    if let Some((name, replay)) = saved_replay {
        replay::run(
            &mut terminal.stdout,
            &format!("{name} (saved replay)"),
            replay,
        )?;
        return Ok(());
    }
    let (map, name) = match custom_level {
        Some(level) => level,
        None => {
            let Some(index) = level_select::choose(
                &mut terminal.stdout,
                BUNDLED_LEVELS,
                options.solve,
                options.solutions_dir.as_deref(),
                &mut play_settings,
            )?
            else {
                return Ok(());
            };
            let level = BUNDLED_LEVELS[index];
            (
                level.playable_map()?.to_owned(),
                format!("{} (bundled)", level.name),
            )
        }
    };
    if options.solve {
        replay::solve_and_run(&mut terminal.stdout, &name, &map)?;
        return Ok(());
    }
    let mut app = App::new(GameState::from_ascii_with_settings(&map, play_settings)?);
    let mut last_saved_path = None;
    let mut previous_status = app.state.status();
    let mut save_requested = false;
    let mut restart_hold = RestartHold::default();
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
                restart_hold.expire();
                if crossterm::terminal::size()? != size {
                    break;
                }
                continue;
            }
            match event::read()? {
                Event::Key(key) => {
                    if restart_hold.handle_key(key) {
                        app.apply(Command::Restart);
                        break;
                    }
                    match app::command_for_key(key) {
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
                    }
                }
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
