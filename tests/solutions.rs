use magicube_solver::{GameInput, GameState, GameStatus};

fn assert_solution_wins(map: &str, inputs: &str) {
    let mut game = GameState::from_ascii(map).unwrap();
    for input in inputs.split_whitespace().map(parse_input) {
        game = game.step(input);
    }
    assert_eq!(game.status(), GameStatus::Won);
}

fn parse_input(input: &str) -> GameInput {
    match input {
        "left" => GameInput::Left,
        "right" => GameInput::Right,
        "jump" => GameInput::Jump,
        "shoot" => GameInput::Shoot,
        "wait" => GameInput::Wait,
        other => panic!("unknown saved input: {other}"),
    }
}

#[test]
fn saved_solution_wins_level_1() {
    assert_solution_wins(
        include_str!("../data/level-manual-labels/1.txt"),
        "right right jump right shoot right wait right right jump
         right jump right shoot right wait right jump right shoot
         left wait left left",
    );
}

#[test]
fn saved_solution_wins_level_2() {
    assert_solution_wins(
        include_str!("../data/level-manual-labels/2.txt"),
        "left left left shoot left wait jump left jump left jump
         right right right right jump right right jump right
         shoot left wait left jump left left jump left left
         right right right left right right right right
         jump right jump right right shoot right wait jump right
         right shoot right",
    );
}

#[test]
fn saved_solution_wins_level_3() {
    assert_solution_wins(
        include_str!("../data/level-manual-labels/3.txt"),
        "jump right right right right right right jump right
         right left left right shoot left wait jump left shoot
         right wait left left left right right right
         right jump right shoot left wait left jump left jump left
         left left left shoot left wait jump left left jump
         left shoot right wait right right jump right",
    );
}
