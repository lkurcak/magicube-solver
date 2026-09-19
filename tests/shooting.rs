use magicube_solver::GameInput::{Jump, Left, Right, Shoot, Wait};
use magicube_solver::{
    CubeSource, GameSettings, GameState, GameStatus, ParseLevelError, PlayerMode, Position,
};

#[test]
fn aiming_and_cancel_pause_the_whole_world() {
    let initial =
        GameState::from_ascii("##########\n#   C    #\n#        #\n#@   O   #\n##########")
            .unwrap();
    assert_eq!(
        initial.to_ascii(),
        "##########\n#   C    #\n#        #\n#@   O   #\n##########"
    );
    let aiming = initial.step(Shoot);
    assert_eq!(aiming.player().mode, PlayerMode::Aiming);
    assert_eq!(aiming.to_ascii(), initial.to_ascii());
    assert_eq!(aiming.step(Wait), aiming);
    assert_eq!(aiming.step(Jump), aiming);
    assert_eq!(aiming.step(Shoot), initial);

    let flying = GameState::from_ascii("##########\n#@       #\n##########")
        .unwrap()
        .step(Shoot)
        .step(Right)
        .step(Wait)
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
        let initial = GameState::from_ascii(&format!("########\n{row}\n########")).unwrap();
        let fired = initial.step(Shoot).step(direction);
        assert_eq!(
            fired.projectile().unwrap().position,
            initial.player().position
        );
        let mut shot = fired.step(Wait);
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
        let initial = GameState::from_ascii(&format!("######\n# @{obstacle} #\n######")).unwrap();
        let aiming = initial.step(Shoot);
        assert_eq!(aiming.step(Right), aiming, "obstacle {obstacle}");
    }
    let initial = GameState::from_ascii("########\n# @# O #\n########").unwrap();
    let aiming = initial.step(Shoot);
    assert_eq!(aiming.step(Right), aiming);
    let fired = aiming.step(Left);
    assert!(fired.cubes().is_empty());
    assert_eq!(fired.player().mode, PlayerMode::Recovering);
    let landed = fired.step(Wait);
    assert_eq!(landed.cubes().len(), 1);
    assert_eq!(landed.cubes()[0].position, Position { x: 1, y: 1 });
}

#[test]
fn recovery_ignores_actions_for_the_configured_updates_and_moves_the_projectile() {
    let fired = GameState::from_ascii("############\n#          #\n# @        #\n############")
        .unwrap()
        .step(Shoot)
        .step(Right);
    assert_eq!(fired.player().mode, PlayerMode::Recovering);
    assert_eq!(fired.player().recovery_updates_remaining, 2);
    assert_eq!(
        fired.projectile().unwrap().position,
        fired.player().position
    );
    let recovering = fired.step(Wait);
    assert_eq!(recovering.player().mode, PlayerMode::Recovering);
    assert_eq!(recovering.player().recovery_updates_remaining, 1);
    assert_eq!(
        recovering.projectile().unwrap().position,
        Position { x: 5, y: 2 }
    );
    let recovered = recovering.step(Wait);
    assert_eq!(recovered.player().mode, PlayerMode::Normal);
    assert_eq!(recovered.player().recovery_updates_remaining, 0);
    assert_eq!(recovered.player().position, fired.player().position);
    assert_eq!(
        recovered.projectile().unwrap().position,
        Position { x: 8, y: 2 }
    );
    for input in [Left, Right, Jump, Shoot, Wait] {
        assert_eq!(
            fired.step(input),
            recovering,
            "first recovery input {input:?}"
        );
        assert_eq!(
            recovering.step(input),
            recovered,
            "second recovery input {input:?}"
        );
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
    let map = "##########\n#@       #\n##########";
    let default = GameState::from_ascii(map).unwrap().step(Shoot).step(Right);
    assert_eq!(default.settings().projectile_tiles_per_update, 3);
    assert_eq!(
        default.projectile().unwrap().position,
        Position { x: 1, y: 1 }
    );

    let configured = GameState::from_ascii_with_settings(
        map,
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
        Position { x: 1, y: 1 }
    );
    assert_eq!(
        default.step(Wait).projectile().unwrap().position,
        Position { x: 4, y: 1 }
    );
    assert_eq!(
        configured.step(Wait).projectile().unwrap().position,
        Position { x: 2, y: 1 }
    );
}

#[test]
fn removing_the_player_cube_settles_its_stack_before_projectile_travel() {
    let initial = GameState::from_ascii("#######\n#  C  #\n#@ O# #\n#######").unwrap();
    let fired = initial.step(Shoot).step(Right);

    assert_eq!(
        fired.cubes(),
        &[magicube_solver::Cube {
            position: Position { x: 3, y: 2 },
            source: CubeSource::Map,
        }]
    );
    assert_eq!(
        fired.projectile().unwrap().position,
        initial.player().position
    );

    let collided = fired.step(Wait);
    assert!(collided.projectile().is_none());
    assert_eq!(collided.symbol_at(Position { x: 2, y: 2 }), 'O');
    assert_eq!(collided.symbol_at(Position { x: 3, y: 2 }), 'C');
}

#[test]
fn recovery_count_allows_zero_and_legacy_firing_timing_is_configurable() {
    let map = "############\n#@         #\n############";
    for recovery_updates in [0, 1, 3] {
        let settings = GameSettings {
            shot_recovery_updates: recovery_updates,
            ..GameSettings::default()
        };
        let mut state = GameState::from_ascii_with_settings(map, settings)
            .unwrap()
            .step(Shoot)
            .step(Right);
        assert_eq!(state.player().recovery_updates_remaining, recovery_updates);
        assert_eq!(
            state.player().mode,
            if recovery_updates == 0 {
                PlayerMode::Normal
            } else {
                PlayerMode::Recovering
            }
        );
        for remaining in (0..recovery_updates).rev() {
            state = state.step(Wait);
            assert_eq!(state.player().recovery_updates_remaining, remaining);
        }
        assert_eq!(state.player().mode, PlayerMode::Normal);
    }

    let legacy = GameState::from_ascii_with_settings(
        map,
        GameSettings {
            shot_recovery_updates: 1,
            projectile_moves_on_firing_update: true,
            ..GameSettings::default()
        },
    )
    .unwrap()
    .step(Shoot)
    .step(Right);
    assert_eq!(
        legacy.projectile().unwrap().position,
        Position { x: 4, y: 1 }
    );
    assert_eq!(legacy.player().recovery_updates_remaining, 1);
    assert_eq!(legacy.step(Wait).player().mode, PlayerMode::Normal);
}

#[test]
fn recovery_spends_airtime_and_applies_player_and_cube_gravity() {
    let initial = GameState::from_ascii_with_settings(
        "############\n#        C #\n#          #\n#          #\n#          #\n#          #\n#          #\n# @        #\n############",
        GameSettings { allow_airborne_shooting: true, ..GameSettings::default() },
    ).unwrap();
    let fired = initial.step(Jump).step(Shoot).step(Right);
    assert_eq!(fired.player().air_inputs_remaining, 1);
    assert_eq!(fired.player().position, Position { x: 2, y: 6 });
    assert_eq!(fired.cubes()[0].position, Position { x: 9, y: 5 });

    let recovering = fired.step(Wait);
    assert_eq!(recovering.player().mode, PlayerMode::Recovering);
    let recovered = recovering.step(Wait);
    assert_eq!(recovered.player().air_inputs_remaining, 0);
    assert_eq!(recovered.player().position, Position { x: 2, y: 7 });
    assert_eq!(recovered.cubes()[0].position, Position { x: 9, y: 7 });
    assert!(recovered.is_grounded());
}

#[test]
fn shooting_requires_support_by_default_and_fun_mode_allows_jumps_and_falls() {
    let map = "##########\n#        #\n#        #\n#        #\n#@       #\n##########";
    for allow_airborne_shooting in [false, true] {
        let initial = GameState::from_ascii_with_settings(
            map,
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
        let falling =
            GameState::from_ascii_with_settings("@\n \n \n \n#", initial.settings()).unwrap();
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
    let default = GameState::from_ascii(map).unwrap();
    assert_eq!(default.settings(), GameSettings::default());
    assert_ne!(
        default,
        GameState::from_ascii_with_settings(
            map,
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
        let initial =
            GameState::from_ascii(&format!("########\n# @    #\n# {cube}    #\n########")).unwrap();
        assert!(initial.is_grounded());
        let fired = initial.step(Shoot).step(Right);
        assert_eq!(fired.player().mode, PlayerMode::Recovering);
        assert!(fired.projectile().is_some());
    }
}

#[test]
fn a_new_shot_removes_only_the_player_cube_and_new_cubes_fall_immediately() {
    let initial = GameState::from_ascii("##########\n#    C   #\n#@ O     #\n##########").unwrap();
    let fired = initial.step(Shoot).step(Right);
    assert_eq!(fired.cubes().len(), 1);
    assert_eq!(fired.cubes()[0].source, CubeSource::Map);
    assert_eq!(
        fired.projectile().unwrap().position,
        initial.player().position
    );
    let hit_cube = fired.step(Wait).step(Wait);
    assert_eq!(hit_cube.cubes().len(), 2);
    assert_eq!(hit_cube.symbol_at(Position { x: 4, y: 2 }), 'O');
    assert_eq!(hit_cube.symbol_at(Position { x: 5, y: 2 }), 'C');

    let initial = GameState::from_ascii("#######\n#@ #  #\n##    #\n#     #\n#######").unwrap();
    let fired = initial.step(Shoot).step(Right);
    assert!(fired.cubes().is_empty());
    assert_eq!(
        fired.step(Wait).cubes()[0].position,
        Position { x: 2, y: 3 }
    );
    assert_eq!(
        GameState::from_ascii("@OO"),
        Err(ParseLevelError::MultiplePlayerCubes)
    );
}

#[test]
fn both_cube_kinds_support_jumps() {
    for symbol in ['C', 'O'] {
        let initial = GameState::from_ascii(&format!(
            "#######\n#     #\n#     #\n# @{symbol}  #\n#######"
        ))
        .unwrap();
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
    let initial = GameState::from_ascii("#####\n# C #\n# O #\n#   #\n#@  #\n#####").unwrap();
    let fallen = initial.step(Wait);
    assert_eq!(fallen.cubes()[0].position, Position { x: 2, y: 3 });
    assert_eq!(fallen.cubes()[1].position, Position { x: 2, y: 4 });
    assert_eq!(fallen.step(Wait), fallen);

    let initial = GameState::from_ascii("#####\n# C #\n#   #\n# # #\n#@  #\n#####").unwrap();
    assert_eq!(
        initial.step(Wait).cubes()[0].position,
        Position { x: 2, y: 2 }
    );
}

#[test]
fn falling_cube_crushes_player_and_game_over_freezes_the_state() {
    let initial = GameState::from_ascii("#####\n# C #\n#   #\n#   #\n# @ #\n#####").unwrap();
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
