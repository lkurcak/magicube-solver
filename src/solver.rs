//! Shortest-input search using the simulator's exact transition rules.

mod astar;
mod bfs;
mod heuristic;
mod key;

use std::ops::ControlFlow;

use crate::{Direction, GameInput, GameState, GameStatus, PlayerMode};

/// Resource limits and the search algorithm for a single search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolveOptions {
    /// Maximum number of distinct states retained, including the initial state
    /// and a discovered win. `None` allows unlimited search. A zero limit stops
    /// immediately, except when the initial state is already terminal.
    pub max_states: Option<usize>,
    /// Maximum solution cost (see [`solution_cost`]); for [`Algorithm::Bfs`],
    /// the maximum number of search moves, where a whole shot counts once.
    /// `None` allows any length.
    pub max_depth: Option<usize>,
    pub algorithm: Algorithm,
}

impl Default for SolveOptions {
    fn default() -> Self {
        Self {
            max_states: Some(1_000_000),
            max_depth: None,
            algorithm: Algorithm::default(),
        }
    }
}

/// How the state space is explored.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Algorithm {
    /// Breadth-first search over search moves. Moves cost different amounts,
    /// so its solutions are not guaranteed cheapest.
    Bfs,
    /// Best-first search on `g + weight * h`, where `weight_percent` is the
    /// weight in hundredths. With a weight of 100 (1.0) and an admissible
    /// [`Heuristic`] the solution is cheapest. Larger weights
    /// usually search fewer states but may return costlier solutions.
    AStar {
        heuristic: Heuristic,
        weight_percent: u32,
    },
}

/// Uniform-cost search (A* without a heuristic), which finds cheapest solutions.
impl Default for Algorithm {
    fn default() -> Self {
        Self::AStar {
            heuristic: Heuristic::default(),
            weight_percent: 100,
        }
    }
}

impl Algorithm {
    /// Whether a [`SolveOutcome::Solved`] result is guaranteed to be cheapest.
    pub fn is_optimal(self) -> bool {
        match self {
            Self::Bfs => false,
            Self::AStar {
                heuristic,
                weight_percent,
            } => heuristic.is_admissible() && weight_percent <= 100,
        }
    }
}

/// Estimates of the cost still needed to win, for [`Algorithm::AStar`].
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub enum Heuristic {
    /// No estimate: A* becomes uniform-cost search (Dijkstra).
    #[default]
    Zero,
    /// Horizontal distance from the player's cube, a projectile in flight,
    /// or the player (for a new shot) to the nearest goal column.
    GoalDistance,
    /// Horizontal distance from the player to the nearest goal column. It can
    /// overestimate, because shots win from afar, so solutions may be costlier.
    PlayerDistance,
}

impl Heuristic {
    /// Whether the estimate never exceeds the true remaining cost.
    pub fn is_admissible(self) -> bool {
        !matches!(self, Self::PlayerDistance)
    }
}

/// A complete answer or an explicit indication that the search was incomplete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveOutcome {
    /// A winning sequence of inputs, including aiming and recovery waits.
    /// It is cheapest when [`Algorithm::is_optimal`] holds.
    Solved(Vec<GameInput>),
    /// Every reachable nonterminal state was explored, or the start was game over.
    Unsolvable,
    /// Search stopped before storing another state; solvability is still unknown.
    StateLimitReached,
    /// No solution exists within the depth limit, but states at the limit
    /// were left unexpanded, so a longer one may exist.
    DepthLimitReached,
}

/// Deterministic counts of retained states and successor expansions.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SolveStats {
    /// Distinct states retained, including the initial state and any win.
    /// Game-over successors are discarded. Terminal starts require no search
    /// and report zero discovered and expanded states.
    pub discovered_states: usize,
    /// States whose successor generation began, including a partially expanded
    /// state when a win or the resource limit ends the search. A* counts a
    /// state again each time a shorter path reopens it.
    pub expanded_states: usize,
}

/// Search outcome and resource-use statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolveResult {
    pub outcome: SolveOutcome,
    pub stats: SolveStats,
}

/// Finds a winning sequence without changing `initial`, cheapest (see
/// [`solution_cost`]) unless `options` selects BFS or weighted A*.
/// Uses the settings carried by `initial`. [`GameState::from_ascii`] supplies
/// the standard grounded-only rules; non-default settings require an explicit
/// [`GameState::from_ascii_with_settings`] call.
///
/// From a normal state the search takes a whole shot (aim, fire, and the
/// forced recovery `Wait`) as a single move, so aiming and recovering states
/// are never stored. Ties use the deterministic order left, right, jump,
/// shoot left, shoot right, wait. The returned inputs replay directly through
/// [`GameState::step`], even when starting while aiming or recovering.
///
/// Search is synchronous; [`SolveOptions::algorithm`] picks breadth-first
/// (the default) or A*. Map edges wrap, so the reachable state space is
/// finite, though it can be very large. Reaching the configured state limit
/// is therefore distinct from proving that a level is unsolvable.
///
/// ```
/// use magicube_solver::{GameState, GameStatus, SolveOptions, SolveOutcome, solve};
///
/// let initial = GameState::from_ascii(
///     r#"
/// #####
/// #@ ##
/// ##G##
/// "#
///     .trim_matches('\n'),
/// )
/// .unwrap();
/// let result = solve(&initial, SolveOptions::default());
/// let SolveOutcome::Solved(inputs) = result.outcome else {
///     panic!("expected this small level to be solvable");
/// };
/// let won = inputs.into_iter().fold(initial, |state, input| state.step(input));
/// assert_eq!(won.status(), GameStatus::Won);
/// ```
pub fn solve(initial: &GameState, options: SolveOptions) -> SolveResult {
    solve_with_progress(initial, options, |_| {})
}

/// Equivalent to [`solve`], with periodic search statistics for user interfaces.
/// The callback runs after the first expansion and then every 10,000 expanded
/// states. It must return quickly because the search remains single-threaded.
pub fn solve_with_progress(
    initial: &GameState,
    options: SolveOptions,
    mut progress: impl FnMut(SolveStats),
) -> SolveResult {
    solve_cancellable(initial, options, |stats| {
        progress(stats);
        ControlFlow::Continue(())
    })
    .expect("the progress callback never cancels")
}

/// Equivalent to [`solve_with_progress`], except that the callback can stop the
/// search by returning [`ControlFlow::Break`], which returns `None`. Use this
/// for searches without a state limit, which may otherwise run for a very long time.
pub fn solve_cancellable(
    initial: &GameState,
    options: SolveOptions,
    mut progress: impl FnMut(SolveStats) -> ControlFlow<()>,
) -> Option<SolveResult> {
    let immediate = match initial.status() {
        GameStatus::Won => Some(SolveOutcome::Solved(Vec::new())),
        GameStatus::GameOver => Some(SolveOutcome::Unsolvable),
        GameStatus::Playing if options.max_states == Some(0) => {
            Some(SolveOutcome::StateLimitReached)
        }
        GameStatus::Playing => None,
    };
    if let Some(outcome) = immediate {
        return Some(SolveResult {
            outcome,
            stats: SolveStats::default(),
        });
    }
    match options.algorithm {
        Algorithm::Bfs => bfs::search(initial, options, &mut progress),
        Algorithm::AStar {
            heuristic,
            weight_percent,
        } => astar::search(initial, options, heuristic, weight_percent, &mut progress),
    }
}

/// The total cost of replaying `inputs` from `initial`, which solvers
/// minimize: every input costs one, except that a walk pushing cubes costs two.
pub fn solution_cost(initial: &GameState, inputs: &[GameInput]) -> usize {
    let mut state = initial.clone();
    let mut cost = 0;
    for &input in inputs {
        let (next, pushed) = state.step_reporting_push(input);
        cost += 1 + usize::from(pushed);
        state = next;
    }
    cost
}

/// One edge of the search graph.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Move {
    Input(GameInput),
    /// Aim, fire, then the recovery `Wait`.
    Shot(Direction),
    /// Aim and fire, winning before recovery.
    WinningShot(Direction),
}

impl Move {
    fn inputs(self) -> &'static [GameInput] {
        use GameInput::{Left, Right, Shoot, Wait};
        match self {
            Self::Input(Left) => &[Left],
            Self::Input(Right) => &[Right],
            Self::Input(GameInput::Jump) => &[GameInput::Jump],
            Self::Input(Shoot) => &[Shoot],
            Self::Input(Wait) => &[Wait],
            Self::Shot(Direction::Left) => &[Shoot, Left, Wait],
            Self::Shot(Direction::Right) => &[Shoot, Right, Wait],
            Self::WinningShot(Direction::Left) => &[Shoot, Left],
            Self::WinningShot(Direction::Right) => &[Shoot, Right],
        }
    }
}

/// Moves worth trying from a state in `mode`. Searches store only normal
/// states, apart from a start while aiming or recovering, which falls back
/// to single inputs.
fn moves_for(mode: PlayerMode) -> &'static [Move] {
    match mode {
        PlayerMode::Normal => &[
            Move::Input(GameInput::Left),
            Move::Input(GameInput::Right),
            Move::Input(GameInput::Jump),
            Move::Shot(Direction::Left),
            Move::Shot(Direction::Right),
            Move::Input(GameInput::Wait),
        ],
        // Other aiming inputs are no-ops. Every recovery input has the
        // same effect, so emit the explicit wait used by saved solutions.
        PlayerMode::Aiming => &[
            Move::Input(GameInput::Left),
            Move::Input(GameInput::Right),
            Move::Input(GameInput::Shoot),
        ],
        PlayerMode::Recovering => &[Move::Input(GameInput::Wait)],
    }
}

/// The non-fatal successors of `state` with the move recorded for each and
/// its [`solution_cost`]. Shots that cannot aim or are blocked are skipped.
fn successors(state: &GameState) -> impl Iterator<Item = (Move, GameState, u32)> + '_ {
    moves_for(state.player().mode)
        .iter()
        .filter_map(|&action| match action {
            Move::Input(input) => {
                let (next, pushed) = state.step_reporting_push(input);
                Some((action, next, 1 + u32::from(pushed)))
            }
            Move::Shot(direction) => {
                let aiming = state.step(GameInput::Shoot);
                if aiming.player().mode != PlayerMode::Aiming {
                    return None;
                }
                let fired = aiming.step(match direction {
                    Direction::Left => GameInput::Left,
                    Direction::Right => GameInput::Right,
                });
                match (fired.status(), fired.player().mode) {
                    (GameStatus::Won, _) => Some((Move::WinningShot(direction), fired, 2)),
                    (GameStatus::Playing, PlayerMode::Recovering) => {
                        Some((action, fired.step(GameInput::Wait), 3))
                    }
                    _ => None,
                }
            }
            Move::WinningShot(_) => unreachable!("only produced by shots"),
        })
        .filter(|(_, next, _)| next.status() != GameStatus::GameOver)
}

/// Progress is reported after the first expansion and every 10,000 after it.
fn reports_progress(stats: SolveStats) -> bool {
    stats.expanded_states == 1 || stats.expanded_states.is_multiple_of(10_000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn visited_keys_distinguish_airtime_and_modes_with_identical_drawings() {
        let initial = GameState::from_ascii(
            r#"
#####
#   #
# @ #
#####
"#
            .trim_matches('\n'),
        )
        .unwrap();
        let aiming = initial.step(GameInput::Shoot);
        assert_eq!(initial.to_ascii(), aiming.to_ascii());
        let jumping = initial.step(GameInput::Jump);
        let waiting = jumping.step(GameInput::Wait);
        assert_eq!(jumping.to_ascii(), waiting.to_ascii());

        let mut store = key::StateStore::new(&initial);
        for state in [&initial, &aiming, &jumping, &waiting] {
            if store.find(state).is_none() {
                store.insert_pending(0, Move::Input(GameInput::Wait));
            }
        }
        assert_eq!(store.len(), 4);
    }
}
