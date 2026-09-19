mod support;

use magicube_solver::{GameInput, GameState, Position, Tile};
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
    assert_level_eq(&game.to_ascii(), "#####\n#@PD#\n#####");
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
        let initial =
            GameState::from_ascii(&format!("#########\n#@{cube} P D #\n#########")).unwrap();
        let pressed = initial.step(GameInput::Right).step(GameInput::Right);

        assert_eq!(pressed.cubes()[0].position, Position { x: 4, y: 1 });
        assert!(pressed.pressure_plates_active());
        assert!(pressed.is_solid(Position { x: 6, y: 1 }));
    }
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
