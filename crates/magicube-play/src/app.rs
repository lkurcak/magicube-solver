use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use magicube_solver::{GameInput, GameState, GameStatus, PlayerMode};
use std::time::{Duration, Instant};

const RESTART_HOLD_DURATION: Duration = Duration::from_millis(800);
const RESTART_REPEAT_GAP: Duration = Duration::from_millis(800);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Step(GameInput),
    Undo,
    Restart,
    Save,
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
    // Shift is harmless, but terminal shortcuts must not become game actions.
    if !(key.modifiers - KeyModifiers::SHIFT).is_empty() {
        return None;
    }
    match key.code {
        KeyCode::Left | KeyCode::Char('a' | 'A') => Some(Command::Step(GameInput::Left)),
        KeyCode::Right | KeyCode::Char('d' | 'D') => Some(Command::Step(GameInput::Right)),
        KeyCode::Char('z' | 'Z') => Some(Command::Step(GameInput::Jump)),
        KeyCode::Char('s' | 'S' | '.') => Some(Command::Step(GameInput::Wait)),
        KeyCode::Down => Some(Command::Undo),
        KeyCode::Char('x' | 'X') => Some(Command::Step(GameInput::Shoot)),
        KeyCode::Backspace | KeyCode::Char('u' | 'U') => Some(Command::Undo),
        KeyCode::Char('r' | 'R') => Some(Command::Restart),
        KeyCode::Char('p' | 'P') if key.kind != KeyEventKind::Repeat => Some(Command::Save),
        KeyCode::Esc | KeyCode::Char('q' | 'Q') => Some(Command::Quit),
        _ => None,
    }
}

/// Recognizes a held Up arrow from the terminal's key-repeat stream. Requiring
/// several events as well as elapsed time keeps an ordinary tap harmless.
#[derive(Debug, Default)]
pub struct RestartHold {
    started: Option<Instant>,
    last_event: Option<Instant>,
    events: usize,
}

impl RestartHold {
    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        self.handle_key_at(key, Instant::now())
    }

    fn handle_key_at(&mut self, key: KeyEvent, now: Instant) -> bool {
        let up = key.code == KeyCode::Up
            && (key.modifiers - KeyModifiers::SHIFT).is_empty()
            && key.kind != KeyEventKind::Release;
        if !up {
            self.reset();
            return false;
        }

        if self
            .last_event
            .is_none_or(|last| now.duration_since(last) > RESTART_REPEAT_GAP)
        {
            self.started = Some(now);
            self.events = 0;
        }
        self.last_event = Some(now);
        self.events += 1;

        let held_long_enough = now.duration_since(self.started.unwrap()) >= RESTART_HOLD_DURATION;
        if held_long_enough && self.events >= 3 {
            self.reset();
            true
        } else {
            false
        }
    }

    pub fn expire(&mut self) {
        if self
            .last_event
            .is_some_and(|last| last.elapsed() > RESTART_REPEAT_GAP)
        {
            self.reset();
        }
    }

    fn reset(&mut self) {
        self.started = None;
        self.last_event = None;
        self.events = 0;
    }
}

pub struct Notification {
    pub text: String,
    pub error: bool,
}

pub struct App {
    initial: GameState,
    pub state: GameState,
    pub notification: Option<Notification>,
    // Each entry retains the complete pre-input state, including jump airtime.
    history: Vec<(GameState, GameInput)>,
}

impl App {
    pub fn new(initial: GameState) -> Self {
        Self {
            state: initial.clone(),
            initial,
            history: Vec::new(),
            notification: None,
        }
    }

    pub fn steps(&self) -> usize {
        self.history.len()
    }

    pub fn last_input(&self) -> Option<GameInput> {
        self.history.last().map(|(_, input)| *input)
    }

    pub fn inputs(&self) -> impl Iterator<Item = GameInput> + '_ {
        self.history.iter().map(|(_, input)| *input)
    }

    pub fn is_recovering(&self) -> bool {
        self.state.status() == GameStatus::Playing
            && self.state.player().mode == PlayerMode::Recovering
    }

    /// Record the automatic update explicitly so exported inputs replay exactly.
    pub fn advance_recovery(&mut self) {
        if self.is_recovering() {
            self.apply(Command::Step(GameInput::Wait));
        }
    }

    pub fn apply(&mut self, command: Command) {
        // Ignored gameplay inputs must not dismiss the saved path or save error.
        if matches!(command, Command::Step(_)) && self.state.status() != GameStatus::Playing {
            return;
        }
        self.notification = None;
        match command {
            Command::Step(input) => {
                let next = self.state.step(input);
                self.history
                    .push((std::mem::replace(&mut self.state, next), input));
            }
            Command::Undo => {
                // Undo a shot and its recovery together; restoring a pending
                // recovery alone would immediately replay the automatic update.
                while let Some((state, _)) = self.history.pop() {
                    self.state = state;
                    if self.state.player().mode != PlayerMode::Recovering {
                        break;
                    }
                }
            }
            Command::Restart => {
                self.state = self.initial.clone();
                self.history.clear();
            }
            Command::Save | Command::Quit => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn press_and_repeat_advance_but_release_and_unbound_shortcuts_do_not() {
        let key = KeyEvent::new(KeyCode::Right, KeyModifiers::NONE);
        assert_eq!(command_for_key(key), Some(Command::Step(GameInput::Right)));
        assert_eq!(
            command_for_key(KeyEvent {
                kind: KeyEventKind::Repeat,
                ..key
            }),
            command_for_key(key)
        );
        assert_eq!(
            command_for_key(KeyEvent {
                kind: KeyEventKind::Release,
                ..key
            }),
            None
        );
        assert_eq!(
            command_for_key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            None
        );
        assert_eq!(
            command_for_key(KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL)),
            Some(Command::Quit)
        );
        assert_eq!(
            command_for_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE)),
            None
        );
        assert_eq!(
            command_for_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE)),
            Some(Command::Undo)
        );
        assert_eq!(
            command_for_key(KeyEvent::new(KeyCode::Char('s'), KeyModifiers::NONE)),
            Some(Command::Step(GameInput::Wait))
        );
    }

    #[test]
    fn up_arrow_requires_a_sustained_hold_to_restart() {
        let start = Instant::now();
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::NONE);
        let mut hold = RestartHold::default();

        assert!(!hold.handle_key_at(up, start));
        assert!(!hold.handle_key_at(up, start + Duration::from_millis(400)));
        assert!(!hold.handle_key_at(up, start + Duration::from_millis(799)));
        assert!(hold.handle_key_at(up, start + Duration::from_millis(800)));

        // A pause breaks the gesture, and other keys cancel it.
        assert!(!hold.handle_key_at(up, start + Duration::from_secs(2)));
        assert!(!hold.handle_key_at(up, start + Duration::from_secs(3)));
        assert!(!hold.handle_key_at(
            KeyEvent::new(KeyCode::Left, KeyModifiers::NONE),
            start + Duration::from_millis(3100)
        ));
        assert!(!hold.handle_key_at(up, start + Duration::from_millis(3200)));
    }

    #[test]
    fn undo_restores_airtime_and_restart_discards_history() {
        let initial = GameState::from_ascii(
            r#"
#######
#     #
# @   #
#######
"#
            .trim_matches('\n'),
        )
        .unwrap();
        let mut app = App::new(initial.clone());
        app.apply(Command::Undo);
        assert_eq!(app.state, initial);
        app.apply(Command::Step(GameInput::Jump));
        let jumping = app.state.clone();
        app.apply(Command::Step(GameInput::Right));
        app.apply(Command::Step(GameInput::Wait));
        assert!(app.state.is_grounded());
        assert_eq!(app.steps(), 3);
        app.apply(Command::Undo);
        assert_eq!(app.state.player().air_inputs_remaining, 1);
        app.apply(Command::Undo);
        assert_eq!(app.state, jumping);
        assert_eq!(app.last_input(), Some(GameInput::Jump));
        app.apply(Command::Step(GameInput::Left));
        assert_eq!(app.state, jumping.step(GameInput::Left));
        app.apply(Command::Restart);
        assert_eq!(app.state, initial);
        assert_eq!(app.steps(), 0);
        assert_eq!(app.last_input(), None);
        app.apply(Command::Undo);
        assert_eq!(app.state, initial);
    }

    #[test]
    fn recovery_is_recorded_once_and_undo_groups_it_with_the_shot() {
        let initial = GameState::from_ascii(
            r#"
##########
#@       #
##########
"#
            .trim_matches('\n'),
        )
        .unwrap();
        let mut app = App::new(initial.clone());
        app.advance_recovery();
        assert_eq!(app.steps(), 0);
        app.apply(Command::Step(GameInput::Shoot));
        let aiming = app.state.clone();
        app.apply(Command::Step(GameInput::Right));
        assert!(app.is_recovering());
        app.advance_recovery();
        assert!(!app.is_recovering());
        app.advance_recovery();
        assert_eq!(
            app.inputs().collect::<Vec<_>>(),
            [GameInput::Shoot, GameInput::Right, GameInput::Wait]
        );
        let recovered = app.state.clone();
        app.apply(Command::Step(GameInput::Right));
        app.apply(Command::Undo);
        assert_eq!(app.state, recovered);
        app.apply(Command::Undo);
        assert_eq!(app.state, aiming);
        assert_eq!(app.steps(), 1);
        app.advance_recovery();
        assert_eq!(app.state, aiming);

        // Blocked shots and cancellation do not schedule recovery.
        app.apply(Command::Step(GameInput::Left));
        assert_eq!(app.state, aiming);
        assert!(!app.is_recovering());
        app.apply(Command::Step(GameInput::Shoot));
        assert_eq!(app.state, initial);
        assert!(!app.is_recovering());
    }

    #[test]
    fn wins_and_deaths_stop_recovery_and_allow_undoing_the_shot() {
        for (map, status, needs_recovery) in [
            (
                r#"
#####
#@ ##
##G##
"#
                .trim_matches('\n'),
                GameStatus::Won,
                false,
            ),
            (
                r#"
########
#@   ###
####G###
"#
                .trim_matches('\n'),
                GameStatus::Won,
                true,
            ),
            (
                r#"
#######
# C   #
#     #
#     #
# @   #
#######
"#
                .trim_matches('\n'),
                GameStatus::GameOver,
                true,
            ),
        ] {
            let mut app = App::new(GameState::from_ascii(map).unwrap());
            app.apply(Command::Step(GameInput::Shoot));
            let aiming = app.state.clone();
            app.apply(Command::Step(GameInput::Right));
            assert_eq!(app.is_recovering(), needs_recovery);
            app.advance_recovery();
            assert_eq!(app.state.status(), status);
            let steps = app.steps();
            assert_eq!(steps, if needs_recovery { 3 } else { 2 });
            app.advance_recovery();
            assert_eq!(app.steps(), steps);
            app.apply(Command::Undo);
            assert_eq!(app.state, aiming);
            assert_eq!(app.steps(), 1);
        }
    }

    #[test]
    fn finished_games_allow_undo_and_restart_without_recording_ignored_inputs() {
        for (map, input, status) in [
            (
                r#"
#####
# C #
# @ #
#####
"#
                .trim_matches('\n'),
                GameInput::Wait,
                GameStatus::GameOver,
            ),
            (
                r#"
######
#@O  #
###G##
"#
                .trim_matches('\n'),
                GameInput::Right,
                GameStatus::Won,
            ),
        ] {
            let initial = GameState::from_ascii(map).unwrap();
            let mut app = App::new(initial.clone());
            app.apply(Command::Step(input));
            assert_eq!(app.state.status(), status);
            app.apply(Command::Step(GameInput::Left));
            app.apply(Command::Step(GameInput::Shoot));
            assert_eq!(app.steps(), 1);
            app.apply(Command::Undo);
            assert_eq!(app.state, initial);
            app.apply(Command::Step(input));
            app.apply(Command::Restart);
            assert_eq!(app.state, initial);
            assert_eq!(app.steps(), 0);
        }
    }
}
