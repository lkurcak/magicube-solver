mod support;

use magicube_solver::GameInput::{Left, Right, Shoot, Wait};
use magicube_solver::{CubeSource, GameState, Position};
use support::{assert_level_eq, level};

fn game(drawing: &str) -> GameState {
    GameState::from_ascii(&level(drawing)).unwrap()
}

#[test]
fn parses_and_renders_blue_cubes() {
    let game = game(
        r#"
#####
#@ b#
#####
        "#,
    );

    assert_eq!(game.cubes().len(), 1);
    assert_eq!(game.cubes()[0].source, CubeSource::Blue);
    assert_eq!(game.symbol_at(Position { x: 3, y: 1 }), 'b');
    assert_level_eq(
        &game.to_ascii(),
        r#"
#####
#@ b#
#####
"#,
    );
}

#[test]
fn blue_cubes_float_and_support_the_player() {
    let initial = game(
        r#"
#####
#@  #
##b #
#   #
#####
        "#,
    );

    let waited = initial.step(Wait).step(Wait);
    assert_eq!(waited.to_ascii(), initial.to_ascii());

    // Standing on a floating blue cube counts as grounded.
    let walked = initial.step(Right);
    assert_eq!(walked.player().position, Position { x: 2, y: 1 });
    assert!(walked.is_grounded());
}

#[test]
fn blue_cubes_cannot_be_pushed_without_the_player_cube_behind_them() {
    for map in ["#@b  #", "#@Cb #", "#@bO #", "#@Ob##"] {
        let map = format!("######\n{map}\n######");
        let initial = game(&map);
        assert_level_eq(&initial.step(Right).to_ascii(), &map);
    }
}

#[test]
fn blue_cubes_copy_pushes_of_the_player_cube() {
    let initial = game(
        r#"
########
#  b   #
# b    #
#@O  bb#
########
        "#,
    );

    // The rightmost blue cube is blocked by the wall, and the one behind it
    // by the blocked cube.
    assert_level_eq(
        &initial.step(Right).to_ascii(),
        r#"
########
#   b  #
#  b   #
# @O bb#
########
"#,
    );

    let map = "########\n#  b O@#\n########";
    assert_level_eq(
        &game(map).step(Left).to_ascii(),
        "########\n# b O@ #\n########",
    );
}

#[test]
fn a_line_of_blue_cubes_moves_together() {
    let initial = game(
        r#"
#########
#@O bbb #
#########
        "#,
    );

    assert_level_eq(
        &initial.step(Right).to_ascii(),
        r#"
#########
# @O bbb#
#########
"#,
    );
}

#[test]
fn blue_cubes_copy_every_tile_the_player_cube_falls() {
    let initial = game(
        r#"
#######
#@O b #
### # #
#     #
#     #
#######
        "#,
    );

    // The pushed cube falls two tiles in the same update, and the blue cube
    // copies the push and both falls.
    let pushed = initial.step(Right);
    assert_level_eq(
        &pushed.to_ascii(),
        r#"
#######
# @   #
### # #
#  O b#
#     #
#######
"#,
    );

    assert_level_eq(
        &pushed.step(Wait).to_ascii(),
        r#"
#######
# @   #
### # #
#     #
#  O b#
#######
"#,
    );
}

#[test]
fn blocked_blue_cubes_stay_put_while_others_move() {
    let initial = game(
        r#"
########
#@O  b##
#### b #
########
        "#,
    );

    assert_level_eq(
        &initial.step(Right).to_ascii(),
        r#"
########
# @O b##
####  b#
########
"#,
    );
}

#[test]
fn blue_cubes_do_not_move_into_the_player() {
    let initial = game(
        r#"
########
#b     #
#@O    #
### ####
#      #
########
        "#,
    );

    // The blue cube copies the push, but the player blocks both falls.
    assert_level_eq(
        &initial.step(Right).to_ascii(),
        r#"
########
# b    #
# @    #
### ####
#  O   #
########
"#,
    );
}

#[test]
fn blue_cubes_ignore_other_cubes_and_the_player_moving() {
    let initial = game(
        r#"
#######
#@C  b#
### # #
#######
        "#,
    );

    assert_level_eq(
        &initial.step(Right).step(Right).to_ascii(),
        r#"
#######
#  @ b#
###C# #
#######
"#,
    );
}

#[test]
fn shooting_and_respawning_the_player_cube_does_not_move_blue_cubes() {
    let initial = game(
        r#"
#######
#O@   #
#######
#    b#
#######
        "#,
    );

    let respawned = initial.step(Shoot).step(Right).step(Wait).step(Wait);
    assert_level_eq(
        &respawned.to_ascii(),
        r#"
#######
# @  O#
#######
#    b#
#######
"#,
    );
}

#[test]
fn the_player_cube_carries_blue_cubes_ahead_of_it() {
    for (before, after) in [
        ("#@Ob   #", "# @Ob  #"),
        ("#@ObbC #", "# @ObbC#"),
        ("#@CObC #", "# @CObC#"),
    ] {
        let map = format!("########\n{before}\n########");
        assert_level_eq(
            &game(&map).step(Right).to_ascii(),
            &format!("########\n{after}\n########"),
        );
    }
}

#[test]
fn blue_cubes_see_gates_as_they_were_before_the_player_cube_moved() {
    let initial = game(
        r#"
bD
@O
P###
        "#,
    );

    // Stepping off the plate opens the gate, but only after the blue cube
    // tried to move into it.
    let opened = initial.step(Right);
    assert_level_eq(
        &opened.to_ascii(),
        r#"
bD
 @O
P###
"#,
    );
    assert!(!opened.pressure_plates_active());

    assert_level_eq(
        &opened.step(Right).to_ascii(),
        r#"
 b
  @O
P###
"#,
    );
}

#[test]
fn blue_cubes_push_cubes_ahead_of_them() {
    let initial = game(
        r#"
######
#@O  #
#bC  #
######
        "#,
    );

    assert_level_eq(
        &initial.step(Right).to_ascii(),
        r#"
######
# @O #
# bC #
######
"#,
    );
}

#[test]
fn blue_cubes_stay_put_when_the_cubes_ahead_are_blocked() {
    let initial = game(
        r#"
#####
#@O #
#bC##
#####
        "#,
    );

    assert_level_eq(
        &initial.step(Right).to_ascii(),
        r#"
#####
# @O#
#bC##
#####
"#,
    );
}

#[test]
fn blue_cubes_support_only_while_the_player_cube_is_supported_or_they_are_blocked() {
    // The player cube is in freefall, so the blue cube below the player
    // falls with it and the player cannot shoot from it.
    let falling = game(
        r#"
#######
#@  O #
#b    #
#     #
#     #
#######
        "#,
    );
    assert!(!falling.is_grounded());
    assert!(!falling.can_shoot());
    assert_eq!(falling.step(Shoot).to_ascii(), falling.to_ascii());

    // A blue cube resting on terrain stays put while the player cube falls.
    let blocked = game(
        r#"
#######
#@  O #
#b    #
##    #
#     #
#######
        "#,
    );
    assert!(blocked.is_grounded());
    assert!(blocked.can_shoot());

    // Once the player cube lands, the floating blue cube is stable again.
    let landed = game(
        r#"
#######
#@    #
#b    #
#     #
#   O #
#######
        "#,
    );
    assert!(landed.is_grounded());
}
