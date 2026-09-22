use std::io::{self, Write};
use std::time::{Duration, Instant};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use magicube_solver::{GameInput, GameState, SolveOptions, SolveOutcome, solve};

use crate::app::{App, Notification};
use crate::render;

const PLAYBACK_INTERVAL: Duration = Duration::from_millis(250);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Back(usize),
    Forward(usize),
    Start,
    End,
    TogglePlayback,
    Quit,
}

pub fn command_for_key(key: KeyEvent) -> Option<Command> {
    if key.kind == KeyEventKind::Release {
        return None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c' | 'C'))
    {
        return Some(Command::Quit);
    }
    if !(key.modifiers - KeyModifiers::SHIFT).is_empty() {
        return None;
    }
    match key.code {
        KeyCode::Left | KeyCode::Char('a' | 'A') => Some(Command::Back(1)),
        KeyCode::Right | KeyCode::Char('d' | 'D') => Some(Command::Forward(1)),
        KeyCode::Up | KeyCode::PageUp => Some(Command::Back(10)),
        KeyCode::Down | KeyCode::PageDown => Some(Command::Forward(10)),
        KeyCode::Home => Some(Command::Start),
        KeyCode::End => Some(Command::End),
        KeyCode::Char(' ') if key.kind != KeyEventKind::Repeat => Some(Command::TogglePlayback),
        KeyCode::Esc | KeyCode::Char('q' | 'Q') => Some(Command::Quit),
        _ => None,
    }
}

/// A snapshot after every recorded input, plus the untouched initial state.
/// Seeking never inserts recovery updates or groups inputs like gameplay undo.
pub struct Replay {
    states: Vec<GameState>,
    inputs: Vec<GameInput>,
    position: usize,
    next_tick: Option<Instant>,
}

impl Replay {
    pub fn new(initial: GameState, inputs: Vec<GameInput>) -> Self {
        let mut states = vec![initial];
        for &input in &inputs {
            states.push(states.last().unwrap().step(input));
        }
        Self {
            states,
            inputs,
            position: 0,
            next_tick: None,
        }
    }

    pub fn from_solver(initial: GameState, options: SolveOptions) -> io::Result<Self> {
        match solve(&initial, options).outcome {
            SolveOutcome::Solved(inputs) => Ok(Self::new(initial, inputs)),
            SolveOutcome::Unsolvable => Err(io::Error::other("this level has no solution")),
            SolveOutcome::StateLimitReached => Err(io::Error::other(
                "solver reached its state limit; no complete solution found",
            )),
        }
    }

    pub fn state(&self) -> &GameState {
        &self.states[self.position]
    }

    pub fn final_state(&self) -> &GameState {
        self.states.last().unwrap()
    }

    pub fn position(&self) -> usize {
        self.position
    }

    pub fn len(&self) -> usize {
        self.inputs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.inputs.is_empty()
    }

    pub fn last_input(&self) -> Option<GameInput> {
        self.position.checked_sub(1).map(|index| self.inputs[index])
    }

    pub fn next_input(&self) -> Option<GameInput> {
        self.inputs.get(self.position).copied()
    }

    pub fn is_playing(&self) -> bool {
        self.next_tick.is_some()
    }

    pub fn apply(&mut self, command: Command, now: Instant) {
        match command {
            Command::TogglePlayback => {
                if self.is_playing() || self.inputs.is_empty() {
                    self.next_tick = None;
                } else {
                    if self.position == self.len() {
                        self.position = 0;
                    }
                    self.next_tick = Some(now + PLAYBACK_INTERVAL);
                }
            }
            Command::Back(count) => self.seek(self.position.saturating_sub(count)),
            Command::Forward(count) => self.seek(self.position.saturating_add(count)),
            Command::Start => self.seek(0),
            Command::End => self.seek(self.len()),
            Command::Quit => self.next_tick = None,
        }
    }

    fn seek(&mut self, position: usize) {
        self.position = position.min(self.len());
        self.next_tick = None;
    }

    /// Advance at most one input per displayed frame, even after a delayed tick.
    pub fn tick(&mut self, now: Instant) -> bool {
        if self.next_tick.is_none_or(|deadline| now < deadline) {
            return false;
        }
        self.position += 1;
        self.next_tick = (self.position < self.len()).then_some(now + PLAYBACK_INTERVAL);
        true
    }

    fn poll_timeout(&self, now: Instant) -> Duration {
        self.next_tick
            .map(|deadline| deadline.saturating_duration_since(now))
            .unwrap_or(PLAYBACK_INTERVAL)
            .min(PLAYBACK_INTERVAL)
    }
}

pub fn solve_and_run(out: &mut impl Write, name: &str, map: &str) -> io::Result<()> {
    let initial = GameState::from_ascii(map)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let mut app = App::new(initial);
    app.notification = Some(Notification {
        text: "Finding a shortest solution...".to_owned(),
        error: false,
    });
    render::draw(out, crossterm::terminal::size()?, &app, name)?;
    let replay = Replay::from_solver(app.state, SolveOptions::default())?;
    run(out, &format!("{name} (solver replay)"), replay)
}

pub fn run(out: &mut impl Write, name: &str, mut replay: Replay) -> io::Result<()> {
    let mut size = crossterm::terminal::size()?;
    let mut redraw = true;
    loop {
        if redraw {
            render::draw_replay(out, size, &replay, name)?;
        }
        redraw = false;
        // Handle input before a due tick so pause and seek take effect immediately.
        if event::poll(replay.poll_timeout(Instant::now()))? {
            match event::read()? {
                Event::Key(key) => {
                    if let Some(command) = command_for_key(key) {
                        if command == Command::Quit {
                            return Ok(());
                        }
                        replay.apply(command, Instant::now());
                        redraw = true;
                    }
                }
                Event::Resize(_, _) => redraw = true,
                _ => {}
            }
        }
        redraw |= replay.tick(Instant::now());
        let current_size = crossterm::terminal::size()?;
        redraw |= current_size != size;
        size = current_size;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use GameInput::{Jump, Left, Right, Shoot, Wait};
    use magicube_solver::{GameStatus, PlayerMode};

    #[test]
    fn scrubbing_restores_exact_snapshots_including_no_ops_and_shot_recovery() {
        let initial = GameState::from_ascii(
            r#"
#######
#     #
#@  ###
###G###
"#
            .trim_matches('\n'),
        )
        .unwrap();
        let inputs = vec![Left, Shoot, Jump, Shoot, Shoot, Right, Wait];
        let mut replay = Replay::new(initial.clone(), inputs.clone());
        let now = Instant::now();
        assert_eq!(replay.position(), 0);
        assert!(!replay.is_playing());
        assert_eq!(replay.last_input(), None);
        assert_eq!(replay.next_input(), Some(Left));
        let mut expected = vec![initial];
        for input in inputs {
            expected.push(expected.last().unwrap().step(input));
        }
        for (index, state) in expected.iter().enumerate().skip(1) {
            replay.apply(Command::Forward(1), now);
            assert_eq!(replay.position(), index);
            assert_eq!(replay.state(), state);
        }
        assert_eq!(replay.state().status(), GameStatus::Won);
        assert_eq!(replay.next_input(), None);
        replay.apply(Command::Back(1), now);
        assert_eq!(replay.state().player().mode, PlayerMode::Recovering);
        for state in expected[..6].iter().rev() {
            replay.apply(Command::Back(1), now);
            assert_eq!(replay.state(), state);
        }
        replay.apply(Command::End, now);
        assert_eq!(replay.state(), expected.last().unwrap());
        replay.apply(Command::Start, now);
        assert_eq!(replay.state(), &expected[0]);
    }

    #[test]
    fn ten_step_seeks_and_endpoints_clamp_without_wrapping() {
        let initial = GameState::from_ascii(
            r#"
@ G
"#
            .trim_matches('\n'),
        )
        .unwrap();
        let mut replay = Replay::new(initial.clone(), vec![Wait; 25]);
        let now = Instant::now();
        for (command, expected) in [
            (Command::Back(10), 0),
            (Command::Forward(10), 10),
            (Command::Forward(10), 20),
            (Command::Forward(10), 25),
            (Command::Back(10), 15),
            (Command::Forward(usize::MAX), 25),
            (Command::Start, 0),
            (Command::End, 25),
        ] {
            replay.apply(command, now);
            assert_eq!(replay.position(), expected);
        }
        assert_eq!(replay.state().player().position.y, 0);
        let mut empty = Replay::new(initial.clone(), vec![]);
        for command in [Command::End, Command::Back(10), Command::TogglePlayback] {
            empty.apply(command, now);
            assert_eq!(empty.state(), &initial);
            assert_eq!(empty.position(), 0);
            assert!(!empty.is_playing());
        }
    }

    #[test]
    fn autoplay_pauses_seeks_resumes_and_stops_at_the_end_without_skipping() {
        let initial = GameState::from_ascii(
            r#"
#####
#@ ##
##G##
"#
            .trim_matches('\n'),
        )
        .unwrap();
        let mut replay = Replay::new(initial, vec![Shoot, Right]);
        let now = Instant::now();
        replay.apply(Command::TogglePlayback, now);
        assert!(!replay.tick(now + PLAYBACK_INTERVAL / 2));
        assert!(replay.tick(now + PLAYBACK_INTERVAL));
        assert_eq!(replay.position(), 1);
        replay.apply(Command::TogglePlayback, now + PLAYBACK_INTERVAL);
        assert!(!replay.tick(now + Duration::from_secs(10)));
        replay.apply(Command::TogglePlayback, now + Duration::from_secs(10));
        // Even a long delay advances just one recorded input.
        assert!(replay.tick(now + Duration::from_secs(20)));
        assert_eq!(replay.position(), 2);
        assert!(!replay.is_playing());
        assert!(!replay.tick(now + Duration::from_secs(21)));
        replay.apply(Command::TogglePlayback, now + Duration::from_secs(21));
        assert_eq!(replay.position(), 0);
        assert!(replay.is_playing());
        replay.apply(Command::Forward(1), now + Duration::from_secs(21));
        assert_eq!(replay.position(), 1);
        assert!(!replay.is_playing());
        replay.apply(Command::TogglePlayback, now + Duration::from_secs(22));
        assert!(!replay.tick(now + Duration::from_secs(22)));
        assert!(replay.tick(now + Duration::from_secs(23)));
        assert_eq!(replay.state().status(), GameStatus::Won);
    }

    #[test]
    fn replay_keys_navigate_and_ignore_gameplay_and_repeat_toggles() {
        let key = |code| KeyEvent::new(code, KeyModifiers::NONE);
        for (code, command) in [
            (KeyCode::Left, Command::Back(1)),
            (KeyCode::Right, Command::Forward(1)),
            (KeyCode::Up, Command::Back(10)),
            (KeyCode::Down, Command::Forward(10)),
            (KeyCode::PageUp, Command::Back(10)),
            (KeyCode::PageDown, Command::Forward(10)),
            (KeyCode::Home, Command::Start),
            (KeyCode::End, Command::End),
            (KeyCode::Char(' '), Command::TogglePlayback),
            (KeyCode::Esc, Command::Quit),
        ] {
            assert_eq!(command_for_key(key(code)), Some(command));
            assert_eq!(
                command_for_key(KeyEvent {
                    kind: KeyEventKind::Release,
                    ..key(code)
                }),
                None
            );
        }
        assert_eq!(
            command_for_key(KeyEvent {
                kind: KeyEventKind::Repeat,
                ..key(KeyCode::Char(' '))
            }),
            None
        );
        assert_eq!(
            command_for_key(KeyEvent {
                kind: KeyEventKind::Repeat,
                ..key(KeyCode::Right)
            }),
            Some(Command::Forward(1))
        );
        assert_eq!(command_for_key(key(KeyCode::Char('z'))), None);
        assert_eq!(command_for_key(key(KeyCode::Char('p'))), None);
        assert_eq!(
            command_for_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Command::Quit)
        );
        assert_eq!(
            command_for_key(KeyEvent::new(KeyCode::Right, KeyModifiers::CONTROL)),
            None
        );
    }

    #[test]
    fn solver_output_uses_the_same_timeline_and_reports_search_failures() {
        let initial = GameState::from_ascii(
            r#"
#####
#@ ##
##G##
"#
            .trim_matches('\n'),
        )
        .unwrap();
        let mut replay = Replay::from_solver(initial.clone(), SolveOptions::default()).unwrap();
        assert_eq!(replay.state(), &initial);
        assert_eq!(replay.len(), 2);
        replay.apply(Command::End, Instant::now());
        assert_eq!(replay.state().status(), GameStatus::Won);
        replay.apply(Command::Start, Instant::now());
        assert_eq!(replay.state(), &initial);
        assert!(
            Replay::from_solver(
                initial,
                SolveOptions {
                    max_states: Some(0)
                }
            )
            .is_err()
        );
        let unsolvable = GameState::from_ascii(
            r#"
#####
#@# #
###G#
"#
            .trim_matches('\n'),
        )
        .unwrap();
        assert!(Replay::from_solver(unsolvable, SolveOptions::default()).is_err());
    }
}
