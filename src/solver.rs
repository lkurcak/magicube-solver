//! Shortest-input search using the simulator's exact transition rules.

use std::collections::HashSet;
use std::hash::{Hash, Hasher};
use std::ops::ControlFlow;
use std::rc::Rc;

use crate::{GameInput, GameState, GameStatus, PlayerMode};

/// Resource limits for a single search.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SolveOptions {
    /// Maximum number of distinct states retained, including the initial state
    /// and a discovered win. `None` allows unlimited search. A zero limit stops
    /// immediately, except when the initial state is already terminal.
    pub max_states: Option<usize>,
}

impl Default for SolveOptions {
    fn default() -> Self {
        Self {
            max_states: Some(1_000_000),
        }
    }
}

/// A complete answer or an explicit indication that the search was incomplete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SolveOutcome {
    /// A shortest sequence of inputs, including aiming and recovery waits.
    Solved(Vec<GameInput>),
    /// Every reachable nonterminal state was explored, or the start was game over.
    Unsolvable,
    /// Search stopped before storing another state; solvability is still unknown.
    StateLimitReached,
}

/// Deterministic counts of retained states and successor expansions.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct SolveStats {
    /// Distinct states retained, including the initial state and any win.
    /// Game-over successors are discarded. Terminal starts require no search
    /// and report zero discovered and expanded states.
    pub discovered_states: usize,
    /// States whose successor generation began, including a partially expanded
    /// state when a win or the resource limit ends the search.
    pub expanded_states: usize,
}

/// Search outcome and resource-use statistics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SolveResult {
    pub outcome: SolveOutcome,
    pub stats: SolveStats,
}

/// Finds a shortest winning sequence without changing `initial`.
/// Uses the settings carried by `initial`. [`GameState::from_ascii`] supplies
/// the standard grounded-only rules; non-default settings require an explicit
/// [`GameState::from_ascii_with_settings`] call.
///
/// Each input costs one, including entering/cancelling aim and the forced
/// recovery update (represented by `Wait`). Ties use the deterministic order
/// left, right, jump, shoot, wait. The returned inputs replay directly through
/// [`GameState::step`], even when starting while aiming or recovering.
///
/// Search is synchronous and breadth-first. States outside the map remain valid;
/// horizontally unbounded coordinates can prevent exhaustion. Reaching the
/// configured state limit is therefore distinct from proving that a level is
/// unsolvable.
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
/// for searches without a state limit, which may otherwise never finish.
pub fn solve_cancellable(
    initial: &GameState,
    options: SolveOptions,
    mut progress: impl FnMut(SolveStats) -> ControlFlow<()>,
) -> Option<SolveResult> {
    let mut stats = SolveStats::default();
    let immediate = match initial.status() {
        GameStatus::Won => Some(SolveOutcome::Solved(Vec::new())),
        GameStatus::GameOver => Some(SolveOutcome::Unsolvable),
        GameStatus::Playing if options.max_states == Some(0) => {
            Some(SolveOutcome::StateLimitReached)
        }
        GameStatus::Playing => None,
    };
    if let Some(outcome) = immediate {
        return Some(SolveResult { outcome, stats });
    }

    // Nodes serve as both a FIFO queue (via the cursor) and a parent arena.
    // Rc lets the visited set and arena share each state and its cube allocation.
    let initial = Rc::new(SearchState(initial.clone()));
    let mut visited = HashSet::from([Rc::clone(&initial)]);
    let mut nodes = vec![Node {
        state: initial,
        parent: None,
    }];
    stats.discovered_states = 1;
    let mut cursor = 0;
    while cursor < nodes.len() {
        let state = Rc::clone(&nodes[cursor].state);
        stats.expanded_states += 1;
        if (stats.expanded_states == 1 || stats.expanded_states.is_multiple_of(10_000))
            && progress(stats).is_break()
        {
            return None;
        }
        let inputs: &[GameInput] = match state.0.player().mode {
            PlayerMode::Normal => &[
                GameInput::Left,
                GameInput::Right,
                GameInput::Jump,
                GameInput::Shoot,
                GameInput::Wait,
            ],
            // Other aiming inputs are no-ops. Every recovery input has the
            // same effect, so emit the explicit wait used by saved solutions.
            PlayerMode::Aiming => &[GameInput::Left, GameInput::Right, GameInput::Shoot],
            PlayerMode::Recovering => &[GameInput::Wait],
        };
        for &input in inputs {
            let next = SearchState(state.0.step(input));
            if next.0.status() == GameStatus::GameOver || visited.contains(&next) {
                continue;
            }
            if options.max_states.is_some_and(|limit| nodes.len() >= limit) {
                return Some(SolveResult {
                    outcome: SolveOutcome::StateLimitReached,
                    stats,
                });
            }
            let won = next.0.status() == GameStatus::Won;
            let next = Rc::new(next);
            visited.insert(Rc::clone(&next));
            nodes.push(Node {
                state: next,
                parent: Some((cursor, input)),
            });
            stats.discovered_states += 1;
            if won {
                return Some(SolveResult {
                    outcome: SolveOutcome::Solved(reconstruct(&nodes, nodes.len() - 1)),
                    stats,
                });
            }
        }
        cursor += 1;
    }
    Some(SolveResult {
        outcome: SolveOutcome::Unsolvable,
        stats,
    })
}

struct Node {
    state: Rc<SearchState>,
    parent: Option<(usize, GameInput)>,
}

fn reconstruct(nodes: &[Node], mut index: usize) -> Vec<GameInput> {
    let mut inputs = Vec::new();
    while let Some((parent, input)) = nodes[index].parent {
        inputs.push(input);
        index = parent;
    }
    inputs.reverse();
    inputs
}

/// This key is local to one search, where all states share the same level.
/// Do not use GameState's derived hash: it also hashes the entire static map.
/// Cube order is significant because it breaks ties during gravity updates.
#[derive(Debug)]
struct SearchState(GameState);

impl PartialEq for SearchState {
    fn eq(&self, other: &Self) -> bool {
        self.0.player() == other.0.player()
            && self.0.settings() == other.0.settings()
            && self.0.cubes() == other.0.cubes()
            && self.0.projectile() == other.0.projectile()
            && self.0.status() == other.0.status()
    }
}

impl Eq for SearchState {}

impl Hash for SearchState {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.player().hash(state);
        self.0.settings().hash(state);
        self.0.cubes().hash(state);
        self.0.projectile().hash(state);
        self.0.status().hash(state);
    }
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

        let states = HashSet::from([
            SearchState(initial.clone()),
            SearchState(initial),
            SearchState(aiming),
            SearchState(jumping),
            SearchState(waiting),
        ]);
        assert_eq!(states.len(), 4);
    }
}
