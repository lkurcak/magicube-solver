mod support;

use magicube_solver::{CubeSource, GameInput, GameState, Position, Tile};
use support::{assert_level_eq, level};

fn game(drawing: &str) -> GameState {
    GameState::from_ascii(&level(drawing)).unwrap()
}

#[test]
fn parses_and_renders_gates_and_pressure_plates() {
    let game = game(
        r#"
#####
#@PD#
#####
        "#,
    );

    assert_eq!(
        game.level().tile_at(Position { x: 2, y: 1 }),
        Tile::PressurePlate
    );
    assert_eq!(game.level().tile_at(Position { x: 3, y: 1 }), Tile::Gate);
    assert_level_eq(
        &game.to_ascii(),
        r#"
#####
#@PD#
#####
"#,
    );
}

#[test]
fn player_makes_gates_solid_only_while_on_a_pressure_plate() {
    let initial = game(
        r#"
########
#@P D  #
########
        "#,
    );
    let gate = Position { x: 4, y: 1 };

    assert!(!initial.is_solid(gate));
    let pressed = initial.step(GameInput::Right);
    assert!(pressed.pressure_plates_active());
    assert!(pressed.is_solid(gate));

    let released = pressed.step(GameInput::Right);
    assert!(!released.pressure_plates_active());
    assert!(!released.is_solid(gate));
    assert_eq!(
        released.step(GameInput::Right).player().position,
        Position { x: 4, y: 1 }
    );
}

#[test]
fn either_kind_of_cube_can_make_every_gate_solid() {
    for cube in ['C', 'O'] {
        let initial = game(&format!(
            r#"
#########
#@{cube} P D #
#########
"#
        ));
        let pressed = initial.step(GameInput::Right).step(GameInput::Right);

        assert_eq!(pressed.cubes()[0].position, Position { x: 4, y: 1 });
        assert!(pressed.pressure_plates_active());
        assert!(pressed.is_solid(Position { x: 6, y: 1 }));
    }
}

#[test]
fn occupied_gate_waits_for_cube_and_player_to_leave_before_closing() {
    let initial = game(
        r#"
############
#PC@  DC   #
############
        "#,
    );
    let gate = Position { x: 6, y: 1 };

    // Fire the player's cube onto the open gate, using the map cube to stop it.
    let gate_occupied = initial
        .step(GameInput::Shoot)
        .step(GameInput::Right)
        .step(GameInput::Wait);
    assert!(
        gate_occupied
            .cubes()
            .iter()
            .any(|cube| cube.position == gate)
    );

    // Push the left map cube onto the plate, then approach the occupied gate.
    let at_gate = gate_occupied
        .step(GameInput::Left)
        .step(GameInput::Right)
        .step(GameInput::Right)
        .step(GameInput::Right);
    assert!(at_gate.pressure_plates_active());

    // The cube can leave the gate while the player takes its place, so it still
    // cannot close. It closes only after the player leaves on the next push.
    let player_on_gate = at_gate.step(GameInput::Right);
    assert_eq!(player_on_gate.player().position, gate);
    assert!(!player_on_gate.is_solid(gate));

    let gate_freed = player_on_gate.step(GameInput::Right);
    assert_eq!(gate_freed.player().position, Position { x: 7, y: 1 });
    assert!(gate_freed.pressure_plates_active());
    assert!(gate_freed.is_solid(gate));
}

#[test]
fn newly_spawned_cube_reactivates_a_plate_only_after_gravity() {
    for gate in ['D', 'X'] {
        let ready = game(&format!("    C\n\n\n  O\n@ P#{gate}#\n#### #"))
            .step(GameInput::Wait)
            .step(GameInput::Wait);
        assert!(ready.pressure_plates_active());
        assert!(ready.cubes().iter().any(|cube| {
            cube.source == CubeSource::Map && cube.position == Position { x: 4, y: 3 }
        }));

        let fired = ready.step(GameInput::Shoot).step(GameInput::Right);

        assert!(fired.pressure_plates_active());
        assert!(fired.is_solid(Position { x: 4, y: 4 }));
        assert!(fired.cubes().iter().any(|cube| {
            cube.source == CubeSource::Player && cube.position == Position { x: 2, y: 4 }
        }));
        assert!(fired.cubes().iter().any(|cube| {
            cube.source == CubeSource::Map && cube.position == Position { x: 4, y: 5 }
        }));
    }
}

#[test]
fn ordinary_falling_cube_still_activates_a_plate_during_gravity() {
    let initial = game(
        r#"
    C
@ C D
# P
  #
        "#,
    );

    let fallen = initial.step(GameInput::Wait);

    assert!(fallen.pressure_plates_active());
    assert!(
        fallen
            .cubes()
            .iter()
            .any(|cube| { cube.position == Position { x: 2, y: 2 } })
    );
    assert!(
        fallen
            .cubes()
            .iter()
            .any(|cube| { cube.position == Position { x: 4, y: 0 } })
    );
}

#[test]
fn active_gates_block_players_cubes_and_projectiles() {
    let player = game(
        r#"
#####
#@PD#
#####
        "#,
    )
    .step(GameInput::Right);
    assert_eq!(player.step(GameInput::Right), player);

    let cube = game(
        r#"
########
#@C PCD#
########
        "#,
    )
    .step(GameInput::Right)
    .step(GameInput::Right);
    assert_eq!(cube.step(GameInput::Right), cube);

    let projectile = game(
        r#"
#########
#D @C P #
#########
        "#,
    )
    .step(GameInput::Right)
    .step(GameInput::Right)
    .step(GameInput::Shoot)
    .step(GameInput::Left)
    .step(GameInput::Wait);
    assert!(projectile.projectile().is_none());
    assert!(
        projectile
            .cubes()
            .iter()
            .any(|cube| cube.position == Position { x: 2, y: 1 })
    );
}

#[test]
fn gate_goals_follow_gate_collision_rules() {
    let open = game(
        r#"
######
#@ X #
######
        "#,
    );
    let gate_goal = Position { x: 3, y: 1 };
    assert!(!open.is_solid(gate_goal));

    let closed = game(
        r#"
######
#@PX #
######
        "#,
    )
    .step(GameInput::Right);
    assert!(closed.is_solid(gate_goal));
    assert_eq!(closed.step(GameInput::Right), closed);

    let shot = game(
        r#"
#########
#X @C P #
#########
        "#,
    )
    .step(GameInput::Right)
    .step(GameInput::Right)
    .step(GameInput::Shoot)
    .step(GameInput::Left)
    .step(GameInput::Wait);
    assert!(shot.projectile().is_none());
    assert!(
        shot.cubes()
            .iter()
            .any(|cube| cube.position == Position { x: 2, y: 1 })
    );
}
