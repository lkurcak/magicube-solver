mod support;

use magicube_solver::{
    GameInput, GameState, GameStatus, ParseLevelError, Position, SolveOptions, SolveOutcome, Tile,
    solve,
};
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
fn a_top_row_pedestal_has_a_goal_position_on_the_bottom_row() {
    let game = GameState::from_ascii("@G\n# ").unwrap();
    assert!(game.level().is_goal(Position { x: 1, y: 1 }));
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
fn jumping_above_the_level_wraps_to_the_bottom() {
    let initial = GameState::from_ascii("@\n#\n ").unwrap();
    let jumping = initial.step(GameInput::Jump);
    assert_eq!(jumping.player().position, Position { x: 0, y: 2 });
}

#[test]
fn jumping_from_the_top_row_is_blocked_by_bottom_row_terrain() {
    let initial = GameState::from_ascii("@ \n# \n##").unwrap();
    let jumping = initial.step(GameInput::Jump);
    assert_eq!(jumping.player().position, Position { x: 0, y: 0 });
}

#[test]
fn walking_and_pushing_past_a_side_edge_wraps_to_the_opposite_column() {
    let initial = GameState::from_ascii(&level(
        r#"
C@  
####
"#,
    ))
    .unwrap();
    let pushed = initial.step(GameInput::Left);
    assert_eq!(pushed.player().position, Position { x: 0, y: 0 });
    assert_eq!(pushed.cubes()[0].position, Position { x: 3, y: 0 });
    let walked = pushed.step(GameInput::Left);
    assert_eq!(walked.player().position, Position { x: 3, y: 0 });
    assert_eq!(walked.cubes()[0].position, Position { x: 2, y: 0 });
}

#[test]
fn projectiles_wrap_past_a_side_edge() {
    let initial = GameState::from_ascii(&level(
        r#"
 #@  
#####
"#,
    ))
    .unwrap();
    let fired = initial.step(GameInput::Shoot).step(GameInput::Right);
    let landed = fired.step(GameInput::Wait).step(GameInput::Wait);
    assert_eq!(landed.projectile(), None);
    assert_eq!(landed.cubes()[0].position, Position { x: 0, y: 0 });
}

#[test]
fn custom_level_1_is_solved_through_side_and_bottom_wrapping() {
    let initial = GameState::from_ascii(include_str!("../data/custom-levels/1.txt")).unwrap();
    use GameInput::{Left, Right, Shoot, Wait};
    let step = |state: GameState, inputs: &[GameInput]| {
        inputs.iter().fold(state, |state, &input| state.step(input))
    };
    // Walk off the right edge into the bottom-left room.
    let state = step(initial.clone(), &[Right; 6]);
    assert_eq!(state.player().position, Position { x: 0, y: 5 });
    // Fall through the floor gap and land in the top-left room.
    let state = step(state, &[Right, Right, Left]);
    assert_eq!(state.player().position, Position { x: 2, y: 1 });
    // Walk off the left edge and drop into the room right of the goal.
    let state = step(state, &[Left; 7]);
    assert_eq!(state.player().position, Position { x: 6, y: 3 });
    // The shot wraps around the right edge and stops against the goal's wall.
    let state = step(state, &[Shoot, Right, Wait, Left]);
    assert_eq!(state.status(), GameStatus::Won);

    let result = solve(&initial, SolveOptions::default());
    let SolveOutcome::Solved(inputs) = result.outcome else {
        panic!("expected a solution, got {result:?}");
    };
    assert_eq!(inputs.len(), 20);
}

#[test]
fn falling_cubes_wrap_to_the_top() {
    let initial = GameState::from_ascii("@ \n  \n C").unwrap();
    let fallen = initial.step(GameInput::Wait);
    assert_eq!(fallen.cubes()[0].position, Position { x: 1, y: 1 });
}
