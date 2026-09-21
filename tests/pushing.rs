use magicube_solver::GameInput::{Jump, Left, Right, Wait};
use magicube_solver::{GameSettings, GameState};

#[test]
fn pushes_single_cubes_and_mixed_chains_in_both_directions() {
    for (before, input, after) in [
        ("#@C     #", Right, "# @C    #"),
        ("#     O@#", Left, "#    O@ #"),
        ("#@COCC  #", Right, "# @COCC #"),
        ("#  CCOC@#", Left, "# CCOC@ #"),
    ] {
        let map = format!("#########\n{before}\n#########");
        let initial = GameState::from_ascii(&map).unwrap();
        let pushed = initial.step(input);
        assert_eq!(pushed.to_ascii(), format!("#########\n{after}\n#########"));
        assert_eq!(initial.to_ascii(), map);
        assert_eq!(pushed.cubes().len(), initial.cubes().len());
    }
}

#[test]
fn wall_at_end_of_chain_blocks_the_entire_push() {
    for (row, input) in [("#@COC#", Right), ("#COC@#", Left)] {
        let initial = GameState::from_ascii(&format!("######\n{row}\n######")).unwrap();
        assert_eq!(initial.step(input), initial);
    }
}

#[test]
fn skull_at_end_of_chain_blocks_the_entire_push() {
    for (row, input) in [("#@COS #", Right), ("# SCO@#", Left)] {
        let initial = GameState::from_ascii(&format!("#######\n{row}\n#######")).unwrap();
        assert_eq!(initial.step(input), initial);
    }
}

#[test]
fn skulls_stop_falling_cubes() {
    let initial =
        GameState::from_ascii("#######\n# C   #\n#     #\n# S   #\n#  @  #\n#######").unwrap();
    let fallen = initial.step(Wait);

    assert_eq!(fallen.cubes()[0].position.y, 2);
    assert_eq!(fallen.step(Wait).cubes()[0].position.y, 2);
}

#[test]
fn cube_pushed_off_a_ledge_falls_in_the_same_update() {
    let initial = GameState::from_ascii("#######\n#@CO  #\n####  #\n#     #\n#######").unwrap();
    let pushed = initial.step(Right);
    assert_eq!(
        pushed.to_ascii(),
        "#######\n# @C  #\n####  #\n#   O #\n#######"
    );
}

#[test]
fn airborne_pushes_require_opt_in_during_both_jumps_and_falls() {
    for (map, input, dx, jump) in [
        (
            "########\n#@CO   #\n#      #\n#      #\n########",
            Right,
            1,
            false,
        ),
        (
            "########\n#   OC@#\n#      #\n#      #\n########",
            Left,
            -1,
            false,
        ),
        ("########\n# CO   #\n#@##   #\n########", Right, 1, true),
        ("########\n#   OC #\n#   ##@#\n########", Left, -1, true),
    ] {
        for allow_airborne_pushing in [false, true] {
            // Shooting and pushing can be enabled independently.
            for allow_airborne_shooting in [false, true] {
                let initial = GameState::from_ascii_with_settings(
                    map,
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
        let initial =
            GameState::from_ascii(&format!("#######\n# @C  #\n# {cube}#  #\n#######")).unwrap();
        assert!(initial.is_grounded());
        assert_eq!(initial.step(Right).player().position.x, 3);
    }
}

#[test]
fn airborne_setting_does_not_allow_pushing_chains_through_walls() {
    for (row, input) in [("#@COC#", Right), ("#COC@#", Left)] {
        let initial = GameState::from_ascii_with_settings(
            &format!("######\n{row}\n#    #\n######"),
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
