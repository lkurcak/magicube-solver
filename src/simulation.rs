//! Deterministic game simulation. Aiming pauses time; other actions advance one update.

use std::cmp::Reverse;
use std::error::Error;
use std::fmt;
use std::sync::Arc;

/// Tile coordinates, with x increasing rightward and y increasing downward.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Position {
    pub x: isize,
    pub y: isize,
}

impl Position {
    fn offset(self, dx: isize, dy: isize) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Direction {
    Left,
    Right,
}

impl Direction {
    fn dx(self) -> isize {
        match self {
            Self::Left => -1,
            Self::Right => 1,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlayerMode {
    Normal,
    Aiming,
    /// The next update advances physics but ignores the player's action.
    Recovering,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CubeSource {
    Map,
    Player,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Cube {
    pub position: Position,
    pub source: CubeSource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Projectile {
    pub position: Position,
    pub direction: Direction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameStatus {
    Playing,
    Won,
    GameOver,
}

/// Rules fixed for a game and all states derived from it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct GameSettings {
    /// Fun mode: allow firing without support, including during jump airtime.
    /// The default requires solid support immediately below the player.
    pub allow_airborne_shooting: bool,
    /// Fun mode: allow pushing cubes during jumps and falls.
    /// The default requires solid support immediately below the player.
    pub allow_airborne_pushing: bool,
    /// Maximum number of tiles a projectile traverses during one update.
    pub projectile_tiles_per_update: usize,
}

impl Default for GameSettings {
    fn default() -> Self {
        Self {
            allow_airborne_shooting: false,
            allow_airborne_pushing: false,
            projectile_tiles_per_update: 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Tile {
    Empty,
    Wall,
    Gate,
    PressurePlateBase,
    Pedestal,
    Skull,
    Torch,
    Unknown,
}

impl Tile {
    /// Whether this tile follows the pressure-plate-controlled gate rules.
    pub const fn is_gate(self) -> bool {
        matches!(self, Self::Gate)
    }

    fn symbol(self) -> char {
        match self {
            Self::Empty => ' ',
            Self::Wall => '#',
            Self::Gate => 'D',
            Self::PressurePlateBase => 'P',
            Self::Pedestal => 'G',
            Self::Skull => 'S',
            Self::Torch => 't',
            Self::Unknown => '?',
        }
    }
}

/// Static tiles shared by states derived from the same level.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Level {
    width: usize,
    height: usize,
    tiles: Vec<Tile>,
}

impl Level {
    pub fn width(&self) -> usize {
        self.width
    }

    pub fn height(&self) -> usize {
        self.height
    }

    /// Outside the drawing is empty space, not an implicit solid boundary.
    pub fn tile_at(&self, position: Position) -> Tile {
        let (Ok(x), Ok(y)) = (usize::try_from(position.x), usize::try_from(position.y)) else {
            return Tile::Empty;
        };
        if x < self.width && y < self.height {
            self.tiles[y * self.width + x]
        } else {
            Tile::Empty
        }
    }

    fn wrap_below(&self, mut position: Position) -> Position {
        if position.y >= self.height as isize {
            position.y = 0;
        }
        position
    }

    /// Whether this occupiable position is immediately above a goal pedestal.
    pub fn is_goal(&self, position: Position) -> bool {
        self.tile_at(position.offset(0, 1)) == Tile::Pedestal
    }

    /// Whether this occupiable position is immediately above a pressure-plate base.
    pub fn is_pressure_plate(&self, position: Position) -> bool {
        self.tile_at(position.offset(0, 1)) == Tile::PressurePlateBase
    }

    pub(crate) fn has_goal(&self) -> bool {
        self.tiles.contains(&Tile::Pedestal)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GameInput {
    Left,
    Right,
    Jump,
    /// Enter aiming mode when shooting is allowed, or cancel it without
    /// advancing time, unless recovering.
    Shoot,
    Wait,
}

/// Dynamic state, including the airtime needed to distinguish search states.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PlayerState {
    pub position: Position,
    /// Updates remaining before gravity resumes (0, 1, or 2).
    pub air_inputs_remaining: u8,
    pub mode: PlayerMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct GameState {
    level: Arc<Level>,
    settings: GameSettings,
    player: PlayerState,
    cubes: Vec<Cube>,
    projectile: Option<Projectile>,
    status: GameStatus,
}

impl GameState {
    /// Parses the importer's map alphabet, with exactly one `@` player.
    ///
    /// `C` creates a map cube; `O` creates the player's existing cube (at most
    /// one). Short rows are padded with empty tiles. Indentation is significant;
    /// trailing line endings are allowed. `G` goal pedestals and `P` pressure-plate
    /// bases are solid; their interactive positions are one tile above. Torches
    /// are passable for all bodies and projectiles. Skulls are passable for the
    /// player and projectiles, but block cubes. Unknown tiles stop projectiles but
    /// do not block player movement or supply ground support.
    pub fn from_ascii(map: &str) -> Result<Self, ParseLevelError> {
        Self::from_ascii_with_settings(map, GameSettings::default())
    }

    /// Parses a level with explicit rules. Ordinary play and solver entry points
    /// use [`Self::from_ascii`] for default grounded-only shooting and pushing.
    pub fn from_ascii_with_settings(
        map: &str,
        settings: GameSettings,
    ) -> Result<Self, ParseLevelError> {
        let lines: Vec<_> = map.trim_end_matches(['\n', '\r']).lines().collect();
        let width = lines
            .iter()
            .map(|line| line.chars().count())
            .max()
            .unwrap_or(0);
        if width == 0 {
            return Err(ParseLevelError::EmptyMap);
        }

        let mut level = Level {
            width,
            height: lines.len(),
            tiles: vec![Tile::Empty; width * lines.len()],
        };
        let mut player_position = None;
        let mut cubes: Vec<Cube> = Vec::new();
        for (y, line) in lines.iter().enumerate() {
            for (x, symbol) in line.chars().enumerate() {
                let position = Position {
                    x: x as isize,
                    y: y as isize,
                };
                level.tiles[y * width + x] = match symbol {
                    ' ' => Tile::Empty,
                    '#' => Tile::Wall,
                    'D' => Tile::Gate,
                    'P' => Tile::PressurePlateBase,
                    'G' => Tile::Pedestal,
                    'S' => Tile::Skull,
                    't' => Tile::Torch,
                    '?' => Tile::Unknown,
                    'C' | 'O' => {
                        let source = if symbol == 'C' {
                            CubeSource::Map
                        } else {
                            CubeSource::Player
                        };
                        if source == CubeSource::Player
                            && cubes.iter().any(|cube| cube.source == CubeSource::Player)
                        {
                            return Err(ParseLevelError::MultiplePlayerCubes);
                        }
                        cubes.push(Cube { position, source });
                        Tile::Empty
                    }
                    '@' => {
                        if player_position.replace(position).is_some() {
                            return Err(ParseLevelError::MultiplePlayers);
                        }
                        Tile::Empty
                    }
                    _ => return Err(ParseLevelError::InvalidTile { position, symbol }),
                };
            }
        }

        Ok(Self {
            level: Arc::new(level),
            settings,
            player: PlayerState {
                position: player_position.ok_or(ParseLevelError::MissingPlayer)?,
                air_inputs_remaining: 0,
                mode: PlayerMode::Normal,
            },
            cubes,
            projectile: None,
            status: GameStatus::Playing,
        })
    }

    pub fn level(&self) -> &Level {
        &self.level
    }

    pub fn settings(&self) -> GameSettings {
        self.settings
    }

    pub fn player(&self) -> PlayerState {
        self.player
    }

    pub fn cubes(&self) -> &[Cube] {
        &self.cubes
    }

    pub fn projectile(&self) -> Option<Projectile> {
        self.projectile
    }

    pub fn status(&self) -> GameStatus {
        self.status
    }

    /// Walls, feature bases, closed gates, and both kinds of cube are solid and
    /// supply ground support.
    pub fn is_solid(&self, position: Position) -> bool {
        self.is_solid_tile(position) || self.cubes.iter().any(|cube| cube.position == position)
    }

    /// A pressure plate is active while the position above its base is occupied
    /// by the player or either kind of cube.
    pub fn pressure_plates_active(&self) -> bool {
        self.pressure_plates_active_ignoring_cube(None)
    }

    fn pressure_plates_active_ignoring_cube(&self, ignored_cube: Option<usize>) -> bool {
        self.level.is_pressure_plate(self.player.position)
            || self.cubes.iter().enumerate().any(|(index, cube)| {
                Some(index) != ignored_cube && self.level.is_pressure_plate(cube.position)
            })
    }

    fn body_occupies(&self, position: Position) -> bool {
        self.player.position == position || self.cubes.iter().any(|cube| cube.position == position)
    }

    fn is_solid_tile(&self, position: Position) -> bool {
        self.is_solid_tile_ignoring_plate_cube(position, None)
    }

    fn is_solid_tile_ignoring_plate_cube(
        &self,
        position: Position,
        ignored_cube: Option<usize>,
    ) -> bool {
        let tile = self.level.tile_at(position);
        matches!(tile, Tile::Wall | Tile::PressurePlateBase | Tile::Pedestal)
            || (tile.is_gate()
                && self.pressure_plates_active_ignoring_cube(ignored_cube)
                && !self.body_occupies(position))
    }

    fn is_solid_ignoring_plate_cube(
        &self,
        position: Position,
        ignored_cube: Option<usize>,
    ) -> bool {
        self.is_solid_tile_ignoring_plate_cube(position, ignored_cube)
            || self.cubes.iter().any(|cube| cube.position == position)
    }

    fn is_solid_for_cube_ignoring_plate_cube(
        &self,
        position: Position,
        ignored_cube: Option<usize>,
    ) -> bool {
        self.is_solid_ignoring_plate_cube(position, ignored_cube)
            || self.level.tile_at(position) == Tile::Skull
    }

    pub fn is_grounded(&self) -> bool {
        self.is_grounded_ignoring_plate_cube(None)
    }

    fn is_grounded_ignoring_plate_cube(&self, ignored_cube: Option<usize>) -> bool {
        self.is_solid_ignoring_plate_cube(self.player.position.offset(0, 1), ignored_cube)
    }

    /// Whether the rules allow aiming/firing at the current position. The shot
    /// must also have a free adjacent tile in the chosen direction.
    pub fn can_shoot(&self) -> bool {
        self.settings.allow_airborne_shooting || self.is_grounded()
    }

    /// Returns the next state without changing this state.
    ///
    /// Shoot toggles aiming without advancing time. While aiming, a valid
    /// left/right input fires instead of walking; other inputs leave time paused.
    /// A blocked shot keeps aiming and preserves the previous cube/projectile.
    /// Without airborne shooting enabled, Shoot is ignored while unsupported
    /// and in Normal mode, without advancing time. Cancelling aim always works.
    /// A successful shot enters Recovering: the following input only advances
    /// physics and consumes airtime, then restores Normal mode. Use Wait for
    /// this forced update when playing or searching for solutions.
    /// Walking pushes any contiguous horizontal chain of cubes one tile if the
    /// space beyond it is not solid and the player is grounded (unless airborne
    /// pushing is enabled). A blocked push leaves the whole chain in place but
    /// still advances time, including airtime and gravity.
    ///
    /// An update resolves the action, moves the projectile by up to the configured
    /// number of tiles, then
    /// applies up to two gravity substeps to player and cubes. Within each gravity
    /// substep, lower bodies move first so stacks and falling supports stay intact.
    /// A jump rises one tile and suspends player gravity for two more air inputs;
    /// gravity resumes at the end of the second one. Cubes have no airtime.
    /// The level is won when the player's cube occupies the position immediately
    /// above a goal pedestal after the update.
    /// Won and game-over states ignore further inputs.
    pub fn step(&self, input: GameInput) -> Self {
        let mut next = self.clone();
        if next.status != GameStatus::Playing {
            return next;
        }
        let mut jumped = false;
        if next.player.mode == PlayerMode::Recovering {
            next.player.mode = PlayerMode::Normal;
        } else if input == GameInput::Shoot {
            next.player.mode = if next.player.mode == PlayerMode::Aiming {
                PlayerMode::Normal
            } else if next.can_shoot() {
                PlayerMode::Aiming
            } else {
                return next;
            };
            return next;
        } else if next.player.mode == PlayerMode::Aiming {
            let direction = match input {
                GameInput::Left => Direction::Left,
                GameInput::Right => Direction::Right,
                _ => return next,
            };
            if !next.try_shoot(direction) {
                return next;
            }
            next.player.mode = PlayerMode::Recovering;
        } else {
            match input {
                GameInput::Left => {
                    next.try_walk(Direction::Left);
                }
                GameInput::Right => {
                    next.try_walk(Direction::Right);
                }
                GameInput::Jump if next.is_grounded() && next.try_move(0, -1) => {
                    next.player.air_inputs_remaining = 2;
                    jumped = true;
                }
                GameInput::Jump | GameInput::Shoot | GameInput::Wait => {}
            }
        }

        let swept_cube_positions = next.gravity_swept_cube_positions();
        let spawned_cube = next.advance_projectile(&swept_cube_positions);
        // A cube created by this projectile becomes a plate activator only after
        // the gravity response to removing the previous player cube has finished.
        if !jumped {
            if next.is_grounded_ignoring_plate_cube(spawned_cube) {
                next.player.air_inputs_remaining = 0;
            } else {
                next.player.air_inputs_remaining =
                    next.player.air_inputs_remaining.saturating_sub(1);
            }
        }
        next.apply_gravity_ignoring_plate_cube(
            !jumped && next.player.air_inputs_remaining == 0,
            spawned_cube,
        );
        if next.is_grounded() {
            next.player.air_inputs_remaining = 0;
        }
        if next.status == GameStatus::Playing
            && next
                .cubes
                .iter()
                .any(|cube| cube.source == CubeSource::Player && next.level.is_goal(cube.position))
        {
            next.status = GameStatus::Won;
        }
        next
    }

    fn blocks_projectile(&self, position: Position) -> bool {
        !matches!(
            self.level.tile_at(position),
            Tile::Empty | Tile::Gate | Tile::Skull | Tile::Torch
        ) || self.is_solid(position)
            || position == self.player.position
    }

    fn try_shoot(&mut self, direction: Direction) -> bool {
        if !self.can_shoot() {
            return false;
        }
        let adjacent = self.player.position.offset(direction.dx(), 0);
        // Validate before removing anything: a blocked shot preserves the world.
        if self.blocks_projectile(adjacent) {
            return false;
        }
        self.cubes.retain(|cube| cube.source == CubeSource::Map);
        self.projectile = Some(Projectile {
            position: self.player.position,
            direction,
        });
        true
    }

    fn advance_projectile(&mut self, swept_cube_positions: &[Position]) -> Option<usize> {
        let mut projectile = self.projectile.take()?;
        for _ in 0..self.settings.projectile_tiles_per_update {
            let target = projectile.position.offset(projectile.direction.dx(), 0);
            if self.blocks_projectile(target) || swept_cube_positions.contains(&target) {
                if self.level.tile_at(projectile.position) != Tile::Skull {
                    let spawned_cube = self.cubes.len();
                    self.cubes.push(Cube {
                        position: projectile.position,
                        source: CubeSource::Player,
                    });
                    if projectile.position == self.player.position {
                        self.status = GameStatus::GameOver;
                    }
                    return Some(spawned_cube);
                }
                return None;
            }
            projectile.position = target;
        }
        self.projectile = Some(projectile);
        None
    }

    /// Tiles occupied by cubes at any point during this update's gravity pass.
    /// This preview does not mutate the real state. It lets projectile collision
    /// treat a falling cube as occupying its complete vertical sweep while all
    /// other physics continues to use the cube's canonical position.
    fn gravity_swept_cube_positions(&self) -> Vec<Position> {
        if self.projectile.is_none() || self.cubes.is_empty() {
            return Vec::new();
        }
        let mut preview = self.clone();
        let mut swept: Vec<_> = preview.cubes.iter().map(|cube| cube.position).collect();
        preview.apply_gravity_recording(false, Some(&mut swept), None);
        swept
    }

    fn apply_gravity_ignoring_plate_cube(
        &mut self,
        player_falls: bool,
        ignored_cube: Option<usize>,
    ) {
        self.apply_gravity_recording(player_falls, None, ignored_cube);
    }

    fn apply_gravity_recording(
        &mut self,
        player_falls: bool,
        mut swept_cube_positions: Option<&mut Vec<Position>>,
        ignored_plate_cube: Option<usize>,
    ) {
        for _ in 0..2 {
            // None identifies the player; Some(index) identifies a cube.
            let mut bodies: Vec<_> = self
                .cubes
                .iter()
                .enumerate()
                .map(|(index, cube)| (cube.position.y, Some(index)))
                .collect();
            bodies.push((self.player.position.y, None));
            bodies.sort_by_key(|(y, _)| Reverse(*y));
            for (_, body) in bodies {
                if self.status == GameStatus::GameOver {
                    return;
                }
                match body {
                    None if player_falls => {
                        self.try_move_ignoring_plate_cube(0, 1, ignored_plate_cube);
                    }
                    Some(index) => {
                        let target = self
                            .level
                            .wrap_below(self.cubes[index].position.offset(0, 1));
                        if !self.is_solid_for_cube_ignoring_plate_cube(target, ignored_plate_cube) {
                            self.cubes[index].position = target;
                            if let Some(swept) = swept_cube_positions.as_deref_mut() {
                                swept.push(target);
                            }
                            if target == self.player.position {
                                self.status = GameStatus::GameOver;
                            }
                        }
                    }
                    None => {}
                }
            }
        }
    }

    /// Renders the original level rectangle with the player overlaid.
    ///
    /// An out-of-bounds player is omitted; its position remains available through
    /// `player()`. Dynamic cubes and the projectile are included.
    pub fn to_ascii(&self) -> String {
        (0..self.level.height)
            .map(|y| {
                (0..self.level.width)
                    .map(|x| {
                        let position = Position {
                            x: x as isize,
                            y: y as isize,
                        };
                        self.symbol_at(position)
                    })
                    .collect::<String>()
                    .trim_end()
                    .to_owned()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// The same one-character representation used in level files and the UI.
    pub fn symbol_at(&self, position: Position) -> char {
        if self.player.position == position {
            '@'
        } else if let Some(cube) = self.cubes.iter().find(|cube| cube.position == position) {
            match cube.source {
                CubeSource::Map => 'C',
                CubeSource::Player => 'O',
            }
        } else if let Some(projectile) = self.projectile.filter(|p| p.position == position) {
            match projectile.direction {
                Direction::Left => '<',
                Direction::Right => '>',
            }
        } else {
            self.level.tile_at(position).symbol()
        }
    }

    fn try_walk(&mut self, direction: Direction) -> bool {
        let dx = direction.dx();
        let destination = self.player.position.offset(dx, 0);
        let mut target = destination;
        let mut chain = Vec::new();
        // Check the entire chain before moving anything. Only walking pushes;
        // jumping and gravity still use ordinary solid-tile collision checks.
        loop {
            if self.is_solid_tile(target) {
                return false;
            }
            match self.cubes.iter().position(|cube| cube.position == target) {
                Some(index) => {
                    if !self.settings.allow_airborne_pushing && !self.is_grounded() {
                        return false;
                    }
                    chain.push(index);
                    target = target.offset(dx, 0);
                }
                None => {
                    if !chain.is_empty() && self.level.tile_at(target) == Tile::Skull {
                        return false;
                    }
                    break;
                }
            }
        }
        for index in chain {
            self.cubes[index].position = self.cubes[index].position.offset(dx, 0);
        }
        self.player.position = destination;
        true
    }

    fn try_move(&mut self, dx: isize, dy: isize) -> bool {
        self.try_move_ignoring_plate_cube(dx, dy, None)
    }

    fn try_move_ignoring_plate_cube(
        &mut self,
        dx: isize,
        dy: isize,
        ignored_cube: Option<usize>,
    ) -> bool {
        let position = self.level.wrap_below(self.player.position.offset(dx, dy));
        if self.is_solid_ignoring_plate_cube(position, ignored_cube) {
            return false;
        }
        self.player.position = position;
        true
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseLevelError {
    EmptyMap,
    MissingPlayer,
    MultiplePlayers,
    MultiplePlayerCubes,
    InvalidTile { position: Position, symbol: char },
}

impl fmt::Display for ParseLevelError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::EmptyMap => write!(f, "level map is empty"),
            Self::MissingPlayer => write!(f, "level map needs one '@' player"),
            Self::MultiplePlayers => write!(f, "level map contains multiple '@' players"),
            Self::MultiplePlayerCubes => {
                write!(f, "level map contains multiple player cubes ('O')")
            }
            Self::InvalidTile { position, symbol } => {
                write!(
                    f,
                    "invalid tile {symbol:?} at ({}, {})",
                    position.x, position.y
                )
            }
        }
    }
}

impl Error for ParseLevelError {}
