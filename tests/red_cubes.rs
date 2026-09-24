mod support;

use magicube_solver::{Cube, CubeSource, GameInput, GameState, Position, Tile};
use support::{assert_level_eq, level};

fn game(drawing: &str) -> GameState {
    GameState::from_ascii(&level(drawing)).unwrap()
}

fn red(x: isize, y: isize) -> Cube {
    Cube {
        position: Position { x, y },
        source: CubeSource::Red,
    }
}

#[test]
fn parses_and_renders_red_cubes_and_red_plate_bases() {
    let game = game(
        r#"
#####
#@ r#
##R##
        "#,
    );

    assert_eq!(
        game.level().tile_at(Position { x: 2, y: 2 }),
        Tile::RedPressurePlateBase
    );
    assert!(game.level().is_red_pressure_plate(Position { x: 2, y: 1 }));
    assert!(!game.level().is_pressure_plate(Position { x: 2, y: 1 }));
    assert!(game.cubes().is_empty());
    assert_eq!(game.inactive_red_cubes(), [Position { x: 3, y: 1 }]);
    assert_level_eq(
        &game.to_ascii(),
        r#"
#####
#@ r#
##R##
"#,
    );
}

#[test]
fn inactive_red_cubes_float_and_let_the_player_through() {
    let initial = game(
        r#"
#####
#@r #
## ##
#####
        "#,
    );

    let waited = initial.step(GameInput::Wait);
    assert_eq!(waited.inactive_red_cubes(), [Position { x: 2, y: 1 }]);

    // The player walks into the floating cube's tile and falls straight through it.
    let fallen = initial.step(GameInput::Right);
    assert_eq!(fallen.player().position, Position { x: 2, y: 2 });
    assert_eq!(fallen.inactive_red_cubes(), [Position { x: 2, y: 1 }]);
    assert_eq!(fallen.symbol_at(Position { x: 2, y: 1 }), 'r');
    assert!(fallen.cubes().is_empty());
}

#[test]
fn pressing_a_red_plate_materializes_red_cubes_and_releasing_it_dematerializes_them_in_place() {
    let initial = game(
        r#"
#######
#   r #
#     #
#     #
#     #
#@    #
##R####
        "#,
    );

    let pressed = initial.step(GameInput::Right);
    assert!(pressed.red_pressure_plates_active());
    assert!(!pressed.pressure_plates_active());
    assert_eq!(pressed.cubes(), [red(4, 3)]);
    assert!(pressed.inactive_red_cubes().is_empty());

    // Released mid-fall, the cube stays where it vanished.
    let released = pressed.step(GameInput::Left);
    assert!(released.cubes().is_empty());
    assert_eq!(released.inactive_red_cubes(), [Position { x: 4, y: 3 }]);
    assert_eq!(
        released.step(GameInput::Wait).inactive_red_cubes(),
        [Position { x: 4, y: 3 }]
    );

    let pressed_again = released.step(GameInput::Right);
    assert_eq!(pressed_again.cubes(), [red(4, 5)]);
}

#[test]
fn a_red_cube_materializes_only_once_its_tile_is_cleared() {
    let initial = game(
        r#"
######
#C   #
#    #
# @r #
#R####
        "#,
    );

    // The map cube lands on the red plate while the player stands in the red cube.
    let blocked = initial.step(GameInput::Right);
    assert!(blocked.red_pressure_plates_active());
    assert_eq!(blocked.player().position, Position { x: 3, y: 3 });
    assert_eq!(blocked.inactive_red_cubes(), [Position { x: 3, y: 3 }]);
    assert_eq!(blocked.cubes().len(), 1);

    let cleared = blocked.step(GameInput::Right);
    assert_eq!(cleared.player().position, Position { x: 4, y: 3 });
    assert!(cleared.inactive_red_cubes().is_empty());
    assert!(cleared.cubes().contains(&red(3, 3)));
}

#[test]
fn active_red_cubes_are_pushable_and_press_ordinary_plates() {
    let initial = game(
        r#"
#########
#C @r  D#
#R##P####
        "#,
    );
    let gate = Position { x: 7, y: 1 };

    // A red plate pressed from the start materializes the cube immediately.
    assert!(initial.cubes().contains(&red(4, 1)));
    assert!(initial.pressure_plates_active());
    assert!(initial.is_solid(gate));

    let pushed = initial.step(GameInput::Right).step(GameInput::Right);
    assert!(pushed.cubes().contains(&red(6, 1)));
    assert!(!pushed.pressure_plates_active());
    assert!(!pushed.is_solid(gate));
}

#[test]
fn red_plates_do_not_close_gates() {
    let pressed = game(
        r#"
#####
#@ D#
##R##
        "#,
    )
    .step(GameInput::Right);

    assert!(pressed.red_pressure_plates_active());
    assert!(!pressed.pressure_plates_active());
    assert!(!pressed.is_solid(Position { x: 3, y: 1 }));
}

#[test]
fn laser_triggers_do_not_press_red_plates_and_beams_pass_inactive_red_cubes() {
    let game = game(
        r#"
@
#} r T
        "#,
    );

    assert!(game.laser_trigger_lit(Position { x: 5, y: 1 }));
    assert!(game.pressure_plates_active());
    assert!(!game.red_pressure_plates_active());
    assert_eq!(game.inactive_red_cubes(), [Position { x: 3, y: 1 }]);
}

#[test]
fn projectiles_pass_through_inactive_red_cubes() {
    let fired = game(
        r#"
#@ r #
######
        "#,
    )
    .step(GameInput::Shoot)
    .step(GameInput::Right)
    .step(GameInput::Wait);

    assert_eq!(
        fired.cubes(),
        [Cube {
            position: Position { x: 4, y: 0 },
            source: CubeSource::Player,
        }]
    );
    assert_eq!(fired.inactive_red_cubes(), [Position { x: 3, y: 0 }]);
}

#[test]
fn red_cube_appearing_in_a_tile_left_this_update_falls_one_tile() {
    // The first push presses the plate while the blue cube copies the push onto
    // the red cube's tile. The second push keeps the plate pressed (the player
    // replaces their cube on it) and moves the blue cube off, but the red cube
    // can appear only once that move ends, so it falls one tile, not two.
    let initial = game(
        r#"
############
#br  @O    #
## ####R####
#          #
#          #
############
        "#,
    );
    let blocked = initial.step(GameInput::Right);
    assert!(blocked.red_pressure_plates_active());
    assert_eq!(blocked.inactive_red_cubes(), [Position { x: 2, y: 1 }]);

    let spawned = blocked.step(GameInput::Right);
    assert!(spawned.cubes().contains(&red(2, 2)));
    assert!(spawned.step(GameInput::Wait).cubes().contains(&red(2, 4)));
}

#[test]
fn red_cube_appearing_when_its_plate_is_pressed_falls_two_tiles() {
    let initial = game(
        r#"
############
# r   @O   #
## ####R####
#          #
#          #
############
        "#,
    );
    let spawned = initial.step(GameInput::Right);
    assert!(spawned.cubes().contains(&red(2, 3)));
}
