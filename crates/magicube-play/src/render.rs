use std::io::{self, Write};

use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use magicube_solver::{
    GameInput, GameSettings, GameState, GameStatus, LaserBeam, PlayerMode, Position,
};

use crate::app::App;
use crate::replay::Replay;

const HEADER_ROWS: u16 = 6;

pub fn draw(
    out: &mut impl Write,
    (width, height): (u16, u16),
    app: &App,
    name: &str,
) -> io::Result<()> {
    if !begin_frame(out, (width, height))? {
        return Ok(());
    }

    let player = app.state.player();
    let motion = motion(&app.state);

    line(
        out,
        0,
        width,
        &format!("MAGICUBE | {}", rules_label(app.state.settings())),
        Color::Cyan,
    )?;
    line(out, 1, width, &format!("Level: {name}"), Color::White)?;
    line(
        out,
        2,
        width,
        &format!(
            "Input {} | {motion} | ({}, {}) | Air: {} | Last: {}",
            app.steps(),
            player.position.x,
            player.position.y,
            player.air_inputs_remaining,
            input_name(app.last_input()),
        ),
        Color::White,
    )?;
    line(
        out,
        3,
        width,
        "A/D or Left/Right: move/push | Z: jump | S/.: wait",
        Color::Grey,
    )?;
    line(
        out,
        4,
        width,
        "X: aim | Down/U: undo | Hold Up/R: restart | P: save | Q: quit",
        Color::Grey,
    )?;
    if app.state.status() == GameStatus::Won {
        line(
            out,
            5,
            width,
            "Level complete! U: undo | R: restart | Q: quit",
            Color::Green,
        )?;
    } else if app.state.status() == GameStatus::GameOver {
        line(
            out,
            5,
            width,
            "Crushed by a cube. U to undo or R to restart.",
            Color::Red,
        )?;
    } else if player.mode == PlayerMode::Aiming {
        line(
            out,
            5,
            width,
            "Left/Right: fire into empty space | X: cancel | Time is paused",
            Color::Yellow,
        )?;
    } else if player.mode == PlayerMode::Recovering {
        line(out, 5, width, "Recovering from shot...", Color::Yellow)?;
    } else if outside(&app.state) {
        line(
            out,
            5,
            width,
            "Outside the level. U to undo or R to restart.",
            Color::Yellow,
        )?;
    }

    let mut board_start = HEADER_ROWS;
    if let Some(notification) = &app.notification {
        let color = if notification.error {
            Color::Red
        } else {
            Color::Green
        };
        let characters: Vec<_> = notification.text.chars().collect();
        for chunk in characters.chunks(usize::from(width)) {
            if board_start + 1 >= height {
                break;
            }
            line(
                out,
                board_start,
                width,
                &chunk.iter().collect::<String>(),
                color,
            )?;
            board_start += 1;
        }
    }
    draw_board(out, (width, height), &app.state, board_start)
}

pub fn draw_replay(
    out: &mut impl Write,
    (width, height): (u16, u16),
    replay: &Replay,
    name: &str,
) -> io::Result<()> {
    if !begin_frame(out, (width, height))? {
        return Ok(());
    }
    let state = replay.state();
    let player = state.player();
    let playback = if replay.is_playing() {
        "Playing"
    } else {
        "Paused"
    };
    line(
        out,
        0,
        width,
        &format!(
            "MAGICUBE REPLAY | {playback} | 4 inputs/sec | {}",
            rules_label(state.settings())
        ),
        Color::Cyan,
    )?;
    line(out, 1, width, &format!("Level: {name}"), Color::White)?;
    line(
        out,
        2,
        width,
        &format!(
            "Step {}/{} | {} | ({}, {}) | Air: {}",
            replay.position(),
            replay.len(),
            motion(state),
            player.position.x,
            player.position.y,
            player.air_inputs_remaining,
        ),
        Color::White,
    )?;
    line(
        out,
        3,
        width,
        &format!(
            "Last: {} | Next: {}",
            input_name(replay.last_input()),
            input_name(replay.next_input())
        ),
        Color::White,
    )?;
    line(
        out,
        4,
        width,
        "Left/Right or A/D: -/+1 | Up/Down or PgUp/PgDn: -/+10",
        Color::Grey,
    )?;
    line(
        out,
        5,
        width,
        "Home/End: start/end | Space: play/pause | Q/Esc: close replay",
        Color::Grey,
    )?;
    draw_board(out, (width, height), state, HEADER_ROWS)
}

pub fn rules_label(settings: GameSettings) -> &'static str {
    match (
        settings.allow_airborne_shooting,
        settings.allow_airborne_pushing,
    ) {
        (false, false) => "Shots: grounded | Pushes: grounded",
        (true, false) => "Shots: airborne | Pushes: grounded",
        (false, true) => "Shots: grounded | Pushes: airborne",
        (true, true) => "Shots: airborne | Pushes: airborne",
    }
}

fn begin_frame(out: &mut impl Write, (width, height): (u16, u16)) -> io::Result<bool> {
    queue!(out, ResetColor, MoveTo(0, 0), Clear(ClearType::All))?;
    if width == 0 || height <= HEADER_ROWS {
        if width > 0 && height > 0 {
            line(
                out,
                0,
                width,
                "Resize terminal to see the board (Q quits)",
                Color::Yellow,
            )?;
        }
        out.flush()?;
        return Ok(false);
    }
    Ok(true)
}

fn outside(state: &GameState) -> bool {
    let position = state.player().position;
    position.x < 0
        || position.y < 0
        || position.x >= state.level().width() as isize
        || position.y >= state.level().height() as isize
}

fn motion(state: &GameState) -> &'static str {
    if state.status() == GameStatus::Won {
        "Won"
    } else if state.status() == GameStatus::GameOver {
        "Game over"
    } else if state.player().mode == PlayerMode::Aiming {
        "Aiming"
    } else if state.player().mode == PlayerMode::Recovering {
        "Recovering"
    } else if outside(state) {
        "Outside map"
    } else if state.is_grounded() {
        "Grounded"
    } else if state.player().air_inputs_remaining > 0 {
        "Jumping"
    } else {
        "Falling"
    }
}

fn draw_board(
    out: &mut impl Write,
    (width, height): (u16, u16),
    state: &GameState,
    board_start: u16,
) -> io::Result<()> {
    let player = state.player();
    let level = state.level();
    let columns = width;
    let rows = height - board_start;
    let origin = Position {
        x: camera_origin(player.position.x, level.width(), columns),
        y: camera_origin(player.position.y, level.height(), rows),
    };
    for y in 0..rows {
        queue!(out, MoveTo(0, board_start + y))?;
        for x in 0..columns {
            let position = Position {
                x: origin.x + x as isize,
                y: origin.y + y as isize,
            };
            let mut glyph = state.symbol_at(position);
            // Beams are derived each frame, so level files never contain them.
            if glyph == ' ' {
                glyph = match state.laser_beam_at(position) {
                    Some(LaserBeam::Horizontal) => '-',
                    Some(LaserBeam::Vertical) => '|',
                    Some(LaserBeam::Crossing) => '+',
                    None => ' ',
                };
            }
            let color = match glyph {
                '@' if state.status() == GameStatus::GameOver => Color::Red,
                '@' => Color::Cyan,
                '#' => Color::Grey,
                'D' if state.is_solid(position) => Color::White,
                'D' => Color::DarkGrey,
                'P' => Color::Yellow,
                'C' => Color::Blue,
                'g' => Color::DarkCyan,
                'O' if level.is_goal(position) => Color::Green,
                'O' => Color::Magenta,
                '<' | '>' => Color::Yellow,
                'G' => Color::Green,
                'S' => Color::Red,
                't' => Color::Yellow,
                '{' | '}' | '^' | 'v' | '-' | '|' | '+' => Color::Red,
                'T' if state.laser_trigger_lit(position) => Color::White,
                'T' => Color::DarkGrey,
                '?' => Color::Magenta,
                _ => Color::Reset,
            };
            queue!(out, SetForegroundColor(color), Print(glyph))?;
        }
    }
    queue!(out, ResetColor)?;
    out.flush()
}

/// Keep the map steady when it fits, otherwise follow the player. Open edges
/// remain visible too, so leaving the original map never hides the player.
fn camera_origin(player: isize, map_size: usize, visible: u16) -> isize {
    let visible = visible as isize;
    let last_origin = (map_size as isize - visible).max(0);
    let origin = (player - visible / 2).clamp(0, last_origin);
    origin.min(player).max(player - visible + 1)
}

fn line(out: &mut impl Write, row: u16, width: u16, text: &str, color: Color) -> io::Result<()> {
    // Use single-column ASCII, including for arbitrary level filenames.
    let text: String = text
        .chars()
        .take(usize::from(width))
        .map(|c| {
            if c.is_ascii() && !c.is_ascii_control() {
                c
            } else {
                '?'
            }
        })
        .collect();
    queue!(out, MoveTo(0, row), SetForegroundColor(color), Print(text))
}

fn input_name(input: Option<GameInput>) -> &'static str {
    match input {
        None => "-",
        Some(GameInput::Left) => "left",
        Some(GameInput::Right) => "right",
        Some(GameInput::Jump) => "jump",
        Some(GameInput::Wait) => "wait",
        Some(GameInput::Shoot) => "shoot",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use magicube_solver::GameState;

    #[test]
    fn viewport_keeps_player_visible_in_small_windows_and_outside_map() {
        for map_size in [1, 10, 100] {
            for visible in [1, 5, 20] {
                for player in [-100, -1, 0, 5, 99, 200] {
                    let origin = camera_origin(player, map_size, visible);
                    assert!(origin <= player && player < origin + visible as isize);
                }
            }
        }
        assert_eq!(camera_origin(4, 10, 20), 0);
    }

    #[test]
    fn small_or_zero_size_terminals_render_without_advancing_the_game() {
        let initial = GameState::from_ascii(
            r#"
###
#@#
###
"#
            .trim_matches('\n'),
        )
        .unwrap();
        let app = App::new(initial.clone());
        for size in [(0, 0), (1, 1), (10, 6), (2, 7), (80, 24)] {
            let mut buffer = Vec::new();
            draw(&mut buffer, size, &app, "test").unwrap();
            assert_eq!(app.state, initial);
        }
    }

    #[test]
    fn replay_renders_cursor_inputs_and_controls_without_advancing() {
        let initial = GameState::from_ascii(
            r#"
########
#      #
#@   ###
####G###
"#
            .trim_matches('\n'),
        )
        .unwrap();
        let mut replay = Replay::new(
            initial,
            vec![GameInput::Shoot, GameInput::Right, GameInput::Wait],
        );
        replay.apply(
            crate::replay::Command::Forward(2),
            std::time::Instant::now(),
        );
        let before = replay.state().clone();
        for size in [(0, 0), (1, 1), (10, 6), (2, 7), (80, 24)] {
            let mut buffer = Vec::new();
            draw_replay(&mut buffer, size, &replay, "test").unwrap();
            assert_eq!(replay.state(), &before);
            assert_eq!(replay.position(), 2);
            if size == (80, 24) {
                let output = String::from_utf8(buffer).unwrap();
                for text in [
                    "REPLAY | Paused",
                    "Step 2/3 | Recovering",
                    "Last: right | Next: wait",
                    "Space: play/pause",
                ] {
                    assert!(output.contains(text), "missing {text}");
                }
                assert!(!output.contains("U: undo"));
            }
        }
    }
}
