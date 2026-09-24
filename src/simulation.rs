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

/// The direction a laser emitter fires its beam.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaserDirection {
    Left,
    Up,
    Right,
    Down,
}

impl LaserDirection {
    fn step(self, position: Position) -> Position {
        match self {
            Self::Left => position.offset(-1, 0),
            Self::Up => position.offset(0, -1),
            Self::Right => position.offset(1, 0),
            Self::Down => position.offset(0, 1),
        }
    }

    /// Whether `target` lies on the ray leaving `origin` in this direction.
    fn points_at(self, origin: Position, target: Position) -> bool {
        match self {
            Self::Left => target.y == origin.y && target.x < origin.x,
            Self::Up => target.x == origin.x && target.y < origin.y,
            Self::Right => target.y == origin.y && target.x > origin.x,
            Self::Down => target.x == origin.x && target.y > origin.y,
        }
    }

    fn is_horizontal(self) -> bool {
        matches!(self, Self::Left | Self::Right)
    }
}

/// Orientation of the laser beams crossing a tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaserBeam {
    Horizontal,
    Vertical,
    Crossing,
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
    /// A map cube that projectiles pass through.
    Glass,
    /// A materialized red cube, present while a red pressure plate is pressed.
    Red,
    /// A cube that never falls and cannot be pushed. It copies every
    /// single-tile move of the player's cube, if its own target tile is free.
    Blue,
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
    /// The default requires stable ground support below the player.
    pub allow_airborne_shooting: bool,
    /// Fun mode: allow pushing stable cubes during player jumps and falls.
    /// The default requires stable ground support below the player.
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
    /// A pressure-plate base that materializes red cubes instead of closing gates.
    RedPressurePlateBase,
    Pedestal,
    Skull,
    Torch,
    /// A solid laser source firing in the given direction.
    LaserEmitter(LaserDirection),
    /// A solid switch that acts like a pressed pressure plate while a laser hits it.
    LaserTrigger,
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
            Self::RedPressurePlateBase => 'R',
            Self::Pedestal => 'G',
            Self::Skull => 'S',
            Self::Torch => 't',
            Self::LaserEmitter(LaserDirection::Left) => '{',
            Self::LaserEmitter(LaserDirection::Up) => '^',
            Self::LaserEmitter(LaserDirection::Right) => '}',
            Self::LaserEmitter(LaserDirection::Down) => 'v',
            Self::LaserTrigger => 'T',
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
    /// Every laser emitter, derived from `tiles`.
    emitters: Vec<(Position, LaserDirection)>,
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

    /// Maps a position that left the map through any edge back onto the
    /// opposite edge.
    fn wrap(&self, mut position: Position) -> Position {
        position.x = position.x.rem_euclid(self.width as isize);
        position.y = position.y.rem_euclid(self.height as isize);
        position
    }

    /// Whether this occupiable position is immediately above a goal pedestal.
    pub fn is_goal(&self, position: Position) -> bool {
        self.tile_at(self.wrap(position.offset(0, 1))) == Tile::Pedestal
    }

    /// Whether this occupiable position is immediately above a pressure-plate base.
    pub fn is_pressure_plate(&self, position: Position) -> bool {
        self.tile_at(self.wrap(position.offset(0, 1))) == Tile::PressurePlateBase
    }

    /// Whether this occupiable position is immediately above a red pressure-plate base.
    pub fn is_red_pressure_plate(&self, position: Position) -> bool {
        self.tile_at(self.wrap(position.offset(0, 1))) == Tile::RedPressurePlateBase
    }

    pub(crate) fn has_goal(&self) -> bool {
        self.tiles.contains(&Tile::Pedestal)
    }

    fn contains(&self, position: Position) -> bool {
        (0..self.width as isize).contains(&position.x)
            && (0..self.height as isize).contains(&position.y)
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
    /// Dematerialized red cubes. They take no part in physics, and become
    /// [`CubeSource::Red`] entries of `cubes` once they can materialize.
    inactive_red_cubes: Vec<Position>,
    projectile: Option<Projectile>,
    status: GameStatus,
}

impl GameState {
    /// Parses the importer's map alphabet, with exactly one `@` player.
    ///
    /// `C` creates a map cube; `g` creates a glass map cube that projectiles pass
    /// through; `O` creates the player's existing cube (at most one). Short rows are padded with empty tiles. Indentation is significant;
    /// trailing line endings are allowed. `G` goal pedestals and `P` pressure-plate
    /// bases are solid; their interactive positions are one tile above. Torches
    /// are passable for all bodies and projectiles. Skulls are passable for the
    /// player and projectiles, but block cubes. Unknown tiles stop projectiles but
    /// do not block player movement or supply ground support. `{`, `^`, `}`, and
    /// `v` are solid laser emitters firing left, up, right, and down; `T` is a
    /// solid laser trigger. `r` is a red cube and `R` a solid red pressure-plate
    /// base; red cubes start materialized if a red plate is already pressed.
    /// `b` is a blue cube, which follows the player's cube instead of falling.
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
            emitters: Vec::new(),
        };
        let mut player_position = None;
        let mut cubes: Vec<Cube> = Vec::new();
        let mut inactive_red_cubes = Vec::new();
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
                    'R' => Tile::RedPressurePlateBase,
                    'G' => Tile::Pedestal,
                    'S' => Tile::Skull,
                    't' => Tile::Torch,
                    '{' => Tile::LaserEmitter(LaserDirection::Left),
                    '^' => Tile::LaserEmitter(LaserDirection::Up),
                    '}' => Tile::LaserEmitter(LaserDirection::Right),
                    'v' => Tile::LaserEmitter(LaserDirection::Down),
                    'T' => Tile::LaserTrigger,
                    '?' => Tile::Unknown,
                    'C' | 'g' | 'b' | 'O' => {
                        let source = match symbol {
                            'C' => CubeSource::Map,
                            'g' => CubeSource::Glass,
                            'b' => CubeSource::Blue,
                            _ => CubeSource::Player,
                        };
                        if source == CubeSource::Player
                            && cubes.iter().any(|cube| cube.source == CubeSource::Player)
                        {
                            return Err(ParseLevelError::MultiplePlayerCubes);
                        }
                        cubes.push(Cube { position, source });
                        Tile::Empty
                    }
                    'r' => {
                        inactive_red_cubes.push(position);
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
                if let Tile::LaserEmitter(direction) = level.tiles[y * width + x] {
                    level.emitters.push((position, direction));
                }
            }
        }

        let mut state = Self {
            level: Arc::new(level),
            settings,
            player: PlayerState {
                position: player_position.ok_or(ParseLevelError::MissingPlayer)?,
                air_inputs_remaining: 0,
                mode: PlayerMode::Normal,
            },
            cubes,
            inactive_red_cubes,
            projectile: None,
            status: GameStatus::Playing,
        };
        state.update_red_cubes(None);
        Ok(state)
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

    /// Positions of red cubes that are currently dematerialized.
    pub fn inactive_red_cubes(&self) -> &[Position] {
        &self.inactive_red_cubes
    }

    pub fn projectile(&self) -> Option<Projectile> {
        self.projectile
    }

    pub fn status(&self) -> GameStatus {
        self.status
    }

    /// Walls, feature bases, closed gates, and both kinds of cube are solid.
    /// A cube supplies ground support only when it is itself stably supported.
    pub fn is_solid(&self, position: Position) -> bool {
        self.is_solid_tile(position) || self.cubes.iter().any(|cube| cube.position == position)
    }

    /// A pressure plate is active while the position above its base is occupied
    /// by the player or any kind of cube. A laser trigger hit by a beam counts
    /// as an active pressure plate.
    pub fn pressure_plates_active(&self) -> bool {
        self.pressure_plates_active_ignoring_cube(None)
    }

    fn pressure_plates_active_ignoring_cube(&self, ignored_cube: Option<usize>) -> bool {
        self.pressure_plates_pressed_ignoring_cube(ignored_cube) || self.laser_triggers_lit()
    }

    /// Plate activity from occupants only, ignoring laser triggers.
    fn pressure_plates_pressed_ignoring_cube(&self, ignored_cube: Option<usize>) -> bool {
        self.level.is_pressure_plate(self.player.position)
            || self.cubes.iter().enumerate().any(|(index, cube)| {
                Some(index) != ignored_cube && self.level.is_pressure_plate(cube.position)
            })
    }

    /// A red pressure plate is pressed while the position above its base is
    /// occupied by the player or any materialized cube. Red plates only control
    /// red cubes; they do not close gates, and laser triggers do not press them.
    pub fn red_pressure_plates_active(&self) -> bool {
        self.red_pressure_plates_pressed_ignoring_cube(None)
    }

    fn red_pressure_plates_pressed_ignoring_cube(&self, ignored_cube: Option<usize>) -> bool {
        self.level.is_red_pressure_plate(self.player.position)
            || self.cubes.iter().enumerate().any(|(index, cube)| {
                Some(index) != ignored_cube && self.level.is_red_pressure_plate(cube.position)
            })
    }

    /// Releasing every red plate dematerializes all red cubes where they are.
    /// While a plate is pressed, each inactive red cube materializes as soon as
    /// its tile is free of bodies, the projectile, and solid terrain. Returns
    /// the index of `ignored_cube` after the cube list changes.
    fn update_red_cubes(&mut self, ignored_cube: Option<usize>) -> Option<usize> {
        if !self.red_pressure_plates_pressed_ignoring_cube(ignored_cube) {
            let ignored = ignored_cube.map(|index| self.cubes[index]);
            let (red, others): (Vec<_>, Vec<_>) = std::mem::take(&mut self.cubes)
                .into_iter()
                .partition(|cube| cube.source == CubeSource::Red);
            self.inactive_red_cubes
                .extend(red.into_iter().map(|cube| cube.position));
            self.cubes = others;
            return ignored.and_then(|ignored| self.cubes.iter().position(|&cube| cube == ignored));
        }
        let mut index = 0;
        while index < self.inactive_red_cubes.len() {
            let position = self.inactive_red_cubes[index];
            if self.body_occupies(position)
                || self.is_solid_tile(position)
                || self
                    .projectile
                    .is_some_and(|projectile| projectile.position == position)
            {
                index += 1;
            } else {
                self.inactive_red_cubes.remove(index);
                self.cubes.push(Cube {
                    position,
                    source: CubeSource::Red,
                });
            }
        }
        ignored_cube
    }

    /// Whether a laser beam stops at `position`. Beams pass through empty
    /// space, torches, skulls, glass cubes, and the player (who dies instead).
    /// Gates stop beams only while closed by occupied pressure plates; a gate
    /// closed by a laser trigger never cuts off a beam, which keeps the
    /// trigger's own state free of feedback loops.
    fn blocks_laser(&self, position: Position) -> bool {
        let tile_blocks = match self.level.tile_at(position) {
            Tile::Empty | Tile::Skull | Tile::Torch => false,
            Tile::Gate => {
                self.pressure_plates_pressed_ignoring_cube(None) && !self.body_occupies(position)
            }
            _ => true,
        };
        tile_blocks
            || self
                .cubes
                .iter()
                .any(|cube| cube.position == position && cube.source != CubeSource::Glass)
    }

    /// Whether any emitter's beam ends on a laser trigger.
    fn laser_triggers_lit(&self) -> bool {
        self.level.emitters.iter().any(|&(emitter, direction)| {
            self.level.tile_at(self.laser_beam_end(emitter, direction)) == Tile::LaserTrigger
        })
    }

    /// The first blocking position along a beam, or the first position outside
    /// the map.
    fn laser_beam_end(&self, emitter: Position, direction: LaserDirection) -> Position {
        let mut position = direction.step(emitter);
        while self.level.contains(position) && !self.blocks_laser(position) {
            position = direction.step(position);
        }
        position
    }

    /// Whether a laser trigger at `position` is currently hit by a beam.
    pub fn laser_trigger_lit(&self, position: Position) -> bool {
        self.level.tile_at(position) == Tile::LaserTrigger
            && self.level.emitters.iter().any(|&(emitter, direction)| {
                direction.points_at(emitter, position)
                    && self.laser_beam_end(emitter, direction) == position
            })
    }

    /// Laser beams crossing `position`. Beams continue indefinitely beyond
    /// the map's edges until a body blocks them.
    pub fn laser_beam_at(&self, position: Position) -> Option<LaserBeam> {
        if self.blocks_laser(position) {
            return None;
        }
        let (mut horizontal, mut vertical) = (false, false);
        for &(emitter, direction) in &self.level.emitters {
            if !direction.points_at(emitter, position)
                || (direction.is_horizontal() && horizontal)
                || (!direction.is_horizontal() && vertical)
            {
                continue;
            }
            let mut current = direction.step(emitter);
            while current != position && !self.blocks_laser(current) {
                current = direction.step(current);
            }
            if current == position {
                if direction.is_horizontal() {
                    horizontal = true;
                } else {
                    vertical = true;
                }
            }
        }
        match (horizontal, vertical) {
            (true, true) => Some(LaserBeam::Crossing),
            (true, false) => Some(LaserBeam::Horizontal),
            (false, true) => Some(LaserBeam::Vertical),
            (false, false) => None,
        }
    }

    /// A player touching a laser beam dies, including while moving through it.
    fn apply_laser_damage(&mut self) {
        if self.status == GameStatus::Playing && self.laser_beam_at(self.player.position).is_some()
        {
            self.status = GameStatus::GameOver;
        }
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
        matches!(
            tile,
            Tile::Wall
                | Tile::PressurePlateBase
                | Tile::RedPressurePlateBase
                | Tile::Pedestal
                | Tile::LaserEmitter(_)
                | Tile::LaserTrigger
        ) || (tile.is_gate()
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

    pub fn is_grounded(&self) -> bool {
        self.is_grounded_ignoring_plate_cube(None)
    }

    fn is_grounded_ignoring_plate_cube(&self, ignored_cube: Option<usize>) -> bool {
        let support = self.level.wrap(self.player.position.offset(0, 1));
        if self.is_solid_tile_ignoring_plate_cube(support, ignored_cube) {
            return true;
        }
        self.cubes
            .iter()
            .position(|cube| cube.position == support)
            .is_some_and(|index| self.cube_is_grounded_ignoring_plate_cube(index, ignored_cube))
    }

    fn cube_is_grounded_ignoring_plate_cube(
        &self,
        cube: usize,
        ignored_cube: Option<usize>,
    ) -> bool {
        if self.cubes[cube].source == CubeSource::Blue {
            return true;
        }
        let mut support = self.level.wrap(self.cubes[cube].position.offset(0, 1));
        // Cubes remain solid collision bodies while falling, but they provide
        // ground support only when the whole vertical stack is stable. Follow
        // the stack until it reaches terrain or a blue cube that blocks the
        // bottom cube.
        // The bounded loop also handles a wrapped column of mutually supporting
        // cubes, which cannot move under the same gravity collision rules.
        for _ in 0..=self.cubes.len() {
            if self.is_solid_tile_ignoring_plate_cube(support, ignored_cube)
                || self.level.tile_at(support) == Tile::Skull
            {
                return true;
            }
            let Some(cube) = self.cubes.iter().find(|cube| cube.position == support) else {
                return false;
            };
            if cube.source == CubeSource::Blue {
                return true;
            }
            support = self.level.wrap(cube.position.offset(0, 1));
        }
        true
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
    /// pushing is enabled). Blue cubes cannot be pushed; they copy each
    /// single-tile push or fall of the player's cube instead. A chain that
    /// reaches a blue cube after the player's cube carries it along. A blocked push leaves the whole chain in place but
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
        // Walking, jumping, pushing, or removing the old player cube can press
        // or release red plates, and expose the player to a beam.
        next.update_red_cubes(None);
        next.apply_laser_damage();

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
        // The spawned cube presses red plates from here on.
        next.update_red_cubes(None);
        next.apply_laser_damage();
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

    /// Glass cubes are solid bodies but let projectiles pass through.
    fn blocks_projectile(&self, position: Position) -> bool {
        !matches!(
            self.level.tile_at(position),
            Tile::Empty | Tile::Gate | Tile::Skull | Tile::Torch
        ) || self.is_solid_tile(position)
            || self
                .cubes
                .iter()
                .any(|cube| cube.position == position && cube.source != CubeSource::Glass)
            || position == self.player.position
    }

    fn try_shoot(&mut self, direction: Direction) -> bool {
        if !self.can_shoot() {
            return false;
        }
        let adjacent = self
            .level
            .wrap(self.player.position.offset(direction.dx(), 0));
        // Validate before removing anything: a blocked shot preserves the world.
        // Any solid body next to the player blocks the shot, including glass
        // cubes that projectiles otherwise pass through.
        if self.blocks_projectile(adjacent) || self.is_solid(adjacent) {
            return false;
        }
        self.cubes.retain(|cube| cube.source != CubeSource::Player);
        self.projectile = Some(Projectile {
            position: self.player.position,
            direction,
        });
        true
    }

    fn advance_projectile(&mut self, swept_cube_positions: &[Position]) -> Option<usize> {
        let mut projectile = self.projectile.take()?;
        for _ in 0..self.settings.projectile_tiles_per_update {
            let target = self
                .level
                .wrap(projectile.position.offset(projectile.direction.dx(), 0));
            if self.blocks_projectile(target) || swept_cube_positions.contains(&target) {
                // The cube cannot spawn inside a skull or a glass cube.
                if self.level.tile_at(projectile.position) != Tile::Skull
                    && !self
                        .cubes
                        .iter()
                        .any(|cube| cube.position == projectile.position)
                {
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

    /// Tiles occupied by opaque cubes at any point during this update's gravity pass.
    /// This preview does not mutate the real state. It lets projectile collision
    /// treat a falling cube as occupying its complete vertical sweep while all
    /// other physics continues to use the cube's canonical position.
    fn gravity_swept_cube_positions(&self) -> Vec<Position> {
        if self.projectile.is_none() || self.cubes.is_empty() {
            return Vec::new();
        }
        let mut preview = self.clone();
        let mut swept: Vec<_> = preview
            .cubes
            .iter()
            .filter(|cube| cube.source != CubeSource::Glass)
            .map(|cube| cube.position)
            .collect();
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
        mut ignored_plate_cube: Option<usize>,
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
            // Bodies in a substep fall together, so a gate occupied as the
            // substep begins stays open until it ends. Without this, a stack
            // would lose its gate between the lower body leaving and the upper
            // body entering it.
            let held_open_gates: Vec<_> = bodies
                .iter()
                .map(|&(_, body)| match body {
                    Some(index) => self.cubes[index].position,
                    None => self.player.position,
                })
                .filter(|&position| self.level.tile_at(position).is_gate())
                .collect();
            // Laser triggers are sampled once per substep. A body falling
            // through a beam cannot open gates for bodies moving in the same
            // substep; plate contact still takes effect immediately.
            let lasers_lit = self.laser_triggers_lit();
            let blocks_fall = |state: &Self, target: Position, cube: bool| {
                let solid_tile = if state.level.tile_at(target).is_gate() {
                    (lasers_lit || state.pressure_plates_pressed_ignoring_cube(ignored_plate_cube))
                        && !state.body_occupies(target)
                } else {
                    state.is_solid_tile(target)
                };
                (!held_open_gates.contains(&target) && solid_tile)
                    || state.cubes.iter().any(|other| other.position == target)
                    || (cube && state.level.tile_at(target) == Tile::Skull)
            };
            for (_, body) in bodies {
                if self.status == GameStatus::GameOver {
                    return;
                }
                // The projectile-collision preview must record every sweep, so
                // only the real pass applies laser damage.
                let real_pass = swept_cube_positions.is_none();
                match body {
                    None if player_falls => {
                        let target = self.level.wrap(self.player.position.offset(0, 1));
                        if !blocks_fall(self, target, false) {
                            self.player.position = target;
                            if real_pass {
                                self.apply_laser_damage();
                            }
                        }
                    }
                    Some(index) if self.cubes[index].source != CubeSource::Blue => {
                        let target = self.level.wrap(self.cubes[index].position.offset(0, 1));
                        let gates_closed = lasers_lit
                            || self.pressure_plates_pressed_ignoring_cube(ignored_plate_cube);
                        if !blocks_fall(self, target, true) {
                            self.cubes[index].position = target;
                            if let Some(swept) = swept_cube_positions
                                .as_deref_mut()
                                .filter(|_| self.cubes[index].source != CubeSource::Glass)
                            {
                                swept.push(target);
                            }
                            if self.cubes[index].source == CubeSource::Player {
                                self.move_blue_cubes(
                                    0,
                                    1,
                                    &[],
                                    gates_closed,
                                    swept_cube_positions.as_deref_mut(),
                                );
                            }
                            if target == self.player.position {
                                self.status = GameStatus::GameOver;
                            } else if real_pass {
                                // A falling cube can uncover a beam aimed at the player.
                                self.apply_laser_damage();
                            }
                        }
                    }
                    None | Some(_) => {}
                }
            }
            if self.status == GameStatus::GameOver {
                return;
            }
            // Red cubes react to plates between substeps, so the body indices
            // above stay valid while a substep runs.
            ignored_plate_cube = self.update_red_cubes(ignored_plate_cube);
            if swept_cube_positions.is_none() {
                self.apply_laser_damage();
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
                CubeSource::Glass => 'g',
                CubeSource::Red => 'r',
                CubeSource::Blue => 'b',
            }
        } else if let Some(projectile) = self.projectile.filter(|p| p.position == position) {
            match projectile.direction {
                Direction::Left => '<',
                Direction::Right => '>',
            }
        } else if self.inactive_red_cubes.contains(&position) {
            'r'
        } else {
            self.level.tile_at(position).symbol()
        }
    }

    fn try_walk(&mut self, direction: Direction) -> bool {
        let dx = direction.dx();
        let destination = self.level.wrap(self.player.position.offset(dx, 0));
        let mut target = destination;
        let mut chain = Vec::new();
        // Check the entire chain before moving anything. Only walking pushes;
        // jumping and gravity still use ordinary solid-tile collision checks.
        loop {
            if self.is_solid_tile(target) {
                return false;
            }
            match self.cubes.iter().position(|cube| cube.position == target) {
                // A blue cube ahead of the player's cube moves the same way
                // at the same time, so the chain carries it along.
                Some(index) if self.cubes[index].source == CubeSource::Blue => {
                    if !chain
                        .iter()
                        .any(|&index: &usize| self.cubes[index].source == CubeSource::Player)
                    {
                        return false;
                    }
                    chain.push(index);
                    target = self.level.wrap(target.offset(dx, 0));
                }
                Some(index) => {
                    if !self.cube_is_grounded_ignoring_plate_cube(index, None)
                        || (!self.settings.allow_airborne_pushing && !self.is_grounded())
                    {
                        return false;
                    }
                    chain.push(index);
                    target = self.level.wrap(target.offset(dx, 0));
                }
                None => {
                    if !chain.is_empty() && self.level.tile_at(target) == Tile::Skull {
                        return false;
                    }
                    break;
                }
            }
        }
        let pushed_player_cube = chain
            .iter()
            .any(|&index| self.cubes[index].source == CubeSource::Player);
        let gates_closed = self.pressure_plates_active();
        for &index in &chain {
            self.cubes[index].position = self.level.wrap(self.cubes[index].position.offset(dx, 0));
        }
        self.player.position = destination;
        if pushed_player_cube {
            self.move_blue_cubes(dx, 0, &chain, gates_closed, None);
        }
        true
    }

    /// Moves every blue cube one tile along with the player's cube. Blue cubes
    /// leading in the direction of travel move first, so a line of them moves
    /// together. A horizontally moving blue cube pushes a chain of cubes ahead
    /// of it like the player does. A blue cube stays put when solid terrain, a
    /// skull, the player, or an immovable cube occupies its target. Gates count
    /// as closed according to `gates_closed`, the plate state from before the
    /// player's cube moved, so a gate its own push opens still blocks this
    /// move. Skips the `moved` cubes, and records moved-to tiles in `swept`.
    fn move_blue_cubes(
        &mut self,
        dx: isize,
        dy: isize,
        moved: &[usize],
        gates_closed: bool,
        mut swept: Option<&mut Vec<Position>>,
    ) {
        let mut blue: Vec<_> = (0..self.cubes.len())
            .filter(|&index| {
                self.cubes[index].source == CubeSource::Blue && !moved.contains(&index)
            })
            .collect();
        blue.sort_by_key(|&index| {
            let position = self.cubes[index].position;
            Reverse(position.x * dx + position.y * dy)
        });
        for index in blue {
            let target = self.level.wrap(self.cubes[index].position.offset(dx, dy));
            let Some(chain) = self.blue_push_chain(target, dx, dy, moved, gates_closed) else {
                continue;
            };
            for moved_index in chain.into_iter().chain([index]) {
                let target = self.level.wrap(self.cubes[moved_index].position.offset(dx, dy));
                self.cubes[moved_index].position = target;
                if let Some(swept) = swept.as_deref_mut() {
                    swept.push(target);
                }
            }
        }
    }

    /// Returns the cubes a blue cube moving onto `target` pushes along, or
    /// `None` when the move is blocked. Only horizontal moves push, and only a
    /// contiguous chain of grounded non-blue cubes that are not in `moved`.
    fn blue_push_chain(
        &self,
        mut target: Position,
        dx: isize,
        dy: isize,
        moved: &[usize],
        gates_closed: bool,
    ) -> Option<Vec<usize>> {
        let mut chain = Vec::new();
        loop {
            let solid_tile = if self.level.tile_at(target).is_gate() {
                gates_closed && !self.body_occupies(target)
            } else {
                self.is_solid_tile(target)
            };
            if solid_tile
                || self.player.position == target
                || self.level.tile_at(target) == Tile::Skull
            {
                return None;
            }
            let Some(index) = self.cubes.iter().position(|cube| cube.position == target) else {
                return Some(chain);
            };
            if dy != 0
                || self.cubes[index].source == CubeSource::Blue
                || moved.contains(&index)
                || !self.cube_is_grounded_ignoring_plate_cube(index, None)
            {
                return None;
            }
            chain.push(index);
            target = self.level.wrap(target.offset(dx, dy));
        }
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
        let position = self.level.wrap(self.player.position.offset(dx, dy));
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
