use std::ops::ControlFlow;

use crate::{GameState, GameStatus};

use super::key::StateStore;
use super::{SolveOptions, SolveOutcome, SolveResult, SolveStats, reports_progress, successors};

/// Breadth-first search over moves that tests for a win as each state is
/// generated. It ignores move costs.
/// The store doubles as the FIFO queue, since nodes are appended in order.
pub(super) fn search(
    initial: &GameState,
    options: SolveOptions,
    progress: &mut impl FnMut(SolveStats) -> ControlFlow<()>,
) -> Option<SolveResult> {
    let mut stats = SolveStats::default();
    let mut store = StateStore::new(initial);
    stats.discovered_states = 1;
    let mut cursor = 0;
    // Nodes before `layer_end` are at `depth`; those after it are one deeper.
    let mut depth = 0;
    let mut layer_end = 1;
    while cursor < store.len() {
        if cursor == layer_end {
            depth += 1;
            layer_end = store.len();
        }
        if options.max_depth.is_some_and(|limit| depth >= limit) {
            // Every remaining node is at the limit and stays unexpanded.
            return Some(SolveResult {
                outcome: SolveOutcome::DepthLimitReached,
                stats,
            });
        }
        let node = cursor as u32;
        let state = store.state(node);
        stats.expanded_states += 1;
        if reports_progress(stats) && progress(stats).is_break() {
            return None;
        }
        for (action, next, _) in successors(&state) {
            if store.find(&next).is_some() {
                continue;
            }
            if options.max_states.is_some_and(|limit| store.len() >= limit) {
                return Some(SolveResult {
                    outcome: SolveOutcome::StateLimitReached,
                    stats,
                });
            }
            let child = store.insert_pending(node, action);
            stats.discovered_states += 1;
            if next.status() == GameStatus::Won {
                return Some(SolveResult {
                    outcome: SolveOutcome::Solved(store.path(child)),
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
