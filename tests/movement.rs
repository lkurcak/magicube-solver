mod support;

use GameInput::{Jump, Left, Right, Wait};
use magicube_solver::{GameInput, GameState, Position, Tile};
use support::{assert_level_eq, level};

fn game(drawing: &str) -> GameState {
    GameState::from_ascii(&level(drawing)).unwrap()
}

#[test]
fn jump_grants_exactly_two_air_inputs() {
    let initial = game(
        r#"
            #########
            #       #
            #       #
            # @     #
            #########
        "#,
    );
    let jumping = initial.step(Jump);
    assert_eq!(jumping.player().position, Position { x: 2, y: 2 });
    assert_eq!(jumping.player().air_inputs_remaining, 2);
    assert!(!jumping.is_grounded());

    let first = jumping.step(Right);
    assert_eq!(first.player().position, Position { x: 3, y: 2 });
    assert_eq!(first.player().air_inputs_remaining, 1);

    let second = first.step(Right);
    assert_eq!(second.player().position, Position { x: 4, y: 3 });
    assert_eq!(second.player().air_inputs_remaining, 0);
    assert!(second.is_grounded());

    // Branching a solver search must not mutate the original state or copy its map.
    assert_eq!(initial.player().position, Position { x: 2, y: 3 });
    assert_eq!(initial.player().air_inputs_remaining, 0);
    assert!(std::ptr::eq(initial.level(), second.level()));
    assert_eq!(
        initial.step(Left).player().position,
        Position { x: 1, y: 3 }
    );
}

#[test]
fn every_pair_of_air_inputs_including_waits_and_reversals_is_supported() {
    let initial = game(
        r#"
            #########
            #       #
            #   @   #
            #########
        "#,
    );
    let inputs = [(Left, -1), (Right, 1), (Wait, 0)];
    for (first, dx1) in inputs {
        for (second, dx2) in inputs {
            let result = initial.step(Jump).step(first).step(second);
            assert_eq!(
                result.player().position,
                Position {
                    x: 4 + dx1 + dx2,
                    y: 2
                },
                "air inputs: {first:?}, {second:?}",
            );
            assert!(result.is_grounded());
            assert_eq!(result.player().air_inputs_remaining, 0);
        }
    }
}

#[test]
fn long_jump_left() {
    let initial = game(
        r#"
            #######
            #     #
            ### @ #
            ### ###
            #######
        "#,
    );
    let result = initial.step(Jump).step(Left).step(Left);
    assert_level_eq(
        &result.to_ascii(),
        r#"
            #######
            # @   #
            ###   #
            ### ###
            #######
        "#,
    );
    assert!(result.is_grounded());
}

#[test]
fn blocked_air_movement_still_spends_airtime() {
    let initial = game(
        r#"
            #####
            #  ##
            # @##
            #####
        "#,
    );
    let jumping = initial.step(Jump);
    let blocked = jumping.step(Right);
    assert_eq!(blocked.player().position, jumping.player().position);
    assert_eq!(blocked.player().air_inputs_remaining, 1);
    assert_eq!(blocked.step(Right), initial);
}

#[test]
fn airborne_jump_attempts_cannot_extend_airtime() {
    let initial = game(
        r#"
            #####
            #   #
            #   #
            # @ #
            #####
        "#,
    );
    let jumping = initial.step(Jump);
    let attempted = jumping.step(Jump);
    assert_eq!(attempted.player().position, jumping.player().position);
    assert_eq!(attempted.player().air_inputs_remaining, 1);
    assert_eq!(attempted.step(Jump), initial);
}

#[test]
fn landing_on_a_ledge_ends_airtime_and_allows_another_jump() {
    let initial = game(
        r#"
            #######
            #     #
            #     #
            # @#  #
            #######
        "#,
    );
    let landed = initial.step(Jump).step(Right);
    assert!(landed.is_grounded());
    assert_eq!(landed.player().air_inputs_remaining, 0);
    assert_eq!(landed.step(Jump).player().position, Position { x: 3, y: 1 });

    // Walking off that ledge cannot reuse the previous jump's unused airtime.
    let walked_off = landed.step(Right);
    assert_eq!(walked_off.player().position, Position { x: 4, y: 3 });
    assert!(walked_off.is_grounded());
}

#[test]
fn walking_off_either_side_of_a_ledge_falls_two_tiles_immediately() {
    let initial = game(
        r#"
            #######
            #  @  #
            #  #  #
            #     #
            #     #
            #######
        "#,
    );
    for (input, x) in [(Left, 2), (Right, 4)] {
        let falling = initial.step(input);
        assert_eq!(falling.player().position, Position { x, y: 3 });
        assert!(!falling.is_grounded());
        let landed = falling.step(Wait);
        assert_eq!(landed.player().position, Position { x, y: 4 });
        assert!(landed.is_grounded());
    }
}

#[test]
fn falling_checks_each_tile_so_it_cannot_skip_a_platform() {
    let initial = game(
        r#"
            #######
            # @   #
            # #   #
            #  #  #
            #     #
            #######
        "#,
    );
    let landed = initial.step(Right);
    assert_eq!(landed.player().position, Position { x: 3, y: 2 });
    assert!(landed.is_grounded());
    assert_eq!(landed.step(Wait), landed);
}

#[test]
fn after_airtime_expires_falling_continues_two_tiles_per_update() {
    let initial = game(
        r#"
            ########
            #      #
            # @    #
            # #    #
            #      #
            #      #
            #      #
            ########
        "#,
    );
    let falling = initial.step(Jump).step(Right).step(Right);
    assert_eq!(falling.player().position, Position { x: 4, y: 3 });
    assert_eq!(falling.player().air_inputs_remaining, 0);
    let falling = falling.step(Wait);
    assert_eq!(falling.player().position, Position { x: 4, y: 5 });
    let landed = falling.step(Wait);
    assert_eq!(landed.player().position, Position { x: 4, y: 6 });
    assert!(landed.is_grounded());
}

#[test]
fn unsupported_player_falls_on_every_input_including_invalid_jumps() {
    let initial = game(
        r#"
            #######
            #  @  #
            #     #
            #     #
            #     #
            #######
        "#,
    );
    for (input, x) in [(Left, 2), (Right, 4), (Jump, 3), (Wait, 3)] {
        let falling = initial.step(input);
        assert_eq!(falling.player().position, Position { x, y: 3 }, "{input:?}");
        assert_eq!(falling.player().air_inputs_remaining, 0);
    }
}

#[test]
fn blocked_horizontal_movement_does_not_stop_falling() {
    let initial = game(
        r#"
            #####
            ##@##
            #   #
            #   #
            #####
        "#,
    );
    for input in [Left, Right] {
        let landed = initial.step(input);
        assert_eq!(landed.player().position, Position { x: 2, y: 3 });
        assert!(landed.is_grounded());
    }
}

#[test]
fn blocked_jump_does_not_grant_airtime() {
    let initial = game(
        r#"
            #####
            # # #
            # @ #
            # # #
            #   #
            #####
        "#,
    );
    let blocked = initial.step(Jump);
    assert_eq!(blocked, initial);
    assert_eq!(
        blocked.step(Right).player().position,
        Position { x: 3, y: 4 }
    );
}

#[test]
fn non_wall_tiles_are_passable_and_preserved_under_the_player() {
    let initial = game("#######\n#@GSt?#\n#######");
    let mut current = initial;
    for (x, tile) in [
        (2, Tile::Goal),
        (3, Tile::Skull),
        (4, Tile::Torch),
        (5, Tile::Unknown),
    ] {
        current = current.step(Right);
        assert_eq!(current.player().position, Position { x, y: 1 });
        assert_eq!(current.level().tile_at(current.player().position), tile);
    }
    assert_level_eq(&current.to_ascii(), "#######\n# GSt@#\n#######");
    current = current.step(Left);
    assert_level_eq(&current.to_ascii(), "#######\n# GS@?#\n#######");
}

#[test]
fn non_wall_tiles_do_not_supply_ground_support() {
    for symbol in ['G', 'S', 't', '?'] {
        let initial = game(&format!("#####\n# @ #\n# {symbol} #\n#   #\n#####"));
        assert!(!initial.is_grounded());
        let landed = initial.step(Jump);
        assert_eq!(landed.player().position, Position { x: 2, y: 3 });
        assert!(landed.is_grounded());
    }
}

#[test]
fn waiting_advances_airtime() {
    let initial = game("#####\n#   #\n# @ #\n#####");
    assert_eq!(initial.step(Wait), initial);
    let waiting = initial.step(Jump).step(Wait);
    assert_eq!(waiting.player().air_inputs_remaining, 1);
    assert_eq!(waiting.step(Wait), initial);
}

#[test]
fn airtime_is_part_of_state_identity_even_when_the_picture_is_identical() {
    let initial = game("#####\n#   #\n# @ #\n#####");
    let jumping = initial.step(Jump);
    let waiting = jumping.step(Wait);
    assert_eq!(jumping.to_ascii(), waiting.to_ascii());
    let states = std::collections::HashSet::from([jumping, waiting]);
    assert_eq!(states.len(), 2);
}

#[test]
fn right() {
    let game = GameState::from_ascii(&level(
        r#"
            #####
            # @ #
            #####
        "#,
    ))
    .unwrap();

    let game = game.step(Right);

    assert_level_eq(
        &game.to_ascii(),
        r#"
            #####
            #  @#
            #####
        "#,
    );
}

#[test]
fn left() {
    let game = GameState::from_ascii(&level(
        r#"
            #####
            # @ #
            #####
        "#,
    ))
    .unwrap();

    let game = game.step(Left);

    assert_level_eq(
        &game.to_ascii(),
        r#"
            #####
            #@  #
            #####
        "#,
    );
}

#[test]
fn move_up_right() {
    let game = GameState::from_ascii(&level(
        r#"
            #####
            #   #
            # @##
            #####
        "#,
    ))
    .unwrap();

    let game = game.step(Jump).step(Right);

    assert_level_eq(
        &game.to_ascii(),
        r#"
            #####
            #  @#
            #  ##
            #####
        "#,
    );
}

#[test]
fn long_jump_right() {
    let game = GameState::from_ascii(&level(
        r#"
            #####
            #   #
            #@ ##
            ## ##
            #####
        "#,
    ))
    .unwrap();

    let game = game.step(Jump).step(Right).step(Right);

    assert_level_eq(
        &game.to_ascii(),
        r#"
            #####
            #  @#
            #  ##
            ## ##
            #####
        "#,
    );
}

#[test]
fn move_up_left() {
    let game = GameState::from_ascii(&level(
        r#"
            #####
            #   #
            ##@ #
            #####
        "#,
    ))
    .unwrap();

    let game = game.step(Jump).step(Left);

    assert_level_eq(
        &game.to_ascii(),
        r#"
            #####
            #@  #
            ##  #
            #####
        "#,
    );
}

#[test]
fn cant_right() {
    let game = GameState::from_ascii(&level(
        r#"
            #####
            # @##
            #####
        "#,
    ))
    .unwrap();

    let game = game.step(Right);

    assert_level_eq(
        &game.to_ascii(),
        r#"
            #####
            # @##
            #####
        "#,
    );
}

#[test]
fn cant_left() {
    let game = GameState::from_ascii(&level(
        r#"
            #####
            ##@ #
            #####
        "#,
    ))
    .unwrap();

    let game = game.step(Left);

    assert_level_eq(
        &game.to_ascii(),
        r#"
            #####
            ##@ #
            #####
        "#,
    );
}

#[test]
fn cant_move_up_right_1() {
    let game = GameState::from_ascii(&level(
        r#"
            #####
            #  ##
            # @##
            #####
        "#,
    ))
    .unwrap();

    // The blocked move consumes the first air input; waiting finishes the jump.
    let game = game.step(Jump).step(Right).step(Wait);

    assert_level_eq(
        &game.to_ascii(),
        r#"
            #####
            #  ##
            # @##
            #####
        "#,
    );
}

#[test]
fn cant_move_up() {
    let game = GameState::from_ascii(&level(
        r#"
            #####
            # # #
            # @ #
            #####
        "#,
    ))
    .unwrap();

    let game = game.step(Jump);

    assert_level_eq(
        &game.to_ascii(),
        r#"
            #####
            # # #
            # @ #
            #####
        "#,
    );
}

#[test]
fn cant_move_up_left_1() {
    let game = GameState::from_ascii(&level(
        r#"
            #####
            ##  #
            ##@ #
            #####
        "#,
    ))
    .unwrap();

    // The blocked move consumes the first air input; waiting finishes the jump.
    let game = game.step(Jump).step(Left).step(Wait);

    assert_level_eq(
        &game.to_ascii(),
        r#"
            #####
            ##  #
            ##@ #
            #####
        "#,
    );
}
