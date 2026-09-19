use magicube_solver::{
    GameInput, GameSettings, GameState, GameStatus, SolveOptions, SolveOutcome, solve,
};

fn assert_solver_matches_saved_solution(level: usize, map: &str, inputs: &str) {
    let initial = GameState::from_ascii(map).unwrap();
    let saved_inputs: Vec<_> = inputs.split_whitespace().map(parse_input).collect();
    let replay = |inputs: &[GameInput]| {
        inputs
            .iter()
            .fold(initial.clone(), |state, &input| state.step(input))
    };
    assert_eq!(
        replay(&saved_inputs).status(),
        GameStatus::Won,
        "level {level}: saved solution must win"
    );

    let result = solve(&initial, SolveOptions::default());
    let SolveOutcome::Solved(found_inputs) = &result.outcome else {
        panic!("level {level}: solver failed: {result:?}");
    };
    assert_eq!(
        replay(found_inputs).status(),
        GameStatus::Won,
        "level {level}: solver solution must win"
    );
    assert!(
        found_inputs.len() <= saved_inputs.len(),
        "level {level}: solver took {} inputs, saved solution took {}",
        found_inputs.len(),
        saved_inputs.len(),
    );
    eprintln!(
        "Level {level}: solver {} inputs, saved {} inputs; {:?}",
        found_inputs.len(),
        saved_inputs.len(),
        result.stats,
    );
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
fn solves_bundled_level_1_with_no_longer_solution() {
    assert_solver_matches_saved_solution(
        1,
        include_str!("../data/level-manual-labels/1.txt"),
        "right right jump right shoot right wait wait right right jump
         right jump right shoot right wait wait right jump right shoot
         left wait wait left left",
    );
}

#[test]
fn solves_bundled_level_2_with_no_longer_solution() {
    assert_solver_matches_saved_solution(
        2,
        include_str!("../data/level-manual-labels/2.txt"),
        "left left left shoot left wait wait jump left jump left jump
         right right right right jump right right jump right
         shoot left wait wait left jump left left jump left left
         right right right left right right right right
         jump right jump right right shoot right wait wait jump right
         right shoot right wait wait",
    );
}

#[test]
fn solves_bundled_level_3_with_no_longer_solution() {
    assert_solver_matches_saved_solution(
        3,
        include_str!("../data/level-manual-labels/3.txt"),
        "jump right right right right right right jump right
         right left left right shoot left wait wait jump left shoot
         right wait wait left left left right right right
         right jump right shoot left wait wait left jump left jump left
         left left left shoot left wait wait jump left left jump
         left shoot right wait wait right right jump right",
    );
}

#[test]
fn solves_bundled_level_4_with_no_longer_solution() {
    assert_solver_matches_saved_solution(
        4,
        include_str!("../data/level-manual-labels/4.txt"),
        "right right right jump right right jump right right shoot left wait wait
         jump left left left right right shoot right wait wait left right shoot
         left wait wait jump left jump left jump left left right right right right
         shoot right wait wait left right shoot left wait wait left left left left right
         right right",
    );
}

#[test]
fn solves_bundled_level_5_with_no_longer_solution() {
    assert_solver_matches_saved_solution(
        5,
        include_str!("../data/level-manual-labels/5.txt"),
        "jump right right right right right left left left left shoot left wait wait left left left jump left shoot right wait wait right right right
         right right right right right jump right shoot left wait wait jump left
         jump left jump left left left right right right right right shoot
         right wait wait right left left left left left left left left jump left
         shoot left wait wait jump left jump left shoot left wait wait",
    );
}

#[test]
fn bundled_level_6_requires_legacy_shot_timing() {
    let map = include_str!("../data/level-manual-labels/6.txt");
    let standard = GameState::from_ascii(map).unwrap();
    assert_eq!(
        solve(&standard, SolveOptions::default()).outcome,
        SolveOutcome::Unsolvable
    );

    let legacy = GameState::from_ascii_with_settings(
        map,
        GameSettings {
            shot_recovery_updates: 1,
            projectile_moves_on_firing_update: true,
            ..GameSettings::default()
        },
    )
    .unwrap();
    let saved: Vec<_> = "right jump right right shoot right wait right jump right right left
         left left jump left left jump left shoot right wait right jump right
         right left right shoot left wait left left left"
        .split_whitespace()
        .map(parse_input)
        .collect();
    let won = saved
        .iter()
        .fold(legacy.clone(), |state, &input| state.step(input));
    assert_eq!(won.status(), GameStatus::Won);
    let SolveOutcome::Solved(found) = solve(&legacy, SolveOptions::default()).outcome else {
        panic!("legacy level 6 rules should remain solvable");
    };
    assert!(found.len() <= saved.len());
}

#[test]
fn solves_bundled_level_7_with_no_longer_solution() {
    // Recorded with both airborne options disabled in
    // level-7-bundled-1789841583627-0.json (84 inputs under legacy timing).
    assert_solver_matches_saved_solution(
        7,
        include_str!("../data/level-manual-labels/7.txt"),
        "jump right right right jump right right left right shoot left wait wait
         jump left left left right right left left left right right right
         right right right left left left left left left jump left jump
         right right right right right right right left left left right right
         right jump right shoot left wait wait left jump left jump left left
         left left left shoot right wait wait jump left jump right right right
         right right right right right jump right right jump right shoot right wait wait",
    );
}
