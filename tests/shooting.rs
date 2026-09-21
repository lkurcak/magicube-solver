mod support;

use magicube_solver::GameInput::{Jump, Left, Right, Shoot, Wait};
use magicube_solver::{
    CubeSource, GameSettings, GameState, GameStatus, ParseLevelError, PlayerMode, Position,
};
use support::{assert_cube_layout_eq, level};

fn game(drawing: &str) -> GameState {
    GameState::from_ascii(&level(drawing)).unwrap()
}

#[test]
fn aiming_and_cancel_pause_the_whole_world() {
    let initial = game(
        r#"
##########
#   C    #
#        #
#@   O   #
##########
"#,
    );
    assert_eq!(
        initial.to_ascii(),
        level(
            r#"
##########
#   C    #
#        #
#@   O   #
##########
"#
        )
    );
    let aiming = initial.step(Shoot);
    assert_eq!(aiming.player().mode, PlayerMode::Aiming);
    assert_eq!(aiming.to_ascii(), initial.to_ascii());
    assert_eq!(aiming.step(Wait), aiming);
    assert_eq!(aiming.step(Jump), aiming);
    assert_eq!(aiming.step(Shoot), initial);

    let flying = game(
        r#"
##########
#@       #
##########
"#,
    )
    .step(Shoot)
    .step(Right)
    .step(Wait);
    assert!(flying.projectile().is_some());
    assert_eq!(flying.step(Shoot).step(Shoot), flying);
}

#[test]
fn projectile_checks_each_traversed_tile_and_spawns_in_the_last_empty_one() {
    // Hits on any substep of the first or a later movement update.
    for (row, direction, projectile_x, cube_x) in [
        ("#@ #   #", Right, None, 2),
        ("#@  #  #", Right, None, 3),
        ("#@   # #", Right, Some(4), 4),
        ("#@tS # #", Right, Some(4), 4),
        ("#@St # #", Right, Some(4), 4),
        ("# #tS @#", Left, Some(3), 3),
    ] {
        let initial = game(&format!(
            r#"
########
{row}
########
"#
        ));
        let mut shot = initial.step(Shoot).step(direction);
        assert_eq!(shot.player().position, initial.player().position);
        assert_eq!(shot.projectile().map(|p| p.position.x), projectile_x);
        if projectile_x.is_some() {
            shot = shot.step(Wait);
        }
        assert_eq!(shot.projectile(), None);
        assert_eq!(shot.cubes().len(), 1);
        assert_eq!(shot.cubes()[0].position, Position { x: cube_x, y: 1 });
        assert_eq!(shot.cubes()[0].source, CubeSource::Player);
        assert_eq!(shot.symbol_at(shot.cubes()[0].position), 'O');
    }
}

#[test]
fn obstacles_block_shots_and_blocked_shots_preserve_the_previous_cube() {
    for obstacle in ['#', 'C', 'O', '?'] {
        let initial = game(&format!(
            r#"
######
# @{obstacle} #
######
"#
        ));
        let aiming = initial.step(Shoot);
        assert_eq!(aiming.step(Right), aiming, "obstacle {obstacle}");
    }
    let initial = game(
        r#"
########
# @# O #
########
"#,
    );
    let aiming = initial.step(Shoot);
    assert_eq!(aiming.step(Right), aiming);
    let fired = aiming.step(Left);
    assert_eq!(fired.cubes().len(), 1);
    assert_eq!(fired.cubes()[0].position, Position { x: 1, y: 1 });
    assert_eq!(fired.player().mode, PlayerMode::Recovering);
}

#[test]
fn projectile_destroyed_on_a_skull_does_not_spawn_the_player_cube() {
    let fired = game(
        r#"
######
# @S##
######
"#,
    )
    .step(Shoot)
    .step(Right);

    assert!(fired.projectile().is_none());
    assert!(fired.cubes().is_empty());
    assert_eq!(fired.player().mode, PlayerMode::Recovering);
}

#[test]
fn recovery_ignores_actions_for_exactly_one_update_but_moves_the_projectile() {
    let fired = game(
        r#"
############
#          #
# @        #
############
"#,
    )
    .step(Shoot)
    .step(Right);
    assert_eq!(fired.player().mode, PlayerMode::Recovering);
    let recovered = fired.step(Wait);
    assert_eq!(recovered.player().mode, PlayerMode::Normal);
    assert_eq!(recovered.player().position, fired.player().position);
    assert_eq!(
        recovered.projectile().unwrap().position,
        Position { x: 8, y: 2 }
    );
    for input in [Left, Right, Jump, Shoot, Wait] {
        assert_eq!(fired.step(input), recovered, "recovery input {input:?}");
    }
    assert_eq!(
        recovered.step(Right).player().position,
        Position { x: 3, y: 2 }
    );
    assert_eq!(
        recovered.step(Jump).player().position,
        Position { x: 2, y: 1 }
    );
    assert_eq!(recovered.step(Shoot).player().mode, PlayerMode::Aiming);
}

#[test]
fn projectile_distance_is_configurable_and_defaults_to_three_tiles() {
    let map = level(
        r#"
##########
#@       #
##########
"#,
    );
    let default = GameState::from_ascii(&map).unwrap().step(Shoot).step(Right);
    assert_eq!(default.settings().projectile_tiles_per_update, 3);
    assert_eq!(
        default.projectile().unwrap().position,
        Position { x: 4, y: 1 }
    );

    let configured = GameState::from_ascii_with_settings(
        &map,
        GameSettings {
            projectile_tiles_per_update: 1,
            ..GameSettings::default()
        },
    )
    .unwrap()
    .step(Shoot)
    .step(Right);
    assert_eq!(
        configured.projectile().unwrap().position,
        Position { x: 2, y: 1 }
    );
}

#[test]
fn removing_the_player_cube_settles_its_stack_before_projectile_travel() {
    let initial = game(
        r#"
#######
#  C  #
#@ O# #
#######
"#,
    );
    let fired = initial.step(Shoot).step(Right);

    assert!(fired.projectile().is_none());
    assert_eq!(fired.symbol_at(Position { x: 2, y: 2 }), 'O');
    assert_eq!(fired.symbol_at(Position { x: 3, y: 2 }), 'C');
}

#[test]
fn falling_cubes_block_projectiles_at_the_start_middle_and_end_of_their_sweep() {
    for (map, projectile_y, cube_y) in [
        (
            r#"
#######
#     #
#@ C# #
###O  #
#######
"#,
            2,
            3,
        ),
        (
            r#"
#######
#  C  #
#@ O# #
###   #
#######
"#,
            2,
            3,
        ),
        (
            r#"
#######
#  C  #
#  O  #
#@  # #
#######
"#,
            3,
            3,
        ),
    ] {
        let fired = game(map).step(Shoot).step(Right);
        assert!(fired.projectile().is_none());
        assert_eq!(
            fired
                .cubes()
                .iter()
                .find(|cube| cube.source == CubeSource::Map)
                .unwrap()
                .position,
            Position { x: 3, y: cube_y },
            "{map}"
        );
        assert_eq!(
            fired.symbol_at(Position {
                x: 2,
                y: projectile_y
            }),
            'O'
        );
    }

    let misses = game(
        r#"
########
#    C #
#@   O##
#####  #
########
"#,
    )
    .step(Shoot)
    .step(Right);
    assert_eq!(
        misses.projectile().unwrap().position,
        Position { x: 4, y: 2 }
    );
    assert_eq!(misses.symbol_at(Position { x: 5, y: 3 }), 'C');
}

#[test]
fn recovery_spends_airtime_and_applies_player_and_cube_gravity() {
    let initial = GameState::from_ascii_with_settings(
        &level(
            r#"
############
#        C #
#          #
#          #
#          #
#          #
#          #
# @        #
############
"#,
        ),
        GameSettings {
            allow_airborne_shooting: true,
            ..GameSettings::default()
        },
    )
    .unwrap();
    let fired = initial.step(Jump).step(Shoot).step(Right);
    assert_eq!(fired.player().air_inputs_remaining, 1);
    assert_eq!(fired.player().position, Position { x: 2, y: 6 });
    assert_eq!(fired.cubes()[0].position, Position { x: 9, y: 5 });

    let recovered = fired.step(Wait);
    assert_eq!(recovered.player().air_inputs_remaining, 0);
    assert_eq!(recovered.player().position, Position { x: 2, y: 7 });
    assert_eq!(recovered.cubes()[0].position, Position { x: 9, y: 7 });
    assert!(recovered.is_grounded());
}

#[test]
fn shooting_requires_support_by_default_and_fun_mode_allows_jumps_and_falls() {
    let map = level(
        r#"
##########
#        #
#        #
#        #
#@       #
##########
"#,
    );
    for allow_airborne_shooting in [false, true] {
        let initial = GameState::from_ascii_with_settings(
            &map,
            GameSettings {
                allow_airborne_shooting,
                ..GameSettings::default()
            },
        )
        .unwrap();
        assert!(initial.can_shoot());
        let jumping = initial.step(Jump);
        assert!(!jumping.is_grounded());
        assert!(jumping.player().air_inputs_remaining > 0);
        let falling = GameState::from_ascii_with_settings(
            &level(
                r#"
@



#
"#,
            ),
            initial.settings(),
        )
        .unwrap();
        assert_eq!(falling.player().air_inputs_remaining, 0);
        for airborne in [jumping, falling] {
            assert_eq!(airborne.can_shoot(), allow_airborne_shooting);
            if allow_airborne_shooting {
                let aiming = airborne.step(Shoot);
                assert_eq!(aiming.player().mode, PlayerMode::Aiming);
                assert_eq!(aiming.step(Shoot), airborne);
                let fired = aiming.step(Right);
                assert!(fired.projectile().is_some());
                assert_eq!(fired.player().mode, PlayerMode::Recovering);
                assert_eq!(fired.settings(), initial.settings());
            } else {
                assert_eq!(airborne.step(Shoot), airborne);
            }
        }
    }
    let default = GameState::from_ascii(&map).unwrap();
    assert_eq!(default.settings(), GameSettings::default());
    assert_ne!(
        default,
        GameState::from_ascii_with_settings(
            &map,
            GameSettings {
                allow_airborne_shooting: true,
                ..GameSettings::default()
            }
        )
        .unwrap()
    );
}

#[test]
fn standing_on_either_cube_kind_allows_a_grounded_shot() {
    for cube in ['C', 'O'] {
        let initial = game(&format!(
            r#"
########
# @    #
# {cube}    #
########
"#
        ));
        assert!(initial.is_grounded());
        let fired = initial.step(Shoot).step(Right);
        assert_eq!(fired.player().mode, PlayerMode::Recovering);
        assert!(fired.projectile().is_some());
    }
}

#[test]
fn a_new_shot_removes_only_the_player_cube_and_new_cubes_fall_immediately() {
    let initial = game(
        r#"
##########
#    C   #
#@ O     #
##########
"#,
    );
    let fired = initial.step(Shoot).step(Right);
    assert_eq!(fired.cubes().len(), 1);
    assert_eq!(fired.cubes()[0].source, CubeSource::Map);
    assert_eq!(
        fired.projectile().unwrap().position,
        Position { x: 4, y: 2 }
    );
    let hit_cube = fired.step(Wait);
    assert_eq!(hit_cube.cubes().len(), 2);
    assert_eq!(hit_cube.symbol_at(Position { x: 4, y: 2 }), 'O');
    assert_eq!(hit_cube.symbol_at(Position { x: 5, y: 2 }), 'C');

    let initial = game(
        r#"
#######
#@ #  #
##    #
#     #
#######
"#,
    );
    let fired = initial.step(Shoot).step(Right);
    assert_eq!(fired.cubes()[0].position, Position { x: 2, y: 3 });
    assert_eq!(
        GameState::from_ascii(&level(
            r#"
@OO
"#,
        )),
        Err(ParseLevelError::MultiplePlayerCubes)
    );
}

#[test]
fn both_cube_kinds_support_jumps() {
    for symbol in ['C', 'O'] {
        let initial = game(&format!(
            r#"
#######
#     #
#     #
# @{symbol}  #
#######
"#
        ));
        let on_cube = initial.step(Jump).step(Right);
        assert_eq!(on_cube.player().position, Position { x: 3, y: 2 });
        assert!(on_cube.is_grounded());
        assert_eq!(
            on_cube.step(Jump).player().position,
            Position { x: 3, y: 1 }
        );
    }
}

#[test]
fn falling_cube_stacks_move_two_tiles_and_stop_at_platforms() {
    let initial = game(
        r#"
#####
# C #
# O #
#   #
#@  #
#####
"#,
    );
    let fallen = initial.step(Wait);
    assert_eq!(fallen.cubes()[0].position, Position { x: 2, y: 3 });
    assert_eq!(fallen.cubes()[1].position, Position { x: 2, y: 4 });
    assert_eq!(fallen.step(Wait), fallen);

    let initial = game(
        r#"
#####
# C #
#   #
# # #
#@  #
#####
"#,
    );
    assert_eq!(
        initial.step(Wait).cubes()[0].position,
        Position { x: 2, y: 2 }
    );
}

#[test]
fn falling_cube_crushes_player_and_game_over_freezes_the_state() {
    let initial = game(
        r#"
#####
# C #
#   #
#   #
# @ #
#####
"#,
    );
    let falling = initial.step(Wait);
    assert_eq!(falling.status(), GameStatus::Playing);
    assert_eq!(falling.cubes()[0].position, Position { x: 2, y: 3 });
    let crushed = falling.step(Wait);
    assert_eq!(crushed.status(), GameStatus::GameOver);
    assert_eq!(crushed.cubes()[0].position, crushed.player().position);
    for input in [Jump, Left, Right, Shoot, Wait] {
        assert_eq!(crushed.step(input), crushed);
    }
}

#[test]
fn shooting_into_falling_cubes_produces_the_expected_layout() {
    let initial = game(
        r#"
######
# C  #
#CO @#
######
"#,
    );
    let expected = game(
        r#"
######
#    #
#CCO@#
######
"#,
    );
    let shooting = initial.step(Shoot);
    let shot = shooting.step(Left);
    assert_cube_layout_eq(shot.cubes(), expected.cubes());
}
