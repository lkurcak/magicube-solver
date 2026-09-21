mod support;

use magicube_solver::GameInput::{Jump, Left, Right, Shoot, Wait};
use magicube_solver::{GameState, GameStatus, Position, Tile};
use support::{assert_level_eq, level};

fn game(drawing: &str) -> GameState {
    GameState::from_ascii(&level(drawing)).unwrap()
}

#[test]
fn projectiles_pass_through_goals_on_every_substep() {
    for row in ["#@G    #", "#@ G   #", "#@  G  #"] {
        let initial = game(&format!(
            r#"
########
{row}
########
"#
        ));
        let fired = initial.step(Shoot).step(Right);
        assert_eq!(
            fired.projectile().unwrap().position,
            Position { x: 4, y: 1 }
        );
        assert!(fired.cubes().is_empty());
        assert_eq!(fired.status(), GameStatus::Playing);
    }
}

#[test]
fn spawning_the_player_cube_on_a_goal_wins_and_stops_play() {
    let initial = game(
        r#"
#####
#@G##
#####
"#,
    );
    let goal = Position { x: 2, y: 1 };
    assert!(!initial.is_solid(goal));
    let won = initial.step(Shoot).step(Right);
    assert_eq!(won.status(), GameStatus::Won);
    assert_eq!(won.cubes()[0].position, goal);
    assert_eq!(won.level().tile_at(goal), Tile::Goal);
    assert_level_eq(
        &won.to_ascii(),
        r#"
#####
#@O##
#####
"#,
    );
    for input in [Jump, Left, Right, Shoot, Wait] {
        assert_eq!(won.step(input), won);
    }
}

#[test]
fn spawning_the_player_cube_on_an_open_gate_goal_wins() {
    let initial = game(
        r#"
#####
#@X##
#####
"#,
    );
    let goal = Position { x: 2, y: 1 };
    let won = initial.step(Shoot).step(Right);

    assert_eq!(won.status(), GameStatus::Won);
    assert_eq!(won.cubes()[0].position, goal);
    assert_eq!(won.level().tile_at(goal), Tile::GateGoal);
    assert_level_eq(
        &won.to_ascii(),
        r#"
#####
#@O##
#####
"#,
    );
}

#[test]
fn player_and_map_cube_do_not_win_on_a_gate_goal() {
    let walking = game(
        r#"
#####
#@X #
#####
"#,
    )
    .step(Right);
    assert_eq!(walking.player().position, Position { x: 2, y: 1 });
    assert_eq!(walking.status(), GameStatus::Playing);

    let pushed = game(
        r#"
######
#@CX #
######
"#,
    )
    .step(Right);
    assert_eq!(pushed.cubes()[0].position, Position { x: 3, y: 1 });
    assert_eq!(pushed.status(), GameStatus::Playing);
}

#[test]
fn pushing_onto_a_goal_wins_only_with_the_player_cube() {
    for (symbol, status) in [('O', GameStatus::Won), ('C', GameStatus::Playing)] {
        let initial = game(&format!(
            r#"
######
#@{symbol}G #
######
"#
        ));
        let pushed = initial.step(Right);
        assert_eq!(pushed.cubes()[0].position, Position { x: 3, y: 1 });
        assert_eq!(pushed.status(), status);
    }
    let walking = game(
        r#"
######
#@G  #
######
"#,
    )
    .step(Right);
    assert_eq!(walking.player().position, Position { x: 2, y: 1 });
    assert_eq!(walking.status(), GameStatus::Playing);
}

#[test]
fn falling_onto_a_goal_wins_only_with_the_player_cube() {
    for (symbol, status) in [('O', GameStatus::Won), ('C', GameStatus::Playing)] {
        let initial = game(&format!(
            r#"
#######
#  {symbol}  #
#     #
#@ G  #
#######
"#
        ));
        let fallen = initial.step(Wait);
        assert_eq!(fallen.cubes()[0].position, Position { x: 3, y: 3 });
        assert_eq!(fallen.status(), status);
    }
}
