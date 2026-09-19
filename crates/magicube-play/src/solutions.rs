use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use magicube_solver::{GameInput, GameStatus};
use serde::Serialize;

use crate::app::App;

#[derive(Serialize)]
struct Solution<'a> {
    format_version: u32,
    game_version: &'static str,
    level: SavedLevel<'a>,
    status: &'static str,
    inputs: Vec<&'static str>,
}

#[derive(Serialize)]
struct SavedLevel<'a> {
    name: &'a str,
    // Keep the original map, including whitespace and dimensions, for replay.
    map: &'a str,
}

pub fn save(
    app: &App,
    level_name: &str,
    level_map: &str,
    directory_override: Option<&Path>,
) -> io::Result<PathBuf> {
    // Resolve lazily: an unavailable data directory must not prevent playing.
    let directory = match directory_override {
        Some(path) => std::path::absolute(path)?,
        None => ProjectDirs::from("", "", "magicube")
            .ok_or_else(|| {
                io::Error::other("cannot find a user data directory; use --solutions-dir PATH")
            })?
            .data_dir()
            .join("solutions"),
    };
    let solution = Solution {
        format_version: 1,
        game_version: env!("CARGO_PKG_VERSION"),
        level: SavedLevel {
            name: level_name,
            map: level_map,
        },
        status: match app.state.status() {
            GameStatus::Playing => "in_progress",
            GameStatus::Won => "won",
            GameStatus::GameOver => "game_over",
        },
        inputs: app.inputs().map(input_name).collect(),
    };
    let mut json = serde_json::to_vec_pretty(&solution).map_err(io::Error::other)?;
    json.push(b'\n');
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(io::Error::other)?
        .as_millis();
    let stem = filename_stem(level_name);
    fs::create_dir_all(&directory)?;
    for suffix in 0_u64.. {
        let path = directory.join(format!("{stem}-{timestamp}-{suffix}.json"));
        // Exclusive creation prevents overwriting earlier or concurrent saves.
        let mut file = match OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => file,
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        };
        if let Err(error) = file.write_all(&json).and_then(|()| file.sync_all()) {
            drop(file);
            let _ = fs::remove_file(&path);
            return Err(error);
        }
        return Ok(path);
    }
    Err(io::Error::other(
        "could not allocate a unique solution filename",
    ))
}

fn input_name(input: GameInput) -> &'static str {
    match input {
        GameInput::Left => "left",
        GameInput::Right => "right",
        GameInput::Jump => "jump",
        GameInput::Shoot => "shoot",
        GameInput::Wait => "wait",
    }
}

fn filename_stem(name: &str) -> String {
    let name = Path::new(name)
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy();
    let stem = name
        .split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .to_ascii_lowercase();
    let stem = stem.chars().take(60).collect::<String>();
    if stem.is_empty() {
        "level".to_owned()
    } else {
        stem
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::Command;
    use magicube_solver::GameState;
    use serde_json::Value;

    #[test]
    fn saves_replayable_current_branch_and_restart_without_overwriting() {
        let directory = std::env::temp_dir().join(format!(
            "magicube-solutions-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let map = "#######\n#     #\n#@ G###\n#######\n";
        let mut app = App::new(GameState::from_ascii(map).unwrap());
        for input in [GameInput::Jump, GameInput::Right, GameInput::Wait] {
            app.apply(Command::Step(input));
        }
        for _ in 0..3 {
            app.apply(Command::Undo);
        }
        for input in [
            GameInput::Shoot,
            GameInput::Shoot,
            GameInput::Shoot,
            GameInput::Right,
        ] {
            app.apply(Command::Step(input));
        }
        app.advance_recovery();
        assert_eq!(app.state.status(), GameStatus::Won);
        let before = app.state.clone();
        let first = save(&app, "../level 1.txt", map, Some(&directory)).unwrap();
        let bytes = fs::read(&first).unwrap();
        let saved: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(saved["format_version"], 1);
        assert_eq!(saved["status"], "won");
        assert_eq!(saved["level"]["map"], map);
        assert_eq!(
            saved["inputs"],
            serde_json::json!(["shoot", "shoot", "shoot", "right", "wait"])
        );
        let mut replay = GameState::from_ascii(saved["level"]["map"].as_str().unwrap()).unwrap();
        for input in saved["inputs"].as_array().unwrap() {
            let input = match input.as_str().unwrap() {
                "left" => GameInput::Left,
                "right" => GameInput::Right,
                "jump" => GameInput::Jump,
                "shoot" => GameInput::Shoot,
                "wait" => GameInput::Wait,
                other => panic!("unknown saved input: {other}"),
            };
            replay = replay.step(input);
        }
        assert_eq!(replay, app.state);
        assert_eq!(app.state, before);

        let second = save(&app, "../level 1.txt", map, Some(&directory)).unwrap();
        assert_ne!(first, second);
        assert_eq!(first.parent(), Some(directory.as_path()));
        assert_eq!(fs::read(&first).unwrap(), bytes);

        app.apply(Command::Restart);
        let restarted = save(&app, "level", map, Some(&directory)).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(restarted).unwrap()).unwrap();
        assert_eq!(saved["inputs"], serde_json::json!([]));
        assert_eq!(saved["status"], "in_progress");
        // A file used as a directory is a recoverable save error.
        assert!(save(&app, "level", map, Some(&first)).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
