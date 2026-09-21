mod support;

use magicube_solver::{GameInput, GameState, ParseLevelError, Position, Tile};
use support::{assert_level_eq, level};

#[test]
fn loads_existing_importer_maps_without_losing_tiles_or_indentation() {
    for map in [
        include_str!("../data/level-manual-labels/1.txt"),
        include_str!("../data/level-manual-labels/2.txt"),
    ] {
        let game = GameState::from_ascii(map).unwrap();
        assert_eq!(game.to_ascii(), map.trim_end_matches('\n'));
        assert_eq!(game.level().height(), map.lines().count());
        assert_eq!(
            game.level().width(),
            map.lines().map(str::len).max().unwrap()
        );
        assert!(game.is_grounded());
        assert_eq!(game.level().tile_at(game.player().position), Tile::Empty);
    }
}

#[test]
fn short_rows_are_empty_and_crlf_line_endings_are_accepted() {
    let game = GameState::from_ascii("#####\r\n#@\r\n#####\r\n").unwrap();
    assert_eq!(game.level().width(), 5);
    assert_eq!(game.level().height(), 3);
    assert_eq!(game.level().tile_at(Position { x: 2, y: 1 }), Tile::Empty);
    assert_level_eq(
        &game.step(GameInput::Right).to_ascii(),
        r#"
#####
# @
#####
"#,
    );
}

#[test]
fn gates_above_pedestals_round_trip_and_expose_both_capabilities() {
    let game = GameState::from_ascii(&level(
        r#"
#####
#@D #
##G##
"#,
    ))
    .unwrap();
    let position = Position { x: 2, y: 1 };
    let tile = game.level().tile_at(position);

    assert_eq!(tile, Tile::Gate);
    assert!(tile.is_gate());
    assert!(game.level().is_goal(position));
    assert_level_eq(
        &game.to_ascii(),
        r#"
#####
#@D #
##G##
"#,
    );
}

#[test]
fn a_top_row_pedestal_has_a_goal_position_above_the_map() {
    let game = GameState::from_ascii("@G\n##").unwrap();
    assert!(game.level().is_goal(Position { x: 1, y: -1 }));
    assert!(!game.level().is_goal(Position { x: 1, y: 0 }));
}

#[test]
fn rejects_empty_maps_missing_or_multiple_players_and_invalid_symbols() {
    for (map, error) in [
        (String::new(), ParseLevelError::EmptyMap),
        ("\n\r\n".to_owned(), ParseLevelError::EmptyMap),
        (
            level(
                r#"
###
# #
###
"#,
            ),
            ParseLevelError::MissingPlayer,
        ),
        (
            level(
                r#"
@@
"#,
            ),
            ParseLevelError::MultiplePlayers,
        ),
        (
            level(
                r#"
@!
"#,
            ),
            ParseLevelError::InvalidTile {
                position: Position { x: 1, y: 0 },
                symbol: '!',
            },
        ),
        (
            level(
                r#"
@X
"#,
            ),
            ParseLevelError::InvalidTile {
                position: Position { x: 1, y: 0 },
                symbol: 'X',
            },
        ),
    ] {
        assert_eq!(GameState::from_ascii(&map), Err(error), "{map:?}");
    }
}

#[test]
fn outside_the_map_is_empty_and_does_not_grant_a_jump() {
    let initial = GameState::from_ascii(&level(
        r#"
@
"#,
    ))
    .unwrap();
    assert!(!initial.is_grounded());
    for (input, x) in [
        (GameInput::Left, -1),
        (GameInput::Right, 1),
        (GameInput::Jump, 0),
    ] {
        let falling = initial.step(input);
        assert_eq!(falling.player().position, Position { x, y: 2 });
        assert_eq!(falling.player().air_inputs_remaining, 0);
        assert!(!falling.is_grounded());
        assert_eq!(falling.to_ascii(), "");
        assert_eq!(
            falling.step(GameInput::Wait).player().position,
            Position { x, y: 4 }
        );
    }
}

#[test]
fn jump_can_rise_above_the_drawing_without_an_implicit_ceiling() {
    let initial = GameState::from_ascii(&level(
        r#"
@
#
"#,
    ))
    .unwrap();
    let jumping = initial.step(GameInput::Jump);
    assert_eq!(jumping.player().position, Position { x: 0, y: -1 });
    assert_eq!(
        jumping.level().tile_at(jumping.player().position),
        Tile::Empty
    );
    assert_eq!(jumping.step(GameInput::Wait).step(GameInput::Wait), initial);
}
