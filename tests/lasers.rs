mod support;

use magicube_solver::{
    GameInput::{Jump, Left, Right, Shoot, Wait},
    GameState, GameStatus, LaserBeam, LaserDirection, Position, Tile,
};
use support::{assert_level_eq, level};

fn game(drawing: &str) -> GameState {
    GameState::from_ascii(&level(drawing)).unwrap()
}

#[test]
fn parses_and_renders_emitters_and_triggers_without_beams() {
    let game = game(
        r#"
#########
#{ ^ v }#
#@  T   #
#########
        "#,
    );

    for (x, direction) in [
        (1, LaserDirection::Left),
        (3, LaserDirection::Up),
        (5, LaserDirection::Down),
        (7, LaserDirection::Right),
    ] {
        assert_eq!(
            game.level().tile_at(Position { x, y: 1 }),
            Tile::LaserEmitter(direction)
        );
        assert!(game.is_solid(Position { x, y: 1 }));
    }
    assert_eq!(
        game.level().tile_at(Position { x: 4, y: 2 }),
        Tile::LaserTrigger
    );
    assert!(game.is_solid(Position { x: 4, y: 2 }));
    assert_eq!(
        game.laser_beam_at(Position { x: 5, y: 2 }),
        Some(LaserBeam::Vertical)
    );
    assert_level_eq(
        &game.to_ascii(),
        r#"
#########
#{ ^ v }#
#@  T   #
#########
"#,
    );
}

#[test]
fn beams_stop_at_solid_tiles_and_opaque_cubes_but_pass_glass_torches_and_skulls() {
    let game = game(
        r#"
############
#}  gtS C  #
#@         #
############
        "#,
    );
    let beam = |x| game.laser_beam_at(Position { x, y: 1 });

    for x in 2..=6 {
        assert_eq!(beam(x), Some(LaserBeam::Horizontal), "x = {x}");
    }
    assert_eq!(beam(7), Some(LaserBeam::Horizontal));
    assert_eq!(beam(8), None);
    assert_eq!(beam(9), None);
}

#[test]
fn crossing_beams_are_reported_together() {
    let game = game(
        r#"
#####
# v #
#}  #
#@  #
#####
        "#,
    );

    assert_eq!(
        game.laser_beam_at(Position { x: 2, y: 2 }),
        Some(LaserBeam::Crossing)
    );
}

#[test]
fn walking_into_a_beam_kills_the_player() {
    let initial = game(
        r#"
######
#v   #
#  @ #
######
        "#,
    );

    let safe = initial.step(Left);
    assert_eq!(safe.status(), GameStatus::Playing);
    let dead = safe.step(Left);
    assert_eq!(dead.status(), GameStatus::GameOver);
    assert_eq!(dead.player().position, Position { x: 1, y: 2 });
    assert_eq!(dead.step(Right), dead);
}

#[test]
fn jumping_into_a_beam_kills_the_player() {
    let dead = game(
        r#"
#####
#   #
#}  #
#  @#
#####
        "#,
    )
    .step(Jump);

    assert_eq!(dead.status(), GameStatus::GameOver);
}

#[test]
fn falling_through_a_beam_kills_the_player_even_between_gravity_substeps() {
    let initial = game(
        r#"
#####
#  @#
#   #
#}  #
#   #
#   #
#####
        "#,
    );

    // The first update falls from row 1 through row 3 without stopping in the beam.
    let dead = initial.step(Wait).step(Wait);
    assert_eq!(dead.status(), GameStatus::GameOver);
    assert_eq!(dead.player().position, Position { x: 3, y: 3 });
}

#[test]
fn a_cube_shields_the_player_until_it_falls_away() {
    let initial = game(
        r#"
#######
#}  C@#
### ###
#     #
#######
        "#,
    );
    assert_eq!(initial.step(Wait).status(), GameStatus::Playing);

    // Pushing the cube over the gap lets it fall out of the beam.
    let dead = initial.step(Left);
    assert_eq!(dead.status(), GameStatus::GameOver);
}

#[test]
fn the_player_cube_can_block_a_beam() {
    let initial = game(
        r#"
######
# v  #
##  @#
######
        "#,
    );
    assert_eq!(
        initial.laser_beam_at(Position { x: 2, y: 2 }),
        Some(LaserBeam::Vertical)
    );

    let blocked = initial.step(Shoot).step(Left).step(Wait);
    assert_eq!(blocked.laser_beam_at(Position { x: 2, y: 2 }), None);
    let walked = blocked.step(Left);
    assert_eq!(walked.status(), GameStatus::Playing);
    assert_level_eq(
        &walked.to_ascii(),
        r#"
######
# v  #
##O@ #
######
"#,
    );
}

#[test]
fn shooting_away_the_shielding_player_cube_kills_the_player() {
    let dead = game(
        r#"
#######
#}O@  #
#######
        "#,
    )
    .step(Shoot)
    .step(Right);

    assert_eq!(dead.status(), GameStatus::GameOver);
}

#[test]
fn a_lit_trigger_closes_gates_like_a_pressure_plate() {
    let initial = game(
        r#"
#########
#   D  @#
#########
#}   T  #
#########
        "#,
    );
    let gate = Position { x: 4, y: 1 };

    assert!(initial.laser_trigger_lit(Position { x: 5, y: 3 }));
    assert!(initial.pressure_plates_active());
    assert!(initial.is_solid(gate));
    let blocked = initial.step(Left).step(Left).step(Left);
    assert_eq!(blocked.player().position, Position { x: 5, y: 1 });
}

#[test]
fn blocking_the_trigger_beam_opens_gates() {
    let initial = game(
        r#"
#########
#}  C T #
##### ###
#  D   @#
#########
        "#,
    );
    let gate = Position { x: 3, y: 3 };
    assert!(!initial.laser_trigger_lit(Position { x: 6, y: 1 }));
    assert!(!initial.is_solid(gate));

    let lit =
        GameState::from_ascii("#########\n#}    T #\n##### ###\n#  D   @#\n#########").unwrap();
    assert!(lit.laser_trigger_lit(Position { x: 6, y: 1 }));
    assert!(lit.is_solid(gate));
}

#[test]
fn glass_cubes_do_not_shield_the_player_or_the_trigger() {
    let glass = game(
        r#"
#########
#} g  T #
#@      #
#########
        "#,
    );

    assert!(glass.laser_trigger_lit(Position { x: 6, y: 1 }));
    assert_eq!(
        glass.laser_beam_at(Position { x: 3, y: 1 }),
        Some(LaserBeam::Horizontal)
    );
}

#[test]
fn gates_closed_by_pressure_plates_block_beams() {
    let initial = game(
        r#"
#########
#}  D T #
#@      #
##P######
        "#,
    );
    let trigger = Position { x: 6, y: 1 };

    assert!(initial.laser_trigger_lit(trigger));
    let pressed = initial.step(Right);
    assert!(!pressed.laser_trigger_lit(trigger));
    assert_eq!(pressed.laser_beam_at(Position { x: 5, y: 1 }), None);
    assert!(pressed.pressure_plates_active());
}

#[test]
fn beams_continue_beyond_open_map_edges() {
    let initial = game(
        r#"
   }
@
###
        "#,
    );
    assert_eq!(
        initial.laser_beam_at(Position { x: 100, y: 0 }),
        Some(LaserBeam::Horizontal)
    );
    assert_eq!(initial.laser_beam_at(Position { x: 2, y: 0 }), None);
}

#[test]
fn stacks_fall_through_gates_together_after_a_trigger_closes_them() {
    // The player's cube blocks the beam, opening the gates under both map
    // cubes. Relighting the trigger cannot close a gate before the body
    // riding on top follows the cube below it.
    let initial = game(
        r#"
   #######
   #     #
   #     #
####    O#
#   @ T  {
#   C   C#
#DDDDDDDD####
#           #
#  t   t    #
#        #G##
#############
        "#,
    );

    let fallen = initial.step(Wait).step(Wait).step(Wait).step(Wait);
    assert_eq!(fallen.status(), GameStatus::Playing);
    assert_level_eq(
        &fallen.to_ascii(),
        r#"
   #######
   #     #
   #     #
####     #
#     T  {
#        #
#DDDDDDDD####
#           #
#  t@  tO   #
#   C   C#G##
#############
"#,
    );
}

#[test]
fn a_cube_falling_alongside_the_player_shields_it_from_a_beam() {
    let initial = game(
        r#"
######
#  C@#
#}   #
#    #
######
        "#,
    );

    // Both bodies fall through the beam row together; the cube keeps shielding
    // the player during the substep where the player crosses the beam.
    let landed = initial.step(Wait);
    assert_eq!(landed.status(), GameStatus::Playing);
    assert_eq!(landed.player().position, Position { x: 4, y: 3 });
}
