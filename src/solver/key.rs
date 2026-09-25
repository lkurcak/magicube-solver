//! Compact, allocation-free storage for the states discovered by one search.
//!
//! Each state is encoded as a fixed-width row of `u16` words in a single
//! arena, and the visited set stores only row indices. Everything static for
//! the search (the level and the rules) lives in one template state instead.

use std::hash::BuildHasher;

use hashbrown::HashTable;
use rustc_hash::FxBuildHasher;

use super::Move;
use crate::{
    Cube, CubeSource, Direction, GameInput, GameState, GameStatus, PlayerMode, PlayerState,
    Position, Projectile,
};

const SEPARATOR: u16 = u16::MAX;
/// Cube words pack a tile index above a three-bit source.
const MAX_TILES: usize = 1 << 13;
const HEADER: usize = 3;

/// Encodes the dynamic part of a state: player, cube list in order (it breaks
/// gravity ties), inactive red cubes, projectile and status. Settings are
/// omitted because every state of a search shares them.
///
/// Row layout: player tile, packed flags, projectile tile, cube words,
/// separator, inactive red tiles, separator padding.
struct Codec {
    template: GameState,
    width: usize,
    row: usize,
}

impl Codec {
    fn new(initial: &GameState) -> Self {
        let level = initial.level();
        let (width, height) = (level.width(), level.height());
        assert!(
            width * height < MAX_TILES,
            "solver keys support maps of fewer than {MAX_TILES} tiles, got {width}x{height}"
        );
        // Only the player's cube is ever added or removed, and red cubes
        // move between the two lists, so this bounds every reachable state.
        let has_player_cube = initial
            .cubes()
            .iter()
            .any(|cube| cube.source == CubeSource::Player);
        let bodies = initial.cubes().len()
            + initial.inactive_red_cubes().len()
            + usize::from(!has_player_cube);
        Self {
            template: initial.clone(),
            width,
            row: HEADER + bodies + 1,
        }
    }

    fn tile(&self, position: Position) -> u16 {
        (position.y as usize * self.width + position.x as usize) as u16
    }

    fn position(&self, tile: u16) -> Position {
        let tile = usize::from(tile);
        Position {
            x: (tile % self.width) as isize,
            y: (tile / self.width) as isize,
        }
    }

    fn encode(&self, state: &GameState, out: &mut Vec<u16>) {
        let start = out.len();
        let player = state.player();
        let projectile = state.projectile();
        debug_assert!(player.air_inputs_remaining < 4);
        let flags = u16::from(player.air_inputs_remaining)
            | (player_mode_bits(player.mode) << 2)
            | (status_bits(state.status()) << 4)
            | (u16::from(projectile.is_some()) << 6)
            | (u16::from(projectile.is_some_and(|p| p.direction == Direction::Right)) << 7);
        out.push(self.tile(player.position));
        out.push(flags);
        out.push(projectile.map_or(0, |p| self.tile(p.position)));
        out.extend(
            state
                .cubes()
                .iter()
                .map(|cube| (self.tile(cube.position) << 3) | source_bits(cube.source)),
        );
        out.push(SEPARATOR);
        out.extend(
            state
                .inactive_red_cubes()
                .iter()
                .map(|&position| self.tile(position)),
        );
        assert!(
            out.len() - start <= self.row,
            "a state has more cubes than the initial state allows"
        );
        out.resize(start + self.row, SEPARATOR);
    }

    fn decode(&self, row: &[u16]) -> GameState {
        let flags = row[1];
        let player = PlayerState {
            position: self.position(row[0]),
            air_inputs_remaining: (flags & 0b11) as u8,
            mode: player_mode((flags >> 2) & 0b11),
        };
        let projectile = (flags & (1 << 6) != 0).then(|| Projectile {
            position: self.position(row[2]),
            direction: if flags & (1 << 7) != 0 {
                Direction::Right
            } else {
                Direction::Left
            },
        });
        let mut words = row[HEADER..].split(|&word| word == SEPARATOR);
        let cubes = words
            .next()
            .unwrap_or_default()
            .iter()
            .map(|&word| Cube {
                position: self.position(word >> 3),
                source: source(word & 0b111),
            })
            .collect();
        let inactive_red_cubes = words
            .next()
            .unwrap_or_default()
            .iter()
            .map(|&tile| self.position(tile))
            .collect();
        self.template.with_dynamic(
            player,
            cubes,
            inactive_red_cubes,
            projectile,
            status((flags >> 4) & 0b11),
        )
    }
}

/// The distinct states of one search, with the move that last improved each.
pub(super) struct StateStore {
    codec: Codec,
    arena: Vec<u16>,
    table: HashTable<u32>,
    hasher: FxBuildHasher,
    parents: Vec<u32>,
    moves: Vec<Move>,
    /// The most recently looked-up state, ready to be inserted.
    pending: Vec<u16>,
    pending_hash: u64,
}

const ROOT: u32 = u32::MAX;

impl StateStore {
    /// Creates a store holding only `initial`, as node 0.
    pub fn new(initial: &GameState) -> Self {
        let mut store = Self {
            codec: Codec::new(initial),
            arena: Vec::new(),
            table: HashTable::new(),
            hasher: FxBuildHasher,
            parents: Vec::new(),
            moves: Vec::new(),
            pending: Vec::new(),
            pending_hash: 0,
        };
        let found = store.find(initial);
        debug_assert!(found.is_none());
        store.insert_pending(ROOT, Move::Input(GameInput::Wait));
        store
    }

    pub fn len(&self) -> usize {
        self.parents.len()
    }

    fn row(&self, node: u32) -> &[u16] {
        let start = node as usize * self.codec.row;
        &self.arena[start..start + self.codec.row]
    }

    pub fn state(&self, node: u32) -> GameState {
        self.codec.decode(self.row(node))
    }

    /// Returns the node already holding `state`, if any. Otherwise the
    /// state is kept for a following [`Self::insert_pending`].
    pub fn find(&mut self, state: &GameState) -> Option<u32> {
        self.pending.clear();
        self.codec.encode(state, &mut self.pending);
        self.pending_hash = self.hasher.hash_one(&self.pending);
        let (arena, row) = (&self.arena, self.codec.row);
        self.table
            .find(self.pending_hash, |&node| {
                let start = node as usize * row;
                arena[start..start + row] == self.pending[..]
            })
            .copied()
    }

    /// Stores the state from the last unsuccessful [`Self::find`].
    pub fn insert_pending(&mut self, parent: u32, action: Move) -> u32 {
        let node = u32::try_from(self.len()).expect("more than u32::MAX search states");
        self.arena.extend_from_slice(&self.pending);
        self.parents.push(parent);
        self.moves.push(action);
        let (arena, row, hasher) = (&self.arena, self.codec.row, &self.hasher);
        self.table.insert_unique(self.pending_hash, node, |&node| {
            let start = node as usize * row;
            hasher.hash_one(&arena[start..start + row])
        });
        node
    }

    /// Records a shorter path to `node` (A* reopening).
    pub fn set_parent(&mut self, node: u32, parent: u32, action: Move) {
        self.parents[node as usize] = parent;
        self.moves[node as usize] = action;
    }

    /// The inputs leading from the initial state to `node`.
    pub fn path(&self, mut node: u32) -> Vec<GameInput> {
        let mut moves = Vec::new();
        while self.parents[node as usize] != ROOT {
            moves.push(self.moves[node as usize]);
            node = self.parents[node as usize];
        }
        moves
            .iter()
            .rev()
            .flat_map(|action| action.inputs())
            .copied()
            .collect()
    }
}

fn player_mode_bits(mode: PlayerMode) -> u16 {
    match mode {
        PlayerMode::Normal => 0,
        PlayerMode::Aiming => 1,
        PlayerMode::Recovering => 2,
    }
}

fn player_mode(bits: u16) -> PlayerMode {
    match bits {
        0 => PlayerMode::Normal,
        1 => PlayerMode::Aiming,
        _ => PlayerMode::Recovering,
    }
}

fn status_bits(status: GameStatus) -> u16 {
    match status {
        GameStatus::Playing => 0,
        GameStatus::Won => 1,
        GameStatus::GameOver => 2,
    }
}

fn status(bits: u16) -> GameStatus {
    match bits {
        0 => GameStatus::Playing,
        1 => GameStatus::Won,
        _ => GameStatus::GameOver,
    }
}

fn source_bits(source: CubeSource) -> u16 {
    match source {
        CubeSource::Map => 0,
        CubeSource::Player => 1,
        CubeSource::Glass => 2,
        CubeSource::Red => 3,
        CubeSource::Blue => 4,
    }
}

fn source(bits: u16) -> CubeSource {
    match bits {
        0 => CubeSource::Map,
        1 => CubeSource::Player,
        2 => CubeSource::Glass,
        3 => CubeSource::Red,
        _ => CubeSource::Blue,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const INPUTS: [GameInput; 5] = [
        GameInput::Left,
        GameInput::Right,
        GameInput::Jump,
        GameInput::Shoot,
        GameInput::Wait,
    ];

    fn maps() -> Vec<String> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("data");
        let mut maps = Vec::new();
        for directory in ["level-manual-labels", "custom-levels"] {
            for entry in std::fs::read_dir(root.join(directory)).unwrap() {
                maps.push(std::fs::read_to_string(entry.unwrap().path()).unwrap());
            }
        }
        maps
    }

    /// Seeded random walks cover moving cubes, projectiles, aiming,
    /// recovery, red cubes and terminal states on the real levels.
    #[test]
    fn keys_round_trip_and_identify_exactly_the_same_dynamic_state() {
        let mut seed = 0x9e37_79b9_7f4a_7c15_u64;
        let mut next = || {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed
        };
        for map in maps() {
            let initial = GameState::from_ascii(&map).unwrap();
            let codec = Codec::new(&initial);
            let mut seen: Vec<(Vec<u16>, GameState)> = Vec::new();
            for _ in 0..20 {
                let mut state = initial.clone();
                for _ in 0..200 {
                    let mut row = Vec::new();
                    codec.encode(&state, &mut row);
                    assert_eq!(row.len(), codec.row);
                    assert_eq!(codec.decode(&row), state);
                    seen.push((row, state.clone()));
                    if state.status() != GameStatus::Playing {
                        break;
                    }
                    state = state.step(INPUTS[(next() % 5) as usize]);
                }
            }
            for (row_a, a) in seen.iter().step_by(7) {
                for (row_b, b) in seen.iter().step_by(5) {
                    assert_eq!(row_a == row_b, a == b);
                }
            }
        }
    }

    #[test]
    fn store_deduplicates_and_reconstructs_paths() {
        let initial = GameState::from_ascii("#####\n#   #\n# @ #\n#####").unwrap();
        let mut store = StateStore::new(&initial);
        let aiming = initial.step(GameInput::Shoot);
        assert_eq!(store.find(&initial), Some(0));
        assert_eq!(store.find(&aiming), None);
        let node = store.insert_pending(0, Move::Input(GameInput::Shoot));
        assert_eq!(store.find(&aiming), Some(node));
        assert_eq!(store.state(node), aiming);
        assert_eq!(store.path(node), [GameInput::Shoot]);
        assert_eq!(store.path(0), []);
    }
}
