# Magicube solver

A work-in-progress solver for Magicube, including a small computer-vision tool
that converts pixel-perfect level screenshots into character tile maps.

The importer uses 8x8 image templates and detects each screenshot's grid phase
independently. Manually labeled levels are authoritative training fixtures;
unknown or conflicting visual variants are emitted as `?` for later labeling.
Adding a PNG to `data/level-screenshots/` automatically adds it to the compiled
player's level selector on the next build. No ASCII transcription or level-list
edit is needed.

## Usage

Export maps and repair diagnostics under the ignored `cache/levels/` directory:

```sh
cargo run --bin screenshots-to-maps
```

This command is optional for playing: the player imports and embeds levels during
its Cargo build, using the same catalog builder. Generated build assets live in
Cargo's `OUT_DIR`; the compiled game needs no screenshots or data directory at runtime.
Screenshot filenames determine level IDs and numeric ordering (1, 2, …, 10).
The build watches screenshots, manual labels, the atlas, and its referenced templates.
Removing a screenshot removes its bundled entry on the next build.

Only clean imports can be played or solved from the selector. A clean import has
unambiguous alignment and tile recognition, parses as a game map, and contains a
goal. This does not prove solvability. Problem entries are listed as **Corrupted**
and can be previewed when a map is available; play and replay actions are disabled.
One bad screenshot does not prevent the other levels from being bundled. Broken
shared atlas configuration fails the build.

When a level is corrupted, run `screenshots-to-maps` and inspect
`cache/levels/import-report.txt` and `cache/levels/unknown-tiles/`. Add a labeled
template or a manual level example, then rebuild. The converter exports available
maps even when some levels fail, and exits unsuccessfully if any entry is corrupted.
It accepts optional `[screenshots-dir] [levels-dir] [atlas-dir] [labels-dir]` arguments.

## Project progress dashboard

`data/` contains only authored, trusted inputs. `data/level-manifest.txt` lists
the expected level IDs; screenshots, manual maps, and tile templates beneath
`data/` are ground truth. Everything persisted beneath the ignored `cache/`
directory is reproducible and safe to delete.
The former ignored `data/levels/` output is no longer read and can be removed.

Run the one-stop progress dashboard in release mode so solver searches are fast:

```sh
cargo run --release -p magicube-play --bin magicube-progress
```

It rebuilds inferred maps and unknown-tile diagnostics, verifies cached solutions
against the freshly imported maps and current game rules, and then solves clean
unresolved levels one at a time. Failed searches are reused only for the same
map, state limit, and compiled dashboard. Select a solved row and press Enter to
open its replay. Deleting `cache/` makes the next run reconstruct everything.

Print level 1 with the original prototype binary:

```sh
cargo run --bin magicube-solver
```

The search engine is available through the Rust library API and the terminal
player's `--solve` replay mode described below.

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
with Up/Down or J/K and preview it alongside the list:

| Key | Level selector action |
| --- | --- |
| Enter | Play the highlighted level |
| S | Solve the highlighted level and open its replay |
| R | Browse saved attempts for the highlighted level |
| F | Toggle airborne shooting for manual play (off by default) |
| P | Toggle airborne pushing for manual play (off by default) |
| Q / Esc | Quit |

Closing a solver replay returns to the level selector, retaining the highlighted
level. Closing a saved replay returns to the saved-attempt list; Q/Esc there goes
back to the level selector. The saved-attempt list shows outcome, input count,
shooting/pushing rules, and filename, newest first. It matches the embedded starting map, so renamed
levels and files still work. It uses the normal solutions directory, including
any `--solutions-dir` override. Browsing an empty directory does not create it.
Passing a level file on the command line skips the selector.

By default, aiming, firing, and pushing cubes require ground support, including
standing on another cube. Pressing X without support does nothing and does not
advance time. A blocked airborne push still advances time and gravity; walking
through empty space in midair remains allowed.
For fun, toggle airborne shooting with **F** or airborne pushing with **P** in
the selector, or use either or both flags:

```sh
cargo run -p magicube-play -- --airborne-shooting
cargo run -p magicube-play -- --airborne-pushing
```

These independent options permit shots or pushes throughout jumps and falls.
They apply to manual play for the session and are preserved by undo and restart.
You can also combine the flags with a level-file path. The game and replay
headers show both active rules. **S** and `--solve` always solve with the default
grounded-only rules, regardless of the manual-play toggles. During gameplay,
**P** still saves your inputs; it only toggles pushing in the selector.

The status line shows position, airtime, game status, and the last input.
`C` represents a map cube, `O` the player's cube, and `<`/`>` a projectile.
`P` is a pressure-plate base, `G` is a goal pedestal, and `D` is a gate.
Both bases are solid; their interactive position is the cell immediately above.
Gates are normally passable, but become solid while the player or either kind of
cube occupies the cell above a pressure-plate base. An occupied gate stays open
until both the player and all cubes have left its tile.
A falling cube crushing the player forces undo or restart before further play.
Place your `O` cube immediately above `G` to win; a `D` in that cell naturally
acts as both a gate and a goal. The victory screen supports undo and
restart too.
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

New records use format version **5** and include a game version, level name, the
complete starting ASCII map,
`settings: { "allow_airborne_shooting": false, "allow_airborne_pushing": false, "projectile_tiles_per_update": 3 }`,
outcome (`in_progress`, `won`, or `game_over`), and an ordered `inputs`
array. Input names are `left`, `right`, `jump`, `shoot`, and `wait`;
aiming/cancelling are retained as `shoot` inputs. Automatic recovery updates are
recorded explicitly as `wait`. Undone inputs are excluded and
restart clears the sequence. A partial attempt can also be saved while aiming.
The embedded map makes the record usable without the original level file or
checkout: parse `level.map` with the recorded settings and apply `inputs` in
order to replay it. Versions 1–4 used the previous level encoding and are
rejected rather than silently interpreting their maps with the new base-tile
semantics.

Run the test suite, including exact reproduction of all authoritative levels:

```sh
cargo test --workspace
```

See [`data/tile-templates/README.md`](data/tile-templates/README.md) for the
template and manual-label workflow.

## Replay saved inputs and solver solutions

Start `cargo run -p magicube-play` and use **S** or **R** in the level selector
to open replays directly in the game. No extra flags are needed. Solver failures
and saved-file errors appear in the menu so you can choose another attempt.

You can also load a saved solution JSON directly, including partial attempts
and game-over records:

```sh
cargo run -p magicube-play -- --replay /path/to/solution.json
```

The embedded map supplies the starting level, so the original level file is not
needed. Replays use their recorded rules independently of the menu's current
toggles; the CLI rejects `--airborne-shooting` or `--airborne-pushing` together
with `--replay` or `--solve`.
Unsupported format versions, malformed inputs, and a final outcome that
differs from the saved outcome produce an error before entering the replay.

Find a shortest solution and open it in the same replay viewer:

```sh
cargo run --release -p magicube-play -- --solve data/level-manual-labels/4.txt
```

Omit the level path to choose a bundled level:

```sh
cargo run --release -p magicube-play -- --solve
```

With `--solve`, Enter in the selector also launches a solver replay.

The solver uses its default one-million-state cap. If it cannot find a complete
solution within that cap, or exhausts an unsolvable level, it reports the outcome
in the menu (or exits with an error when a level file was passed directly).

Replays start paused at step **0**, the original state. Step **N** is the state
after applying the first N recorded inputs. Every input is shown separately,
including aiming, cancelling, ignored inputs, and recovery waits. The header
shows the current step and total, the last and next inputs, and the game status.

| Key | Replay action |
| --- | --- |
| Left / A | Back one input |
| Right / D | Forward one input |
| Up / Page Up | Back ten inputs |
| Down / Page Down | Forward ten inputs |
| Home | Jump to the initial state |
| End | Jump to the final state |
| Space | Play or pause at four inputs per second |
| Q / Esc / Ctrl-C | Close replay (return to its menu, or exit for a direct file) |

Seeking pauses automatic playback and clamps at the start/end. Playback stops at
the final step; pressing Space there plays again from the start. Backward steps
restore the exact earlier state, including cubes, projectiles, and airtime.
Paused recovery frames stay paused until you advance them. Replays never save
new attempts or modify the recorded sequence, and normal gameplay keys such as
jump, shoot, and save are inactive in the viewer.

## Solver library

`solve` searches from any `GameState`, including one that is already aiming or
recovering, and leaves it unchanged:

```rust
use magicube_solver::{GameState, GameStatus, SolveOptions, SolveOutcome, solve};

let initial = GameState::from_ascii("#####\n#@ ##\n##G##").unwrap();
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

`GameState::from_ascii` uses `GameSettings::default()`, with airborne shooting
and pushing disabled. The library search follows its initial state's rules; deliberately
searching a fun variant requires constructing that state with
`GameState::from_ascii_with_settings(map, GameSettings { allow_airborne_shooting: true, ..GameSettings::default() })`.
Use `allow_airborne_pushing: true` to opt into airborne pushing independently.
The terminal's solver entry points always construct the default state.

The default cap is one million distinct retained states, including the initial
state and any discovered win. Set `SolveOptions { max_states: Some(2_000_000) }`
to raise it, or `SolveOptions { max_states: None }` to remove it. A zero cap stops
immediately. Already-won states return an empty solution and game-over states
return `Unsolvable`, regardless of the cap; both require no search.

`Unsolvable` means the reachable search space was exhausted (or the initial state
was game over). `StateLimitReached` makes no claim about solvability. Open maps
allow objects to travel indefinitely beyond the horizontal edges, so searches
without a cap may never finish and can consume unbounded memory. Search does not
clip horizontal coordinates or change the simulator's rules.

`SolveStats` reports `discovered_states` (distinct retained states, excluding
discarded game-over successors) and `expanded_states` (states whose successor
generation began, including a partially processed final state). Terminal starts
and a zero-cap search report zero for both counts.

The normal test suite solves bundled levels 1–7 using the default state cap,
replays both saved and solver solutions, and checks that each solver solution
uses no more inputs than its saved counterpart. To run only those regressions
with optimizations and display their input and search counts:

```sh
cargo test --release --locked --test solutions -- --nocapture
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
an immutable `Level` and retain their `GameSettings`; dynamic state includes the player, aiming/recovery mode, cubes,
projectile, and game status. Equality and hashing include all of these. The solver
compares the complete dynamic state while omitting the shared level from hashing.

Movement updates work as follows:

- Left/right attempt to move one tile. `#` walls, `G` goal pedestals, `P`
  pressure-plate bases, closed `D` gates, and both kinds of cube are solid
  and support the player. Walking into a cube pushes the entire contiguous row
  of cubes one tile, provided the player is grounded and the space beyond it is
  not solid. `allow_airborne_pushing` removes only the support requirement.
  There is no limit on chain length, and a wall blocks the whole push.
  Pushed cubes then fall normally.
  Skulls block cubes but not the player. Torches and unknown tiles do not block
  player or cube movement.
- Jump requires solid support immediately below and free space above. It rises one tile
  and grants two subsequent air inputs. Left/right and waiting spend those inputs;
  blocked moves and ignored airborne jump attempts also consume time.
- Gravity resumes after movement on the second air input. Without airtime, every
  update applies gravity after movement: descend at most two tiles, checking for
  solid obstacle before each tile. Landing ends any remaining airtime.
- `Wait` advances time without horizontal movement.

`P` pressure-plate bases are solid. While the player or any cube occupies the
cell immediately above any base, every `D` gate behaves like a wall for
movement, gravity, and projectiles once its tile is empty. A gate occupied when
the plate is pressed stays open; if a cube is pushed off and the player takes
its place, it waits for the player to leave before closing. Otherwise gates are
passable.

With default settings, `Shoot` enters aiming only while grounded. Unsupported
attempts are ignored without advancing time. Set `allow_airborne_shooting` to
allow entering aim and firing anywhere in the air, including during jumps.
Entering/cancelling aim never advances time. While aiming, left/right attempts
to fire and other movement inputs are ignored. A blocked shot stays in aiming mode
and preserves the previous cube and projectile. The adjacent tile must be
unoccupied and passable before firing (empty, torch, skull, or an inactive gate).
A derived goal or pressure-plate position is otherwise an ordinary empty cell.
A successful shot immediately removes the
previous `O` cube and replaces any previous projectile, leaving map cubes intact.

A successful shot sets `player().mode` to `PlayerMode::Recovering`. The next
update ignores player actions (including jump and shoot), advances projectiles,
spends airtime, and applies gravity normally, then returns to `Normal`. Call
`step(GameInput::Wait)` for this forced update. Blocked shots and cancelled aiming
do not trigger recovery. A win or game over freezes the state immediately,
including when it happens during firing or recovery.

Projectiles move `projectile_tiles_per_update` tiles per update (three by default),
including the firing update and checking each tile in order. A falling cube
blocks projectiles across its complete vertical gravity sweep for that update,
including its starting, intermediate, and destination tiles. This transient
occupancy affects projectile collision only. Derived goal and pressure-plate
positions, torches, and skulls are passable; walls, feature bases, unknown
terrain, and occupied tiles stop a projectile and create an `O` cube in its
last free position. If that position is a skull, the projectile is destroyed
without creating the cube. After projectile movement, gravity runs in two
single-tile substeps for both player and cubes, processing lower bodies first.
This lets stacks fall together and prevents cubes from skipping through platforms,
skulls, or the player. Newly spawned cubes participate in gravity immediately.
A cube spawned immediately above a pressure-plate base does not reactivate gates
until that update's gravity pass finishes; other plate occupants continue to
affect gates immediately. A cube entering the player's tile causes game over;
further simulation inputs do nothing.

`G` is a solid goal pedestal. At the end of an update, the player's `O` cube
occupying the cell immediately above a pedestal wins, whether
it arrived by spawning, pushing, or falling. A map cube or the player reaching
that cell does not win. A `D` may occupy the goal cell independently, so its
passability follows the pressure-plate gate rules. Winning freezes the state until
undo or restart.

Map coordinates increase rightward/downward. Moving below the level's bottom
boundary wraps a player or falling cube to the top row. The map itself does not
repeat: space above the top and beyond either horizontal edge is empty. Torches
are decorative. Skulls are passable for players and projectiles but act as walls
for cubes. `to_ascii()` renders only the original map rectangle, so use
`player().position` to inspect a player outside it.
Maps require exactly one `@` and can include map cubes (`C`), at most one existing
player cube (`O`), gates (`D`), goal pedestals (`G`), and pressure-plate bases
(`P`). Ragged
rows are padded with empty tiles, matching the screenshot importer's format.
