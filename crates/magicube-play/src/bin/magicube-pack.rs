//! Pack every project level's solution, with pre-rendered frames, into one
//! JSON file for the static solutions website.

use std::error::Error;
use std::fs;
use std::io::{self, IsTerminal};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use crossterm::style::Color;
use magicube_play::render;
use magicube_solver::project::{ProjectPaths, SolverCacheRecord, read_project};
use magicube_solver::{
    GameInput, GameState, GameStatus, PlayerMode, Position, SolveOptions, SolveOutcome,
    solution_cost, solve_with_progress,
};
use serde::{Deserialize, Serialize};

const PACK_FORMAT_VERSION: u32 = 1;

const USAGE: &str = "usage: magicube-pack [--output PATH] [--max-states N|unlimited] [--resolve]

Writes docs/solutions.json by default. Solutions already in the output file or
in the progress dashboard's cache are reused when they still win on the current
map; --resolve searches every level again. The default state limit is unlimited.";

#[derive(Serialize, Deserialize)]
struct Pack {
    format_version: u32,
    levels: Vec<PackedLevel>,
}

#[derive(Serialize, Deserialize)]
struct PackedLevel {
    id: String,
    name: String,
    /// "solved", "unsolvable", "state limit", "depth limit" or "corrupted".
    status: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    issues: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    map: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cost: Option<usize>,
    /// Raw solver inputs, one letter each: L, R, J (jump), S (shoot), W (wait).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    inputs: String,
    /// What each input does, one letter each: L, R, J, A (aim), C (cancel
    /// aim), < and > (fire), W (wait), ~ (automatic recovery after a shot).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    actions: String,
    /// The initial state followed by the state after every input. Rows are
    /// padded to the map width and joined with newlines.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    frames: Vec<Frame>,
}

#[derive(Serialize, Deserialize)]
struct Frame {
    text: String,
    /// One color class per character of `text`, newlines included.
    colors: String,
}

struct Options {
    output: PathBuf,
    max_states: Option<usize>,
    resolve: bool,
}

fn main() -> ExitCode {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crate lives two levels below the workspace root");
    let options = match parse_args(&root) {
        Ok(options) => options,
        Err(message) => {
            eprintln!("magicube-pack: {message}\n\n{USAGE}");
            return ExitCode::FAILURE;
        }
    };
    match run(&root, &options) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("magicube-pack: {error}");
            ExitCode::FAILURE
        }
    }
}

fn parse_args(root: &Path) -> Result<Options, String> {
    let mut options = Options {
        output: root.join("docs/solutions.json"),
        max_states: None,
        resolve: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--output" => {
                options.output = args.next().ok_or("--output needs a path")?.into();
            }
            "--max-states" => {
                let value = args.next().ok_or("--max-states needs a value")?;
                options.max_states = if value.eq_ignore_ascii_case("unlimited") {
                    None
                } else {
                    Some(
                        value
                            .replace('_', "")
                            .parse()
                            .map_err(|_| format!("invalid state limit {value:?}"))?,
                    )
                };
            }
            "--resolve" => options.resolve = true,
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    Ok(options)
}

/// Returns whether every manifest level was packed with a solution.
fn run(root: &Path, options: &Options) -> Result<bool, Box<dyn Error>> {
    let paths = ProjectPaths::from_root(root);
    let previous = if options.resolve {
        None
    } else {
        load_previous(&options.output)?
    };
    let solve_options = SolveOptions {
        max_states: options.max_states,
        ..SolveOptions::default()
    };
    let progress_line = io::stderr().is_terminal();

    let mut levels = Vec::new();
    let mut complete = true;
    for level in read_project(&paths)?.levels {
        let corrupted = |map| PackedLevel {
            id: level.id.clone(),
            name: level.name.clone(),
            status: "corrupted".to_owned(),
            issues: level.issues.clone(),
            map,
            cost: None,
            inputs: String::new(),
            actions: String::new(),
            frames: Vec::new(),
        };
        let Some(map) = level.map.clone().filter(|_| level.is_clean()) else {
            eprintln!("{}: corrupted: {}", level.id, level.issues.join("; "));
            levels.push(corrupted(level.map.clone()));
            complete = false;
            continue;
        };
        let initial = match GameState::from_ascii(&map) {
            Ok(initial) => initial,
            Err(error) => {
                eprintln!("{}: invalid map: {error}", level.id);
                levels.push(corrupted(Some(map)));
                complete = false;
                continue;
            }
        };

        let reused = previous
            .as_ref()
            .and_then(|pack| pack.levels.iter().find(|packed| packed.id == level.id))
            .and_then(|packed| parse_inputs(&packed.inputs))
            .filter(|inputs| wins(&initial, inputs))
            .map(|inputs| (inputs, "pack"))
            .or_else(|| {
                let path = paths.solver_cache().join(format!("{}.json", level.id));
                SolverCacheRecord::load(&path)
                    .ok()
                    .and_then(|record| record.solved_inputs_for(&map))
                    .map(|inputs| (inputs, "cache"))
            })
            .filter(|_| !options.resolve);

        let outcome = match reused {
            Some((inputs, source)) => {
                eprintln!("{}: reused {} inputs from {source}", level.id, inputs.len());
                Ok(inputs)
            }
            None => {
                let start = Instant::now();
                let result = solve_with_progress(&initial, solve_options, |stats| {
                    if progress_line {
                        eprint!(
                            "\r\x1b[K{}: {} discovered, {:.1?}",
                            level.id,
                            stats.discovered_states,
                            start.elapsed()
                        );
                    }
                });
                if progress_line {
                    eprint!("\r\x1b[K");
                }
                eprintln!(
                    "{}: {} in {:.2?}",
                    level.id,
                    outcome_name(&result.outcome),
                    start.elapsed()
                );
                match result.outcome {
                    SolveOutcome::Solved(inputs) => Ok(inputs),
                    other => Err(outcome_name(&other)),
                }
            }
        };

        levels.push(match outcome {
            Ok(inputs) => pack_solution(&level.id, &level.name, &map, initial, &inputs),
            Err(status) => {
                complete = false;
                PackedLevel {
                    status: status.to_owned(),
                    issues: Vec::new(),
                    ..corrupted(Some(map))
                }
            }
        });
    }

    let solved = levels
        .iter()
        .filter(|level| level.status == "solved")
        .count();
    let pack = Pack {
        format_version: PACK_FORMAT_VERSION,
        levels,
    };
    if let Some(parent) = options.output.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut bytes = serde_json::to_vec(&pack)?;
    bytes.push(b'\n');
    fs::write(&options.output, &bytes)?;
    eprintln!(
        "wrote {} ({solved}/{} solved, {} KiB)",
        options.output.display(),
        pack.levels.len(),
        bytes.len() / 1024
    );
    Ok(complete)
}

fn load_previous(path: &Path) -> Result<Option<Pack>, Box<dyn Error>> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("{}: {error}", path.display()).into()),
    };
    // An outdated or damaged pack is simply rebuilt.
    Ok(serde_json::from_slice::<Pack>(&bytes)
        .ok()
        .filter(|pack| pack.format_version == PACK_FORMAT_VERSION))
}

fn pack_solution(
    id: &str,
    name: &str,
    map: &str,
    initial: GameState,
    inputs: &[GameInput],
) -> PackedLevel {
    let cost = solution_cost(&initial, inputs);
    let mut actions = String::new();
    let mut frames = vec![frame(&initial)];
    let mut state = initial;
    for &input in inputs {
        actions.push(action(&state, input));
        state = state.step(input);
        frames.push(frame(&state));
    }
    PackedLevel {
        id: id.to_owned(),
        name: name.to_owned(),
        status: "solved".to_owned(),
        issues: Vec::new(),
        map: Some(map.to_owned()),
        cost: Some(cost),
        inputs: inputs.iter().map(|&input| input_letter(input)).collect(),
        actions,
        frames,
    }
}

fn frame(state: &GameState) -> Frame {
    let level = state.level();
    let mut text = String::new();
    let mut colors = String::new();
    for y in 0..level.height() {
        if y > 0 {
            text.push('\n');
            colors.push('\n');
        }
        for x in 0..level.width() {
            let position = Position {
                x: x as isize,
                y: y as isize,
            };
            let (glyph, color) = render::cell(state, position);
            text.push(glyph);
            colors.push(if glyph == ' ' {
                '.'
            } else {
                color_class(color)
            });
        }
    }
    Frame { text, colors }
}

/// Single-letter names for the terminal colors the renderer uses.
fn color_class(color: Color) -> char {
    match color {
        Color::Grey => 'w',
        Color::White => 'W',
        Color::DarkGrey => 'k',
        Color::Red => 'r',
        Color::DarkRed => 'R',
        Color::Green => 'g',
        Color::Yellow => 'y',
        Color::Blue => 'b',
        Color::DarkBlue => 'B',
        Color::Magenta => 'm',
        Color::Cyan => 'c',
        Color::DarkCyan => 'C',
        _ => '.',
    }
}

fn action(state: &GameState, input: GameInput) -> char {
    match (state.player().mode, input) {
        (PlayerMode::Recovering, _) => '~',
        (PlayerMode::Aiming, GameInput::Left) => '<',
        (PlayerMode::Aiming, GameInput::Right) => '>',
        (PlayerMode::Aiming, GameInput::Shoot) => 'C',
        (_, GameInput::Shoot) => 'A',
        (_, input) => input_letter(input),
    }
}

fn input_letter(input: GameInput) -> char {
    match input {
        GameInput::Left => 'L',
        GameInput::Right => 'R',
        GameInput::Jump => 'J',
        GameInput::Shoot => 'S',
        GameInput::Wait => 'W',
    }
}

fn parse_inputs(letters: &str) -> Option<Vec<GameInput>> {
    letters
        .chars()
        .map(|letter| match letter {
            'L' => Some(GameInput::Left),
            'R' => Some(GameInput::Right),
            'J' => Some(GameInput::Jump),
            'S' => Some(GameInput::Shoot),
            'W' => Some(GameInput::Wait),
            _ => None,
        })
        .collect()
}

fn wins(initial: &GameState, inputs: &[GameInput]) -> bool {
    let mut state = initial.clone();
    for &input in inputs {
        state = state.step(input);
    }
    state.status() == GameStatus::Won
}

fn outcome_name(outcome: &SolveOutcome) -> &'static str {
    match outcome {
        SolveOutcome::Solved(_) => "solved",
        SolveOutcome::Unsolvable => "unsolvable",
        SolveOutcome::StateLimitReached => "state limit",
        SolveOutcome::DepthLimitReached => "depth limit",
    }
}
