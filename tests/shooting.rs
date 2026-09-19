use magicube_solver::GameInput::{Jump, Left, Right, Shoot, Wait};
use magicube_solver::{CubeSource, GameState, GameStatus, ParseLevelError, PlayerMode, Position};

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
        .step(Wait);
    assert!(flying.projectile().is_some());
    assert_eq!(flying.step(Shoot).step(Shoot), flying);
}

#[test]
fn projectile_checks_each_of_its_two_tiles_and_spawns_in_the_last_empty_one() {
    // Hits on either substep, both immediately and on a later frame.
    for (row, direction, projectile_x, cube_x) in [
        ("#@ #   #", Right, None, 2),
        ("#@  #  #", Right, Some(3), 3),
        ("#@   # #", Right, Some(3), 4),
        ("#@tS # #", Right, Some(3), 4),
        ("#@St # #", Right, Some(3), 4),
        ("# #tS @#", Left, Some(4), 3),
    ] {
        let initial = GameState::from_ascii(&format!("########\n{row}\n########")).unwrap();
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
        let initial = GameState::from_ascii(&format!("######\n# @{obstacle} #\n######")).unwrap();
        let aiming = initial.step(Shoot);
        assert_eq!(aiming.step(Right), aiming, "obstacle {obstacle}");
    }
    let initial = GameState::from_ascii("########\n# @# O #\n########").unwrap();
    let aiming = initial.step(Shoot);
    assert_eq!(aiming.step(Right), aiming);
    let fired = aiming.step(Left);
    assert_eq!(fired.cubes().len(), 1);
    assert_eq!(fired.cubes()[0].position, Position { x: 1, y: 1 });
    assert_eq!(fired.player().mode, PlayerMode::Recovering);
}

#[test]
fn recovery_ignores_actions_for_exactly_one_update_but_moves_the_projectile() {
    let fired = GameState::from_ascii("############\n#          #\n# @        #\n############")
        .unwrap()
        .step(Shoot)
        .step(Right);
    assert_eq!(fired.player().mode, PlayerMode::Recovering);
    let recovered = fired.step(Wait);
    assert_eq!(recovered.player().mode, PlayerMode::Normal);
    assert_eq!(recovered.player().position, fired.player().position);
    assert_eq!(
        recovered.projectile().unwrap().position,
        Position { x: 6, y: 2 }
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
fn recovery_spends_airtime_and_applies_player_and_cube_gravity() {
    let initial = GameState::from_ascii(
        "############\n#        C #\n#          #\n#          #\n#          #\n#          #\n#          #\n# @        #\n############",
    ).unwrap();
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
fn a_new_shot_removes_only_the_player_cube_and_new_cubes_fall_immediately() {
    let initial = GameState::from_ascii("#########\n#   C   #\n#@ O    #\n#########").unwrap();
    let fired = initial.step(Shoot).step(Right);
    assert_eq!(fired.cubes().len(), 1);
    assert_eq!(fired.cubes()[0].source, CubeSource::Map);
    assert_eq!(
        fired.projectile().unwrap().position,
        Position { x: 3, y: 2 }
    );
    let hit_cube = fired.step(Wait);
    assert_eq!(hit_cube.cubes().len(), 2);
    assert_eq!(hit_cube.symbol_at(Position { x: 3, y: 2 }), 'O');
    assert_eq!(hit_cube.symbol_at(Position { x: 4, y: 2 }), 'C');

    let initial = GameState::from_ascii("#######\n#@ #  #\n##    #\n#     #\n#######").unwrap();
    let fired = initial.step(Shoot).step(Right);
    assert_eq!(fired.cubes()[0].position, Position { x: 2, y: 3 });
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
