use std::cmp::Ordering;
use std::collections::BinaryHeap;
use std::ops::ControlFlow;

use crate::{GameState, GameStatus};

use super::heuristic::Estimator;
use super::key::StateStore;
use super::{
    Heuristic, SolveOptions, SolveOutcome, SolveResult, SolveStats, reports_progress, successors,
};

/// Best-first search on `100 * g + weight_percent * h`. A win is accepted
/// when popped, so with weight 100 and an admissible heuristic it is cheapest.
/// A shorter path to a known state updates it and queues it again, which
/// keeps that guarantee even if the heuristic is inconsistent.
pub(super) fn search(
    initial: &GameState,
    options: SolveOptions,
    heuristic: Heuristic,
    weight_percent: u32,
    progress: &mut impl FnMut(SolveStats) -> ControlFlow<()>,
) -> Option<SolveResult> {
    let estimator = Estimator::new(heuristic, initial);
    let priority = |cost: u32, state: &GameState| {
        100 * u64::from(cost) + u64::from(weight_percent) * u64::from(estimator.estimate(state))
    };
    let mut stats = SolveStats::default();
    let mut store = StateStore::new(initial);
    stats.discovered_states = 1;
    let mut costs = vec![0_u32];
    let mut open = BinaryHeap::new();
    let mut sequence = 0;
    open.push(Entry {
        priority: priority(0, initial),
        cost: 0,
        sequence,
        node: 0,
    });
    let mut depth_cut_off = false;
    while let Some(entry) = open.pop() {
        if entry.cost != costs[entry.node as usize] {
            continue; // Superseded by a shorter path.
        }
        let state = store.state(entry.node);
        if state.status() == GameStatus::Won {
            return Some(SolveResult {
                outcome: SolveOutcome::Solved(store.path(entry.node)),
                stats,
            });
        }
        if options
            .max_depth
            .is_some_and(|limit| entry.cost as usize >= limit)
        {
            depth_cut_off = true;
            continue;
        }
        stats.expanded_states += 1;
        if reports_progress(stats) && progress(stats).is_break() {
            return None;
        }
        for (action, next, step_cost) in successors(&state) {
            let cost = entry.cost + step_cost;
            if options.max_depth.is_some_and(|limit| cost as usize > limit) {
                depth_cut_off = true;
                continue;
            }
            let node = match store.find(&next) {
                Some(node) if cost < costs[node as usize] => {
                    store.set_parent(node, entry.node, action);
                    costs[node as usize] = cost;
                    node
                }
                Some(_) => continue,
                None => {
                    if options.max_states.is_some_and(|limit| store.len() >= limit) {
                        return Some(SolveResult {
                            outcome: SolveOutcome::StateLimitReached,
                            stats,
                        });
                    }
                    stats.discovered_states += 1;
                    costs.push(cost);
                    store.insert_pending(entry.node, action)
                }
            };
            sequence += 1;
            open.push(Entry {
                priority: priority(cost, &next),
                cost,
                sequence,
                node,
            });
        }
    }
    Some(SolveResult {
        outcome: if depth_cut_off {
            SolveOutcome::DepthLimitReached
        } else {
            SolveOutcome::Unsolvable
        },
        stats,
    })
}

/// Pops the lowest priority first, preferring deeper nodes and then
/// earlier insertions, so ties are deterministic.
#[derive(PartialEq, Eq)]
struct Entry {
    priority: u64,
    cost: u32,
    sequence: u64,
    node: u32,
}

impl Ord for Entry {
    fn cmp(&self, other: &Self) -> Ordering {
        other
            .priority
            .cmp(&self.priority)
            .then(self.cost.cmp(&other.cost))
            .then(other.sequence.cmp(&self.sequence))
    }
}

impl PartialOrd for Entry {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}
