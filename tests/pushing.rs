mod support;

use magicube_solver::GameInput::{Jump, Left, Right, Wait};
use magicube_solver::{GameSettings, GameState};
use support::{assert_level_eq, level};

fn game(drawing: &str) -> GameState {
    GameState::from_ascii(&level(drawing)).unwrap()
}

#[test]
fn pushes_single_cubes_and_mixed_chains_in_both_directions() {
    for (before, input, after) in [
        ("#@C     #", Right, "# @C    #"),
        ("#     O@#", Left, "#    O@ #"),
        ("#@COCC  #", Right, "# @COCC #"),
        ("#  CCOC@#", Left, "# CCOC@ #"),
    ] {
        let map = format!(
            r#"
#########
{before}
#########
"#
        );
        let initial = game(&map);
        let pushed = initial.step(input);
        assert_level_eq(
            &pushed.to_ascii(),
            &format!(
                r#"
#########
{after}
#########
"#
            ),
        );
        assert_level_eq(&initial.to_ascii(), &map);
        assert_eq!(pushed.cubes().len(), initial.cubes().len());
    }
}

#[test]
fn wall_at_end_of_chain_blocks_the_entire_push() {
    for (row, input) in [("#@COC#", Right), ("#COC@#", Left)] {
        let initial = game(&format!(
            r#"
######
{row}
######
"#
        ));
        assert_eq!(initial.step(input), initial);
    }
}

#[test]
fn skull_at_end_of_chain_blocks_the_entire_push() {
    for (row, input) in [("#@COS #", Right), ("# SCO@#", Left)] {
        let initial = game(&format!(
            r#"
#######
{row}
#######
"#
        ));
        assert_eq!(initial.step(input), initial);
    }
}

#[test]
fn skulls_stop_falling_cubes() {
    let initial = game(
        r#"
#######
# C   #
#     #
# S   #
#  @  #
#######
"#,
    );
    let fallen = initial.step(Wait);

    assert_eq!(fallen.cubes()[0].position.y, 2);
    assert_eq!(fallen.step(Wait).cubes()[0].position.y, 2);
}

#[test]
fn cube_pushed_off_a_ledge_falls_in_the_same_update() {
    let initial = game(
        r#"
#######
#@CO  #
####  #
#     #
#######
"#,
    );
    let pushed = initial.step(Right);
    assert_level_eq(
        &pushed.to_ascii(),
        r#"
#######
# @C  #
####  #
#   O #
#######
"#,
    );
}

#[test]
fn airborne_pushes_require_opt_in_during_both_jumps_and_falls() {
    for (map, input, dx, jump) in [
        (
            r#"
########
#@CO   #
#      #
#      #
########
"#,
            Right,
            1,
            false,
        ),
        (
            r#"
########
#   OC@#
#      #
#      #
########
"#,
            Left,
            -1,
            false,
        ),
        (
            r#"
########
# CO   #
#@##   #
########
"#,
            Right,
            1,
            true,
        ),
        (
            r#"
########
#   OC #
#   ##@#
########
"#,
            Left,
            -1,
            true,
        ),
    ] {
        for allow_airborne_pushing in [false, true] {
            // Shooting and pushing can be enabled independently.
            for allow_airborne_shooting in [false, true] {
                let initial = GameState::from_ascii_with_settings(
                    &level(map),
                    GameSettings {
                        allow_airborne_shooting,
                        allow_airborne_pushing,
                        ..GameSettings::default()
                    },
                )
                .unwrap();
                let airborne = if jump { initial.step(Jump) } else { initial };
                assert!(!airborne.is_grounded());
                let next = airborne.step(input);
                let moved = if allow_airborne_pushing { dx } else { 0 };
                assert_eq!(
                    next.player().position.x,
                    airborne.player().position.x + moved
                );
                for (before, after) in airborne.cubes().iter().zip(next.cubes()) {
                    assert_eq!(after.position.x, before.position.x + moved);
                }
                if jump {
                    // A successful push lands on the ledge under the cube;
                    // a blocked push instead spends one of the two air inputs.
                    assert_eq!(
                        next.player().air_inputs_remaining,
                        if allow_airborne_pushing { 0 } else { 1 }
                    );
                    assert_eq!(next.step(Wait).player().air_inputs_remaining, 0);
                } else {
                    assert_eq!(next.player().position.y, airborne.player().position.y + 2);
                }
                assert_eq!(next.settings(), airborne.settings());
            }
        }
    }
}

#[test]
fn standing_on_either_cube_kind_supplies_support_for_pushing() {
    for cube in ['C', 'O'] {
        let initial = game(&format!(
            r#"
#######
# @C  #
# {cube}#  #
#######
"#
        ));
        assert!(initial.is_grounded());
        assert_eq!(initial.step(Right).player().position.x, 3);
    }
}

#[test]
fn airborne_setting_does_not_allow_pushing_chains_through_walls() {
    for (row, input) in [("#@COC#", Right), ("#COC@#", Left)] {
        let initial = GameState::from_ascii_with_settings(
            &level(&format!(
                r#"
######
{row}
#    #
######
"#
            )),
            GameSettings {
                allow_airborne_pushing: true,
                ..GameSettings::default()
            },
        )
        .unwrap();
        assert!(!initial.is_grounded());
        let blocked = initial.step(input);
        assert_eq!(blocked.player().position.x, initial.player().position.x);
        for (before, after) in initial.cubes().iter().zip(blocked.cubes()) {
            assert_eq!(after.position.x, before.position.x);
        }
    }
}
