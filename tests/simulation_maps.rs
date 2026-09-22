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
fn falling_below_the_level_wraps_to_the_top() {
    let initial = GameState::from_ascii(&level(
        r#"
  @


t
"#,
    ))
    .unwrap();
    let wrapped = initial.step(GameInput::Wait).step(GameInput::Wait);
    assert_eq!(wrapped.player().position, Position { x: 2, y: 0 });
    assert!(wrapped.player().position.y < wrapped.level().height() as isize);
}

#[test]
fn jumping_above_the_level_does_not_wrap_or_repeat_the_map() {
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
        jumping.level().tile_at(Position { x: 0, y: -1 }),
        Tile::Empty
    );
    assert_eq!(
        jumping.level().tile_at(Position { x: 0, y: 2 }),
        Tile::Empty
    );
    assert_eq!(jumping.step(GameInput::Wait).step(GameInput::Wait), initial);
}

#[test]
fn falling_cubes_wrap_to_the_top() {
    let initial = GameState::from_ascii("@ \n  \n C").unwrap();
    let fallen = initial.step(GameInput::Wait);
    assert_eq!(fallen.cubes()[0].position, Position { x: 1, y: 1 });
}
