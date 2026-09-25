//! Non-interactive solver runs for comparing search algorithms.

use std::error::Error;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::{Duration, Instant};

use clap::{Parser, ValueEnum};
use magicube_solver::project::{ProjectPaths, SolverCacheRecord, read_project};
use magicube_solver::{
    Algorithm, GameState, Heuristic, SolveOptions, SolveOutcome, SolveResult, solution_cost,
    solve_with_progress,
};

/// Solve Magicube levels and report the search effort and time for each.
///
/// Levels come from the project (screenshots and manual labels, imported
/// without writing anything). Solutions found by the progress dashboard
/// are reused unless --no-cache is given. This command never writes the cache.
#[derive(Parser)]
#[command(version)]
struct Cli {
    /// Project level IDs (e.g. 42) or paths to ASCII maps. Defaults to every
    /// clean project level.
    levels: Vec<String>,

    #[arg(long, value_enum, default_value_t = AlgorithmArg::Astar)]
    algorithm: AlgorithmArg,

    /// Lower bound used by A*.
    #[arg(long, value_enum, default_value_t = HeuristicArg::Zero)]
    heuristic: HeuristicArg,

    /// A* heuristic weight. Above 1 usually searches fewer states but may
    /// return costlier solutions.
    #[arg(long, default_value_t = 1.0, value_parser = parse_weight)]
    weight: f64,

    /// Maximum distinct states per level, or "unlimited".
    #[arg(long, default_value = "1000000", value_parser = parse_limit)]
    max_states: Limit,

    /// Maximum solution cost (BFS: moves), or "unlimited".
    #[arg(long, default_value = "unlimited", value_parser = parse_limit)]
    max_depth: Limit,

    /// Search every level, even if the cache holds a valid solution.
    #[arg(long)]
    no_cache: bool,
}

#[derive(Clone, Copy, ValueEnum)]
enum AlgorithmArg {
    Bfs,
    Astar,
}

#[derive(Clone, Copy, ValueEnum)]
enum HeuristicArg {
    Zero,
    GoalDistance,
    PlayerDistance,
}

/// `None` is unlimited. A newtype keeps clap from treating it as optional.
#[derive(Clone, Copy)]
struct Limit(Option<usize>);

fn parse_limit(value: &str) -> Result<Limit, String> {
    if value.eq_ignore_ascii_case("unlimited") {
        return Ok(Limit(None));
    }
    let digits = value.replace('_', "");
    digits
        .parse()
        .map(|limit| Limit(Some(limit)))
        .map_err(|_| format!("expected a number or \"unlimited\", got {value:?}"))
}

fn parse_weight(value: &str) -> Result<f64, String> {
    match value.parse::<f64>() {
        Ok(weight) if (0.0..=1000.0).contains(&weight) => Ok(weight),
        _ => Err(format!(
            "expected a weight between 0 and 1000, got {value:?}"
        )),
    }
}

struct Level {
    id: String,
    map: String,
    cache: Option<PathBuf>,
}

fn main() -> ExitCode {
    match run(Cli::parse()) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(error) => {
            eprintln!("magicube-solver: {error}");
            ExitCode::FAILURE
        }
    }
}

/// Returns whether every requested level could be loaded and parsed.
fn run(cli: Cli) -> Result<bool, Box<dyn Error>> {
    let algorithm = match cli.algorithm {
        AlgorithmArg::Bfs => Algorithm::Bfs,
        AlgorithmArg::Astar => Algorithm::AStar {
            heuristic: match cli.heuristic {
                HeuristicArg::Zero => Heuristic::Zero,
                HeuristicArg::GoalDistance => Heuristic::GoalDistance,
                HeuristicArg::PlayerDistance => Heuristic::PlayerDistance,
            },
            weight_percent: (cli.weight * 100.0).round() as u32,
        },
    };
    let options = SolveOptions {
        max_states: cli.max_states.0,
        max_depth: cli.max_depth.0,
        algorithm,
    };
    let paths = ProjectPaths::from_root(Path::new(env!("CARGO_MANIFEST_DIR")));
    let (levels, mut ok) = levels(&cli.levels, &paths)?;

    println!(
        "{:<10} {:<14} {:>7} {:>6} {:>12} {:>12} {:>10}",
        "level", "outcome", "inputs", "cost", "discovered", "expanded", "time"
    );
    let progress_line = io::stderr().is_terminal();
    let mut total = Duration::ZERO;
    let mut searched = 0;
    for level in levels {
        let initial = match GameState::from_ascii(&level.map) {
            Ok(initial) => initial,
            Err(error) => {
                println!("{:<10} invalid map: {error}", level.id);
                ok = false;
                continue;
            }
        };
        let cached = level
            .cache
            .filter(|_| !cli.no_cache)
            .and_then(|path| SolverCacheRecord::load(&path).ok())
            .and_then(|record| record.solved_inputs_for(&level.map));
        if let Some(inputs) = cached {
            println!(
                "{:<10} {:<14} {:>7} {:>6} {:>12} {:>12} {:>10}",
                level.id,
                "solved",
                inputs.len(),
                solution_cost(&initial, &inputs),
                "-",
                "-",
                "cached"
            );
            continue;
        }
        let start = Instant::now();
        let result = solve_with_progress(&initial, options, |stats| {
            if progress_line {
                eprint!(
                    "\r\x1b[K{}: {} discovered, {} expanded, {:.1?}",
                    level.id,
                    stats.discovered_states,
                    stats.expanded_states,
                    start.elapsed()
                );
            }
        });
        let elapsed = start.elapsed();
        if progress_line {
            eprint!("\r\x1b[K");
        }
        total += elapsed;
        searched += 1;
        print_result(&level.id, &initial, &result, elapsed);
        io::stdout().flush()?;
    }
    println!(
        "\n{searched} searched in {total:.2?} with {}; solutions are {}.",
        describe(options),
        if algorithm.is_optimal() {
            "cheapest"
        } else {
            "not guaranteed cheapest"
        }
    );
    Ok(ok)
}

fn print_result(id: &str, initial: &GameState, result: &SolveResult, elapsed: Duration) {
    let (outcome, inputs, cost) = match &result.outcome {
        SolveOutcome::Solved(inputs) => (
            "solved",
            inputs.len().to_string(),
            solution_cost(initial, inputs).to_string(),
        ),
        SolveOutcome::Unsolvable => ("unsolvable", "-".to_owned(), "-".to_owned()),
        SolveOutcome::StateLimitReached => ("state limit", "-".to_owned(), "-".to_owned()),
        SolveOutcome::DepthLimitReached => ("depth limit", "-".to_owned(), "-".to_owned()),
    };
    println!(
        "{:<10} {:<14} {:>7} {:>6} {:>12} {:>12} {:>10}",
        id,
        outcome,
        inputs,
        cost,
        result.stats.discovered_states,
        result.stats.expanded_states,
        format!("{elapsed:.2?}")
    );
}

fn describe(options: SolveOptions) -> String {
    let limit = |limit: Option<usize>| limit.map_or("unlimited".to_owned(), |n| n.to_string());
    let algorithm = match options.algorithm {
        Algorithm::Bfs => "bfs".to_owned(),
        Algorithm::AStar {
            heuristic,
            weight_percent,
        } => format!(
            "astar ({heuristic:?}, weight {:.2})",
            f64::from(weight_percent) / 100.0
        ),
    };
    format!(
        "{algorithm}, max states {}, max depth {}",
        limit(options.max_states),
        limit(options.max_depth)
    )
}

/// Resolves arguments to maps. An argument naming an existing file is a map
/// path; anything else is a project level ID. Unknown or unclean levels are
/// reported and make the run fail, but do not stop the others.
fn levels(
    arguments: &[String],
    paths: &ProjectPaths,
) -> Result<(Vec<Level>, bool), Box<dyn Error>> {
    let needs_project = arguments.is_empty() || arguments.iter().any(|a| !Path::new(a).is_file());
    let project = if needs_project {
        read_project(paths)?.levels
    } else {
        Vec::new()
    };
    let cache = |id: &str| Some(paths.solver_cache().join(format!("{id}.json")));
    if arguments.is_empty() {
        let levels = project
            .into_iter()
            .filter(|level| level.is_clean())
            .map(|level| Level {
                cache: cache(&level.id),
                map: level.map.unwrap(),
                id: level.id,
            })
            .collect();
        return Ok((levels, true));
    }
    let mut ok = true;
    let mut levels = Vec::new();
    for argument in arguments {
        let path = Path::new(argument);
        if path.is_file() {
            levels.push(Level {
                id: argument.clone(),
                map: std::fs::read_to_string(path)?,
                cache: None,
            });
            continue;
        }
        match project.iter().find(|level| level.id == *argument) {
            Some(level) if level.is_clean() => levels.push(Level {
                id: level.id.clone(),
                map: level.map.clone().unwrap(),
                cache: cache(&level.id),
            }),
            Some(level) => {
                eprintln!(
                    "magicube-solver: level {argument} is not clean: {}",
                    level.issues.join("; ")
                );
                ok = false;
            }
            None => {
                eprintln!("magicube-solver: no project level or file named {argument:?}");
                ok = false;
            }
        }
    }
    Ok((levels, ok))
}
