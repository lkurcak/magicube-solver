# Simulator and solver tests

Run `cargo test --locked --workspace` to execute the simulator, solver, terminal
player, and screenshot importer tests.

`movement.rs` contains the before/after movement drawings and update-by-update
jump and gravity checks. `simulation_maps.rs` covers parsing imported maps and
open map edges. `shooting.rs` covers aiming, shot recovery, projectile collisions, cube ownership,
falling, crushing, default grounded-only shots, and optional airborne shots.
`pushing.rs` covers cube chains, blocked and falling cubes, pushing off ledges,
grounded-only defaults, and independent airborne-pushing opt-in during jumps.
`lasers.rs` covers emitter and trigger parsing, beam blocking and crossing,
player death by walking, jumping, or falling into beams and by losing a cube's
shelter, and triggers acting as pressure plates.
`red_cubes.rs` covers red plates and red cubes: intangible inactive cubes,
materializing only into a free tile, dematerializing in place, pushing and plate
pressing while active, and independence from gates and laser triggers.
`goals.rs` covers passable positions above solid pedestals and winning with the
player's cube.
`solutions.rs` replays saved solutions for levels 1–7, solves each with the default
state cap, replays the solver's result, and checks that it is no longer than the
saved input sequence. Length bounds come directly from the saved inputs.
`solver.rs` finds and replays shortest solutions for tiny fixtures, compares them
with exhaustive enumeration of shorter inputs, and covers terminal states,
aiming/recovery, state limits, cycles, and open maps.
`level_drawing.rs` checks the ASCII fixture helper.
`catalog.rs` checks screenshot discovery and natural ordering, isolated import
failures, learning new tile patterns across levels, conflicting teaching examples,
and converter reports/crop exports. Library tests reproduce all manual fixtures
and cover ambiguous grid phases, transactional training, and conflict diagnostics.

The `magicube-play` crate also tests replay navigation, exact snapshot restoration,
play/pause timing and endpoint behavior, saved-record validation, solver-to-replay
integration, replay controls/rendering, and command-line mode selection.
Menu tests cover solver/saved-replay actions and empty lists; save discovery
tests check map matching, directory overrides, and newest-first ordering.
Menu tests also check screenshot-only bundled levels, corrupted previews, and
blocked launch actions for corrupted imports.
Replay records test version-5 settings, rejection of pre-migration formats, and
settings preservation on round trips, undo and restart.

Run the bundled solver regressions with optimizations and visible search counts:

```sh
cargo test --release --locked --test solutions -- --nocapture
```

`support::level` removes shared test indentation. Production parsing preserves
indentation because leading spaces are meaningful in imported levels.

```rust
mod support;

use magicube_solver::{GameInput, GameState};
use support::{assert_level_eq, level};

#[test]
fn player_moves_right() {
    let initial = GameState::from_ascii(&level(
        r#"
            #####
            #@  #
            #####
        "#,
    )).unwrap();

    let next = initial.step(GameInput::Right);

    assert_level_eq(
        &next.to_ascii(),
        r#"
            #####
            # @ #
            #####
        "#,
    );
}
```

A jump consumes its own update and rises one tile. The next two inputs spend
airtime; gravity runs at the end of the second one. A blocked sideways input
still consumes time, so a blocked-jump example waits out the remaining air input
before asserting that the player is back on the ground.
