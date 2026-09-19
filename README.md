# Magicube solver

A work-in-progress solver for Magicube, including a small computer-vision tool
that converts pixel-perfect level screenshots into character tile maps.

The importer uses 8x8 image templates and detects each screenshot's grid phase
independently. Manually labeled levels are authoritative training fixtures;
unknown visual variants are emitted as `?` for later labeling.

## Usage

Generate maps under the ignored `data/levels/` directory:

```sh
cargo run --bin screenshots-to-maps
```

Print level 1 with the original prototype binary:

```sh
cargo run --bin magicube-solver
```

The search engine is available through the Rust library API described below.

Open the interactive level selector for the bundled levels:

```sh
cargo run -p magicube-play
```

Or load an ASCII level file:

```sh
cargo run -p magicube-play -- data/level-manual-labels/2.txt
```

The player runs in a separate workspace crate using Crossterm for immediate
keyboard input and colored terminal rendering, with one character per tile just
like the level files. Movement, firing, and waiting advance the core simulator;
aiming pauses time. A successful shot is displayed for 120 ms, then the game
automatically advances one recovery update before accepting another action.
Keys pressed during that pause are handled after recovery. No Enter key is needed, and holding a
key uses the terminal's normal key repeat. Resizing redraws without advancing time.

| Key | Action |
| --- | --- |
| Left / A | Move or push left |
| Right / D | Move or push right |
| Z | Jump |
| S / . | Wait one update |
| X | Enter aiming mode, or cancel it |
| Left / Right while aiming | Fire without moving sideways |
| Down / U / Backspace | Undo one action, including its automatic recovery, cubes, projectiles, and airtime |
| Hold Up / R | Restart the level |
| P | Save the current input sequence, including partial attempts |
| Q / Esc / Ctrl-C | Quit |

The startup selector uses Ratatui's stateful list widget. Choose a bundled level
with Up/Down or J/K, preview it alongside the list, and press Enter to play.
Passing a level file on the command line skips the selector.

The status line shows position, airtime, game status, and the last input.
`C` represents a map cube, `O` the player's cube, and `<`/`>` a projectile.
`P` is a pressure plate and `D` is a gate. Gates are normally passable, but become
solid while the player or either kind of cube occupies a pressure plate.
A falling cube crushing the player forces undo or restart before further play.
Place your `O` cube on `G` to win; the victory screen supports undo and restart too.
The view follows the
player when the level is larger than the terminal or the player leaves the map.
The terminal is restored on exit, errors, and panics. Run
`cargo run -p magicube-play -- --help` for usage without entering interactive mode.

Winning automatically saves a JSON record of your solution, including wins during
shot recovery. Press **P** to save manually at any time, including partial attempts.
The default directory comes from the `directories` crate:

| Platform | Default solutions directory |
| --- | --- |
| macOS | `~/Library/Application Support/magicube/solutions/` |
| Linux | `$XDG_DATA_HOME/magicube/solutions/`, or `~/.local/share/magicube/solutions/` |
| Windows | `%APPDATA%\magicube\data\solutions\` |

Override it for a particular run:

```sh
cargo run -p magicube-play -- --solutions-dir ./solutions data/level-manual-labels/2.txt
```

Directories are created only when saving. Each save gets a new filename, so
previous attempts are preserved. The filename is shown in the terminal and the
last saved path is printed again on exit for copying. Each victory triggers one
automatic save; undoing or restarting and winning again saves a new record.
A save failure is displayed without interrupting play; press **P** to retry.
Saving does not advance time or add an undo entry.

Records include a format version, game version, level name, the complete starting
ASCII map, outcome (`in_progress`, `won`, or `game_over`), and an ordered `inputs`
array. Input names are `left`, `right`, `jump`, `shoot`, and `wait`;
aiming/cancelling are retained as `shoot` inputs. Automatic recovery updates are
recorded explicitly as `wait`. Undone inputs are excluded and
restart clears the sequence. A partial attempt can also be saved while aiming.
The embedded map makes the record usable without the original level file or
checkout: parse `level.map` and apply `inputs` in order to replay it.

Run the test suite, including exact reproduction of all authoritative levels:

```sh
cargo test --workspace
```

See [`data/tile-templates/README.md`](data/tile-templates/README.md) for the
template and manual-label workflow.

## Solver library

`solve` searches from any `GameState`, including one that is already aiming or
recovering, and leaves it unchanged:

```rust
use magicube_solver::{GameState, GameStatus, SolveOptions, SolveOutcome, solve};

let initial = GameState::from_ascii("#####\n#@G##\n#####").unwrap();
let result = solve(&initial, SolveOptions::default());
match result.outcome {
    SolveOutcome::Solved(inputs) => {
        println!("Solved in {} inputs: {inputs:?}", inputs.len());
        let won = inputs.into_iter().fold(initial, |state, input| state.step(input));
        assert_eq!(won.status(), GameStatus::Won);
    }
    SolveOutcome::Unsolvable => println!("No winning sequence exists"),
    SolveOutcome::StateLimitReached => println!("Search incomplete; increase the state limit"),
}
println!("Search statistics: {:?}", result.stats);
```

The synchronous, single-threaded breadth-first search returns a shortest sequence
of `GameInput` entries. Every entry costs one, including entering/cancelling aim
and forced recovery (recorded explicitly as `Wait`). This minimizes recorded
inputs, not elapsed simulation updates. Equally short solutions are selected
deterministically using left, right, jump, shoot, wait order; recovery always uses
wait. Returned inputs replay directly through `GameState::step`.

The default cap is one million distinct retained states, including the initial
state and any discovered win. Set `SolveOptions { max_states: Some(2_000_000) }`
to raise it, or `SolveOptions { max_states: None }` to remove it. A zero cap stops
immediately. Already-won states return an empty solution and game-over states
return `Unsolvable`, regardless of the cap; both require no search.

`Unsolvable` means the reachable search space was exhausted (or the initial state
was game over). `StateLimitReached` makes no claim about solvability. Open maps
allow objects to travel indefinitely outside the drawing, so searches without a
cap may never finish and can consume unbounded memory. Search does not clip
coordinates or change the simulator's rules.

`SolveStats` reports `discovered_states` (distinct retained states, excluding
discarded game-over successors) and `expanded_states` (states whose successor
generation began, including a partially processed final state). Terminal starts
and a zero-cap search report zero for both counts.

The normal test suite solves and replays bundled levels 1–3. To run only those
regressions with optimizations and display their search counts:

```sh
cargo test --release --locked --test solver solves_bundled -- --nocapture
```

## Game simulation

```rust
use magicube_solver::{GameInput, GameState};

let initial = GameState::from_ascii("#######\n#     #\n# @   #\n#######").unwrap();
let next = initial
    .step(GameInput::Jump)
    .step(GameInput::Right)
    .step(GameInput::Wait);
println!("{}", next.to_ascii());
```

`GameState::step` returns a new state and leaves its input unchanged. States share
an immutable `Level`; dynamic state includes the player, aiming/recovery mode, cubes,
projectile, and game status. Equality and hashing include all of these. The solver
compares the complete dynamic state while omitting the shared level from hashing.

Movement updates work as follows:

- Left/right attempt to move one tile. `#` walls, closed `D` gates, and both kinds of cube are solid
  and support the player. Walking into a cube pushes the entire contiguous row
  of cubes one tile, provided the space beyond it is not solid. There is no limit
  on chain length, and a wall blocks the whole push. Pushed cubes then fall normally.
  Other map symbols do not block player or cube movement.
- Jump requires solid support immediately below and free space above. It rises one tile
  and grants two subsequent air inputs. Left/right and waiting spend those inputs;
  blocked moves and ignored airborne jump attempts also consume time.
- Gravity resumes after movement on the second air input. Without airtime, every
  update applies gravity after movement: descend at most two tiles, checking for
  solid obstacle before each tile. Landing ends any remaining airtime.
- `Wait` advances time without horizontal movement.

`P` pressure plates are passable. While the player or any cube occupies any
pressure plate, every `D` gate behaves like a wall for movement, gravity, and
projectiles. Otherwise gates are passable.

`Shoot` toggles aiming without advancing time. While aiming, left/right attempts
to fire and other movement inputs are ignored. A blocked shot stays in aiming mode
and preserves the previous cube and projectile. The adjacent tile must be
unoccupied and passable before firing (empty, goal, torch, skull, pressure plate,
or an inactive gate).
A successful shot immediately removes the
previous `O` cube and replaces any previous projectile, leaving map cubes intact.

A successful shot sets `player().mode` to `PlayerMode::Recovering`. The next
update ignores player actions (including jump and shoot), advances projectiles,
spends airtime, and applies gravity normally, then returns to `Normal`. Call
`step(GameInput::Wait)` for this forced update. Blocked shots and cancelled aiming
do not trigger recovery. A win or game over freezes the state immediately,
including when it happens during firing or recovery.

Projectiles move two tiles per update, including the firing update, checking each
tile in order. Goals, torches, and skulls are passable; walls, unknown terrain, and occupied tiles stop
a projectile and create an `O` cube in its last free position. After projectile movement, gravity runs in
two single-tile substeps for both player and cubes, processing lower bodies first.
This lets stacks fall together and prevents cubes from skipping through platforms
or the player. Newly spawned cubes participate in gravity immediately. A cube
entering the player's tile causes game over; further simulation inputs do nothing.

`G` is a nonsolid target and does not stop players, cubes, or projectiles. At the
end of an update, the player's `O` cube occupying a goal wins the level, whether
it arrived by spawning, pushing, or falling. A map cube or the player reaching
the goal does not win. Winning freezes the state until undo or restart.

Map coordinates increase rightward/downward. Outside the drawing is empty space;
leaving the map has no special effect yet. Torches and skulls are decorative for
now and do not affect movement, projectiles, or gravity. `to_ascii()`
renders only the original map rectangle, so use `player().position` to inspect a
player outside it.
Maps require exactly one `@` and can include map cubes (`C`), at most one existing
player cube (`O`), gates (`D`), and pressure plates (`P`). Ragged rows are padded
with empty tiles, matching the screenshot importer's format.
