use std::io::{self, Write};

use crossterm::cursor::MoveTo;
use crossterm::queue;
use crossterm::style::{Color, Print, ResetColor, SetForegroundColor};
use crossterm::terminal::{Clear, ClearType};
use magicube_solver::{GameInput, GameStatus, PlayerMode, Position, Tile};

use crate::app::App;

const HEADER_ROWS: u16 = 6;

pub fn draw(
    out: &mut impl Write,
    (width, height): (u16, u16),
    app: &App,
    name: &str,
) -> io::Result<()> {
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
        return out.flush();
    }

    let player = app.state.player();
    let level = app.state.level();
    let outside = player.position.x < 0
        || player.position.y < 0
        || player.position.x >= level.width() as isize
        || player.position.y >= level.height() as isize;
    let motion = if app.state.status() == GameStatus::Won {
        "Won"
    } else if app.state.status() == GameStatus::GameOver {
        "Game over"
    } else if player.mode == PlayerMode::Aiming {
        "Aiming"
    } else if player.mode == PlayerMode::Recovering {
        "Recovering"
    } else if outside {
        "Outside map"
    } else if app.state.is_grounded() {
        "Grounded"
    } else if player.air_inputs_remaining > 0 {
        "Jumping"
    } else {
        "Falling"
    };

    line(out, 0, width, "MAGICUBE", Color::Cyan)?;
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
        "A/D or Left/Right: move/push | Z: jump | S/Down/.: wait",
        Color::Grey,
    )?;
    line(
        out,
        4,
        width,
        "X: aim/cancel | U: undo | R: restart | P: save | Q/Esc: quit",
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
    } else if outside {
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
            let glyph = app.state.symbol_at(position);
            let color = match glyph {
                '@' if app.state.status() == GameStatus::GameOver => Color::Red,
                '@' => Color::Cyan,
                '#' => Color::Grey,
                'C' => Color::Blue,
                'O' if level.tile_at(position) == Tile::Goal => Color::Green,
                'O' => Color::Magenta,
                '<' | '>' => Color::Yellow,
                'G' => Color::Green,
                'S' => Color::Red,
                't' => Color::Yellow,
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
        let initial = GameState::from_ascii("###\n#@#\n###").unwrap();
        let app = App::new(initial.clone());
        for size in [(0, 0), (1, 1), (10, 6), (2, 7), (80, 24)] {
            let mut buffer = Vec::new();
            draw(&mut buffer, size, &app, "test").unwrap();
            assert_eq!(app.state, initial);
        }
    }
}
