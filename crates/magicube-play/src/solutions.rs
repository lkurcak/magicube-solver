use std::fs::{self, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use directories::ProjectDirs;
use magicube_solver::{GameInput, GameSettings, GameState, GameStatus};
use serde::{Deserialize, Serialize};

use crate::app::App;
use crate::replay::Replay;

#[derive(Serialize)]
struct Solution<'a> {
    format_version: u32,
    game_version: &'static str,
    settings: RecordedSettings,
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

#[derive(Deserialize)]
struct RecordedSolution {
    format_version: u32,
    level: RecordedLevel,
    status: String,
    inputs: Vec<String>,
    settings: Option<RecordedSettings>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct RecordedSettings {
    allow_airborne_shooting: bool,
    // Absent in version 2; new records always write an explicit boolean.
    allow_airborne_pushing: Option<bool>,
    // Absent before version 4, when projectile speed was fixed at two.
    projectile_tiles_per_update: Option<usize>,
}

impl From<GameSettings> for RecordedSettings {
    fn from(settings: GameSettings) -> Self {
        Self {
            allow_airborne_shooting: settings.allow_airborne_shooting,
            allow_airborne_pushing: Some(settings.allow_airborne_pushing),
            projectile_tiles_per_update: Some(settings.projectile_tiles_per_update),
        }
    }
}

impl RecordedSolution {
    fn game_settings(&self) -> io::Result<GameSettings> {
        match self.format_version {
            // Version 1 predates both grounded-only rules; preserve its physics.
            1 => Ok(GameSettings {
                allow_airborne_shooting: true,
                allow_airborne_pushing: true,
                projectile_tiles_per_update: 2,
            }),
            2..=4 => {
                let settings = self.settings.ok_or_else(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("solution format {} requires settings", self.format_version),
                    )
                })?;
                let allow_airborne_pushing = if self.format_version == 2 {
                    // Version 2 only made shooting configurable.
                    true
                } else {
                    settings.allow_airborne_pushing.ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "solution format 3 requires settings.allow_airborne_pushing",
                        )
                    })?
                };
                let projectile_tiles_per_update = if self.format_version < 4 {
                    2
                } else {
                    settings.projectile_tiles_per_update.ok_or_else(|| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            "solution format 4 requires settings.projectile_tiles_per_update",
                        )
                    })?
                };
                Ok(GameSettings {
                    allow_airborne_shooting: settings.allow_airborne_shooting,
                    allow_airborne_pushing,
                    projectile_tiles_per_update,
                })
            }
            version => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported solution format version {version} (expected 1, 2, 3, or 4)"),
            )),
        }
    }
}

#[derive(Deserialize)]
struct RecordedLevel {
    name: String,
    map: String,
}

pub struct SavedAttempt {
    pub path: PathBuf,
    pub status: String,
    pub input_count: usize,
    pub settings: Option<GameSettings>,
}

pub struct SavedAttempts {
    pub directory: PathBuf,
    pub entries: Vec<SavedAttempt>,
}

fn directory(directory_override: Option<&Path>) -> io::Result<PathBuf> {
    match directory_override {
        Some(path) => std::path::absolute(path),
        None => ProjectDirs::from("", "", "magicube")
            .map(|project| project.data_dir().join("solutions"))
            .ok_or_else(|| {
                io::Error::other("cannot find a user data directory; use --solutions-dir PATH")
            }),
    }
}

/// Browse without creating a save directory or relying on level names/filenames.
pub fn list_for_level(map: &str, directory_override: Option<&Path>) -> io::Result<SavedAttempts> {
    let directory = directory(directory_override)?;
    let initial = GameState::from_ascii(map)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    let files = match fs::read_dir(&directory) {
        Ok(files) => files,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(SavedAttempts {
                directory,
                entries: Vec::new(),
            });
        }
        Err(error) => {
            return Err(io::Error::new(
                error.kind(),
                format!("{}: {error}", directory.display()),
            ));
        }
    };
    let mut entries = Vec::new();
    for file in files {
        let file = file?;
        let path = file.path();
        if path.extension().is_none_or(|extension| extension != "json") {
            continue;
        }
        let Ok(bytes) = fs::read(&path) else { continue };
        let Ok(record) = serde_json::from_slice::<RecordedSolution>(&bytes) else {
            continue;
        };
        if GameState::from_ascii(&record.level.map).ok().as_ref() != Some(&initial) {
            continue;
        }
        let modified = file
            .metadata()
            .and_then(|metadata| metadata.modified())
            .unwrap_or(UNIX_EPOCH);
        entries.push((
            modified,
            SavedAttempt {
                path,
                status: match record.status.as_str() {
                    "won" => "Won",
                    "in_progress" => "Partial",
                    "game_over" => "Game over",
                    _ => "Unknown outcome",
                }
                .to_owned(),
                input_count: record.inputs.len(),
                settings: record.game_settings().ok(),
            },
        ));
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.path.cmp(&b.1.path)));
    Ok(SavedAttempts {
        directory,
        entries: entries.into_iter().map(|(_, entry)| entry).collect(),
    })
}

/// The embedded map is authoritative; replay never needs the original level file.
pub fn load(path: &Path) -> io::Result<(String, Replay)> {
    fs::read(path)
        .and_then(|bytes| decode(&bytes))
        .map_err(|error| io::Error::new(error.kind(), format!("{}: {error}", path.display())))
}

fn decode(bytes: &[u8]) -> io::Result<(String, Replay)> {
    let invalid = |message| io::Error::new(io::ErrorKind::InvalidData, message);
    let record: RecordedSolution = serde_json::from_slice(bytes)
        .map_err(|error| invalid(format!("invalid solution JSON: {error}")))?;
    let initial = GameState::from_ascii_with_settings(&record.level.map, record.game_settings()?)
        .map_err(|error| invalid(format!("invalid embedded level: {error}")))?;
    let inputs = record
        .inputs
        .iter()
        .enumerate()
        .map(|(index, input)| match input.as_str() {
            "left" => Ok(GameInput::Left),
            "right" => Ok(GameInput::Right),
            "jump" => Ok(GameInput::Jump),
            "shoot" => Ok(GameInput::Shoot),
            "wait" => Ok(GameInput::Wait),
            _ => Err(invalid(format!(
                "unknown input {input:?} at step {}",
                index + 1
            ))),
        })
        .collect::<io::Result<Vec<_>>>()?;
    let status = match record.status.as_str() {
        "in_progress" => GameStatus::Playing,
        "won" => GameStatus::Won,
        "game_over" => GameStatus::GameOver,
        _ => return Err(invalid(format!("unknown saved status {:?}", record.status))),
    };
    let replay = Replay::new(initial, inputs);
    if replay.final_state().status() != status {
        return Err(invalid(format!(
            "saved status {:?} does not match replay outcome {:?}; the save may use different game rules",
            record.status,
            replay.final_state().status(),
        )));
    }
    Ok((record.level.name, replay))
}

pub fn save(
    app: &App,
    level_name: &str,
    level_map: &str,
    directory_override: Option<&Path>,
) -> io::Result<PathBuf> {
    // Resolve lazily: an unavailable data directory must not prevent playing.
    let directory = directory(directory_override)?;
    let solution = Solution {
        format_version: 4,
        game_version: env!("CARGO_PKG_VERSION"),
        settings: app.state.settings().into(),
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
        let map = r#"
########
#      #
#@  G###
########
"#
        .trim_start_matches('\n');
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
        assert_eq!(saved["format_version"], 4);
        assert_eq!(saved["settings"]["allow_airborne_shooting"], false);
        assert_eq!(saved["settings"]["allow_airborne_pushing"], false);
        assert_eq!(saved["settings"]["projectile_tiles_per_update"], 3);
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

        let (name, mut loaded) = load(&first).unwrap();
        assert_eq!(name, "../level 1.txt");
        assert_eq!(loaded.position(), 0);
        assert_eq!(loaded.len(), 5);
        loaded.apply(crate::replay::Command::End, std::time::Instant::now());
        assert_eq!(loaded.state(), &app.state);
        loaded.apply(crate::replay::Command::Back(1), std::time::Instant::now());
        assert_eq!(
            loaded.state().player().mode,
            magicube_solver::PlayerMode::Recovering
        );

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

    fn record(map: &str, inputs: &[&str], status: &str) -> Value {
        serde_json::json!({
            "format_version": 4,
            "settings": {
                "allow_airborne_shooting": false,
                "allow_airborne_pushing": false,
                "projectile_tiles_per_update": 3
            },
            "game_version": "0.1.0",
            "level": {"name": "recorded level", "map": map},
            "status": status,
            "inputs": inputs,
        })
    }

    #[test]
    fn loads_empty_partial_aiming_recovering_winning_and_game_over_attempts() {
        let map = r#"
########
#      #
#@  G###
########
"#
        .trim_matches('\n');
        for (inputs, status) in [
            (vec![], "in_progress"),
            (vec!["shoot"], "in_progress"),
            (vec!["shoot", "right"], "in_progress"),
            (vec!["shoot", "right", "wait"], "won"),
        ] {
            let (_, replay) =
                decode(&serde_json::to_vec(&record(map, &inputs, status)).unwrap()).unwrap();
            assert_eq!(replay.len(), inputs.len());
            assert_eq!(replay.state(), &GameState::from_ascii(map).unwrap());
        }
        let death = record(
            r#"
#####
# C #
# @ #
#####
"#
            .trim_matches('\n'),
            &["wait"],
            "game_over",
        );
        let (_, replay) = decode(&serde_json::to_vec(&death).unwrap()).unwrap();
        assert_eq!(replay.final_state().status(), GameStatus::GameOver);
    }

    #[test]
    fn rejects_corrupt_unsupported_or_inconsistent_records() {
        let valid = record(
            r#"
#####
#@G##
#####
"#
            .trim_matches('\n'),
            &["shoot", "right"],
            "won",
        );
        let mut cases = Vec::new();
        let mut version = valid.clone();
        version["format_version"] = 99.into();
        cases.push((version, "unsupported solution format"));
        let mut no_settings = valid.clone();
        no_settings.as_object_mut().unwrap().remove("settings");
        cases.push((no_settings, "requires settings"));
        let mut incomplete_settings = valid.clone();
        incomplete_settings["settings"] = serde_json::json!({});
        cases.push((incomplete_settings, "missing field"));
        for pushing in [None, Some(Value::Null)] {
            let mut incomplete = valid.clone();
            if let Some(value) = pushing {
                incomplete["settings"]["allow_airborne_pushing"] = value;
            } else {
                incomplete["settings"]
                    .as_object_mut()
                    .unwrap()
                    .remove("allow_airborne_pushing");
            }
            cases.push((incomplete, "requires settings.allow_airborne_pushing"));
        }
        let mut incomplete = valid.clone();
        incomplete["settings"]
            .as_object_mut()
            .unwrap()
            .remove("projectile_tiles_per_update");
        cases.push((incomplete, "requires settings.projectile_tiles_per_update"));
        let mut old_no_settings = valid.clone();
        old_no_settings["format_version"] = 2.into();
        old_no_settings.as_object_mut().unwrap().remove("settings");
        cases.push((old_no_settings, "requires settings"));
        let mut input = valid.clone();
        input["inputs"][0] = "teleport".into();
        cases.push((input, "unknown input"));
        let mut map = valid.clone();
        map["level"]["map"] = "invalid".into();
        cases.push((map, "invalid embedded level"));
        let mut status = valid.clone();
        status["status"] = "unknown".into();
        cases.push((status, "unknown saved status"));
        let mut mismatch = valid.clone();
        mismatch["inputs"] = serde_json::json!([]);
        cases.push((mismatch, "does not match replay outcome"));
        let mut missing = valid;
        missing.as_object_mut().unwrap().remove("inputs");
        cases.push((missing, "missing field"));
        for (record, message) in cases {
            let error = decode(&serde_json::to_vec(&record).unwrap()).err().unwrap();
            assert_eq!(error.kind(), io::ErrorKind::InvalidData);
            assert!(error.to_string().contains(message), "{error}");
        }
        assert!(decode(b"not json").is_err());
    }

    #[test]
    fn recorded_rules_control_airborne_shots_and_legacy_saves_keep_old_behavior() {
        let map = r#"
#####
# G##
#@###
#####
"#
        .trim_matches('\n');
        let inputs = ["jump", "shoot", "right"];
        let mut fun = record(map, &inputs, "won");
        fun["settings"]["allow_airborne_shooting"] = true.into();
        let (_, mut replay) = decode(&serde_json::to_vec(&fun).unwrap()).unwrap();
        assert!(replay.state().settings().allow_airborne_shooting);
        replay.apply(crate::replay::Command::End, std::time::Instant::now());
        assert_eq!(replay.state().status(), GameStatus::Won);
        replay.apply(crate::replay::Command::Start, std::time::Instant::now());
        assert!(replay.state().settings().allow_airborne_shooting);

        let mut standard = fun.clone();
        standard["settings"]["allow_airborne_shooting"] = false.into();
        assert!(decode(&serde_json::to_vec(&standard).unwrap()).is_err());
        standard["status"] = "in_progress".into();
        let (_, replay) = decode(&serde_json::to_vec(&standard).unwrap()).unwrap();
        assert!(!replay.state().settings().allow_airborne_shooting);
        assert_eq!(replay.final_state().status(), GameStatus::Playing);

        fun["format_version"] = 1.into();
        fun.as_object_mut().unwrap().remove("settings");
        let (_, legacy) = decode(&serde_json::to_vec(&fun).unwrap()).unwrap();
        assert!(legacy.state().settings().allow_airborne_shooting);
        assert_eq!(legacy.final_state().status(), GameStatus::Won);
    }

    #[test]
    fn recorded_pushing_rules_and_legacy_versions_reproduce_airborne_pushes() {
        let map = r#"
#######
#@OG###
# ### #
#######
"#
        .trim_matches('\n');
        for version in [1, 2, 3, 4] {
            for allow_airborne_pushing in [false, true] {
                let mut saved = record(map, &["right"], "won");
                saved["format_version"] = version.into();
                match version {
                    1 => {
                        saved.as_object_mut().unwrap().remove("settings");
                    }
                    2 => {
                        saved["settings"]
                            .as_object_mut()
                            .unwrap()
                            .remove("allow_airborne_pushing");
                    }
                    3 | 4 => {
                        saved["settings"]["allow_airborne_pushing"] = allow_airborne_pushing.into();
                    }
                    _ => unreachable!(),
                }
                let can_push = version < 3 || allow_airborne_pushing;
                if !can_push {
                    assert!(decode(&serde_json::to_vec(&saved).unwrap()).is_err());
                    saved["status"] = "in_progress".into();
                }
                let (_, mut replay) = decode(&serde_json::to_vec(&saved).unwrap()).unwrap();
                let initial = replay.state().clone();
                assert_eq!(initial.settings().allow_airborne_pushing, can_push);
                assert_eq!(initial.settings().allow_airborne_shooting, version == 1);
                replay.apply(crate::replay::Command::End, std::time::Instant::now());
                assert_eq!(replay.state(), &initial.step(GameInput::Right));
                assert_eq!(replay.state().status() == GameStatus::Won, can_push);
                replay.apply(crate::replay::Command::Start, std::time::Instant::now());
                assert_eq!(replay.state(), &initial);
            }
        }
    }

    #[test]
    fn saves_both_settings_independently_and_preserves_them_on_undo_and_restart() {
        let directory = std::env::temp_dir().join(format!(
            "magicube-pushing-saves-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let map = r#"
#######
#@OG###
# ### #
#######
"#
        .trim_matches('\n');
        for allow_airborne_shooting in [false, true] {
            for allow_airborne_pushing in [false, true] {
                let settings = GameSettings {
                    allow_airborne_shooting,
                    allow_airborne_pushing,
                    ..GameSettings::default()
                };
                let initial = GameState::from_ascii_with_settings(map, settings).unwrap();
                let mut app = App::new(initial.clone());
                app.apply(Command::Step(GameInput::Right));
                let path = save(&app, "push", map, Some(&directory)).unwrap();
                let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
                assert_eq!(saved["format_version"], 4);
                assert_eq!(
                    saved["settings"]["allow_airborne_shooting"],
                    allow_airborne_shooting
                );
                assert_eq!(
                    saved["settings"]["allow_airborne_pushing"],
                    allow_airborne_pushing
                );
                let (_, replay) = load(&path).unwrap();
                assert_eq!(replay.state(), &initial);
                assert_eq!(replay.final_state(), &app.state);
                app.apply(Command::Undo);
                assert_eq!(app.state, initial);
                app.apply(Command::Step(GameInput::Right));
                app.apply(Command::Restart);
                assert_eq!(app.state, initial);
            }
        }
        let attempts = list_for_level(map, Some(&directory)).unwrap();
        assert_eq!(attempts.entries.len(), 4);
        let settings: std::collections::HashSet<_> = attempts
            .entries
            .iter()
            .map(|entry| entry.settings.unwrap())
            .collect();
        assert_eq!(settings.len(), 4);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn saves_and_loads_the_fun_setting() {
        let directory = std::env::temp_dir().join(format!(
            "magicube-fun-saves-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let map = r#"
#####
# G##
#@###
#####
"#
        .trim_matches('\n');
        let initial = GameState::from_ascii_with_settings(
            map,
            GameSettings {
                allow_airborne_shooting: true,
                projectile_tiles_per_update: 4,
                ..GameSettings::default()
            },
        )
        .unwrap();
        let mut app = App::new(initial.clone());
        for input in [GameInput::Jump, GameInput::Shoot, GameInput::Right] {
            app.apply(Command::Step(input));
        }
        assert_eq!(app.state.status(), GameStatus::Won);
        let path = save(&app, "fun", map, Some(&directory)).unwrap();
        let saved: Value = serde_json::from_slice(&fs::read(&path).unwrap()).unwrap();
        assert_eq!(saved["settings"]["allow_airborne_shooting"], true);
        assert_eq!(saved["settings"]["projectile_tiles_per_update"], 4);
        let (_, replay) = load(&path).unwrap();
        assert_eq!(replay.state(), &initial);
        assert_eq!(replay.final_state(), &app.state);
        // The browser groups by map, independent of the saved rules.
        let attempts = list_for_level(map, Some(&directory)).unwrap();
        assert_eq!(attempts.entries.len(), 1);
        assert_eq!(attempts.entries[0].settings, Some(initial.settings()));
        app.apply(Command::Restart);
        assert_eq!(app.state, initial);
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn discovers_matching_attempts_newest_first_without_creating_directories() {
        let directory = std::env::temp_dir().join(format!(
            "magicube-replay-list-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        let map = r#"
#####
#@G##
#####
"#
        .trim_matches('\n');
        let empty = list_for_level(map, Some(&directory)).unwrap();
        assert!(empty.entries.is_empty());
        assert_eq!(empty.directory, directory);
        assert!(!directory.exists());

        fs::create_dir_all(&directory).unwrap();
        let older = directory.join("a-old.json");
        let newer = directory.join("z-new.json");
        fs::write(
            &older,
            serde_json::to_vec(&record(map, &[], "in_progress")).unwrap(),
        )
        .unwrap();
        // Names and newline conventions do not determine level identity.
        let mut won = record(
            &format!("{}\r\n", map.replace('\n', "\r\n")),
            &["shoot", "right"],
            "won",
        );
        won["level"]["name"] = "a different name".into();
        fs::write(&newer, serde_json::to_vec(&won).unwrap()).unwrap();
        for (path, seconds) in [(&older, 1), (&newer, 2)] {
            fs::OpenOptions::new()
                .write(true)
                .open(path)
                .unwrap()
                .set_times(
                    fs::FileTimes::new()
                        .set_modified(UNIX_EPOCH + std::time::Duration::from_secs(seconds)),
                )
                .unwrap();
        }
        fs::write(
            directory.join("unrelated.json"),
            serde_json::to_vec(&record(
                r#"
###
#@#
###
"#
                .trim_matches('\n'),
                &[],
                "in_progress",
            ))
            .unwrap(),
        )
        .unwrap();
        fs::write(directory.join("broken.json"), b"not json").unwrap();
        fs::write(
            directory.join("ignore.txt"),
            serde_json::to_vec(&won).unwrap(),
        )
        .unwrap();
        let attempts = list_for_level(map, Some(&directory)).unwrap();
        assert_eq!(attempts.entries.len(), 2);
        assert_eq!(attempts.entries[0].path, newer);
        assert_eq!(attempts.entries[0].status, "Won");
        assert_eq!(attempts.entries[0].input_count, 2);
        assert_eq!(attempts.entries[1].path, older);
        assert_eq!(attempts.entries[1].status, "Partial");
        assert_eq!(attempts.entries[1].input_count, 0);
        assert!(list_for_level(map, Some(&older)).is_err());
        fs::remove_dir_all(directory).unwrap();
    }
}
