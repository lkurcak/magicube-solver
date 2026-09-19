# Simulator tests

Run `cargo test --locked --workspace` to execute the movement examples and the
screenshot importer tests.

`movement.rs` contains the before/after movement drawings and update-by-update
jump and gravity checks. `simulation_maps.rs` covers parsing imported maps and
open map edges. `shooting.rs` covers aiming, shot recovery, projectile collisions, cube ownership,
falling, and crushing. `pushing.rs` covers cube chains, blocked pushes, and pushing
off ledges. `goals.rs` covers passable goals and winning with the player's cube.
`level_drawing.rs` checks the ASCII fixture helper.

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

    let next = initial.step(GameInput::MoveRight);

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
