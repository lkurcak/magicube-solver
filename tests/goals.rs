use magicube_solver::GameInput::{Jump, MoveLeft, MoveRight, Shoot, Wait};
use magicube_solver::{GameState, GameStatus, Position, Tile};

#[test]
fn projectiles_pass_through_goals_on_either_substep() {
    for row in ["#@G    #", "#@ G   #"] {
        let initial = GameState::from_ascii(&format!("########\n{row}\n########")).unwrap();
        let fired = initial.step(Shoot).step(MoveRight);
        assert_eq!(
            fired.projectile().unwrap().position,
            Position { x: 3, y: 1 }
        );
        assert!(fired.cubes().is_empty());
        assert_eq!(fired.status(), GameStatus::Playing);
    }
}

#[test]
fn spawning_the_player_cube_on_a_goal_wins_and_stops_play() {
    let initial = GameState::from_ascii("#####\n#@G##\n#####").unwrap();
    let goal = Position { x: 2, y: 1 };
    assert!(!initial.is_solid(goal));
    let won = initial.step(Shoot).step(MoveRight);
    assert_eq!(won.status(), GameStatus::Won);
    assert_eq!(won.cubes()[0].position, goal);
    assert_eq!(won.level().tile_at(goal), Tile::Goal);
    assert_eq!(won.to_ascii(), "#####\n#@O##\n#####");
    for input in [Jump, MoveLeft, MoveRight, Shoot, Wait] {
        assert_eq!(won.step(input), won);
    }
}

#[test]
fn pushing_onto_a_goal_wins_only_with_the_player_cube() {
    for (symbol, status) in [('O', GameStatus::Won), ('C', GameStatus::Playing)] {
        let initial = GameState::from_ascii(&format!("######\n#@{symbol}G #\n######")).unwrap();
        let pushed = initial.step(MoveRight);
        assert_eq!(pushed.cubes()[0].position, Position { x: 3, y: 1 });
        assert_eq!(pushed.status(), status);
    }
    let walking = GameState::from_ascii("######\n#@G  #\n######")
        .unwrap()
        .step(MoveRight);
    assert_eq!(walking.player().position, Position { x: 2, y: 1 });
    assert_eq!(walking.status(), GameStatus::Playing);
}

#[test]
fn falling_onto_a_goal_wins_only_with_the_player_cube() {
    for (symbol, status) in [('O', GameStatus::Won), ('C', GameStatus::Playing)] {
        let initial = GameState::from_ascii(&format!(
            "#######\n#  {symbol}  #\n#     #\n#@ G  #\n#######"
        ))
        .unwrap();
        let fallen = initial.step(Wait);
        assert_eq!(fallen.cubes()[0].position, Position { x: 3, y: 3 });
        assert_eq!(fallen.status(), status);
    }
}
