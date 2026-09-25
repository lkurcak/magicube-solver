//! Estimates of the cost still needed to win.

use crate::{CubeSource, GameState, GameStatus, PlayerMode, Position, Tile};

use super::Heuristic;

/// A heuristic prepared for one level.
pub(super) enum Estimator {
    Zero,
    GoalDistance(GoalDistance),
    PlayerDistance(GoalDistance),
}

impl Estimator {
    pub fn new(heuristic: Heuristic, initial: &GameState) -> Self {
        match heuristic {
            Heuristic::Zero => Self::Zero,
            Heuristic::GoalDistance => Self::GoalDistance(GoalDistance::new(initial)),
            Heuristic::PlayerDistance => Self::PlayerDistance(GoalDistance::new(initial)),
        }
    }

    pub fn estimate(&self, state: &GameState) -> u32 {
        match self {
            Self::Zero => 0,
            Self::GoalDistance(goal) => goal.estimate(state),
            Self::PlayerDistance(goal) => goal.player_distance(state),
        }
    }
}

/// Winning needs the player's cube resting above a pedestal. Only three
/// things can bring it there, so the cheapest one bounds the remaining cost:
///
/// - the existing cube moves sideways only when pushed, one tile per push,
///   and each push costs two;
/// - a projectile in flight covers at most `projectile_tiles_per_update`
///   tiles per input before becoming the cube;
/// - a new shot needs the player to finish recovering and aim first, then
///   the same projectile travel from the player's column.
///
/// Walking before firing or pushing a spawned cube afterwards costs at least
/// one per tile, so by the triangle inequality neither beats these
/// bounds. Distances are horizontal and wrap around the map edges; falling
/// and jumping are vertical and free.
pub(super) struct GoalDistance {
    /// Wrapped horizontal distance from each column to the nearest goal column.
    /// Empty when the level has no goal.
    columns: Vec<u32>,
    tiles_per_update: u32,
}

impl GoalDistance {
    fn new(initial: &GameState) -> Self {
        let level = initial.level();
        let width = level.width();
        let goals: Vec<usize> = (0..width)
            .filter(|&x| {
                (0..level.height()).any(|y| {
                    level.tile_at(Position {
                        x: x as isize,
                        y: y as isize,
                    }) == Tile::Pedestal
                })
            })
            .collect();
        let columns = if goals.is_empty() {
            Vec::new()
        } else {
            (0..width)
                .map(|x| {
                    goals
                        .iter()
                        .map(|&goal| {
                            let distance = x.abs_diff(goal);
                            distance.min(width - distance) as u32
                        })
                        .min()
                        .unwrap()
                })
                .collect()
        };
        Self {
            columns,
            tiles_per_update: initial.settings().projectile_tiles_per_update.max(1) as u32,
        }
    }

    /// Horizontal distance from the player to the nearest goal column. Not a
    /// lower bound: a shot can win from far away.
    fn player_distance(&self, state: &GameState) -> u32 {
        if state.status() == GameStatus::Won || self.columns.is_empty() {
            return 0;
        }
        self.columns[state.player().position.x as usize]
    }

    fn estimate(&self, state: &GameState) -> u32 {
        if state.status() == GameStatus::Won || self.columns.is_empty() {
            return 0;
        }
        let distance = |position: Position| self.columns[position.x as usize];
        let flight = |position: Position| distance(position).div_ceil(self.tiles_per_update).max(1);
        let player = state.player();
        let before_firing = match player.mode {
            PlayerMode::Aiming => 0,
            PlayerMode::Normal => 1,
            PlayerMode::Recovering => 2,
        };
        let mut bound = before_firing + flight(player.position);
        if let Some(projectile) = state.projectile() {
            bound = bound.min(flight(projectile.position));
        }
        for cube in state.cubes() {
            if cube.source == CubeSource::Player {
                bound = bound.min((2 * distance(cube.position)).max(1));
            }
        }
        bound
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Algorithm, GameInput, SolveOptions, SolveOutcome, solution_cost, solve};

    /// Compares the estimate with the exact remaining cost for states
    /// along seeded random walks on the small real levels. The reference is
    /// uniform-cost search, which does not depend on the estimate.
    #[test]
    fn goal_distance_never_overestimates_on_real_levels() {
        let inputs = [
            GameInput::Left,
            GameInput::Right,
            GameInput::Jump,
            GameInput::Shoot,
            GameInput::Wait,
        ];
        let mut seed = 0x2545_f491_4f6c_dd1d_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        let root =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data/level-manual-labels");
        let mut checked = 0;
        for level in ["1", "2", "3", "6"] {
            let map = std::fs::read_to_string(root.join(format!("{level}.txt"))).unwrap();
            let initial = GameState::from_ascii(&map).unwrap();
            let estimator = Estimator::new(Heuristic::GoalDistance, &initial);
            let mut state = initial.clone();
            for _ in 0..300 {
                if state.status() != GameStatus::Playing {
                    state = initial.clone();
                }
                let exact = SolveOptions {
                    algorithm: Algorithm::AStar {
                        heuristic: Heuristic::Zero,
                        weight_percent: 100,
                    },
                    ..SolveOptions::default()
                };
                if let SolveOutcome::Solved(path) = solve(&state, exact).outcome {
                    let estimate = estimator.estimate(&state);
                    let cost = solution_cost(&state, &path);
                    assert!(
                        estimate as usize <= cost,
                        "level {level}: estimate {estimate} exceeds cost {cost}"
                    );
                    checked += 1;
                }
                state = state.step(inputs[(next() % 5) as usize]);
            }
        }
        assert!(checked > 500, "only {checked} states checked");
    }
}
