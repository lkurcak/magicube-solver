use magicube_solver::GameInput::{Jump, Left, Right, Shoot, Wait};
use magicube_solver::{
    GameInput, GameSettings, GameState, GameStatus, PlayerMode, SolveOptions, SolveOutcome,
    SolveStats, solve,
};

const ALL_INPUTS: [GameInput; 5] = [Left, Right, Jump, Shoot, Wait];

fn replay(initial: &GameState, inputs: &[GameInput]) -> GameState {
    inputs.iter().fold(initial.clone(), |state, &input| {
        if state.player().mode == PlayerMode::Recovering {
            assert_eq!(input, Wait, "recovery must be recorded as wait");
        }
        assert_eq!(state.status(), GameStatus::Playing);
        state.step(input)
    })
}

fn solution(initial: &GameState) -> Vec<GameInput> {
    let before = initial.clone();
    let result = solve(initial, SolveOptions::default());
    assert_eq!(*initial, before, "search changed the initial state");
    let SolveOutcome::Solved(inputs) = result.outcome else {
        panic!("expected a solution, got {result:?}");
    };
    assert_eq!(replay(initial, &inputs).status(), GameStatus::Won);
    inputs
}

// Independent exhaustive enumeration: no visited keys or action reductions.
fn can_win_within(state: &GameState, remaining: usize) -> bool {
    state.status() == GameStatus::Won
        || (remaining > 0
            && ALL_INPUTS
                .iter()
                .any(|&input| can_win_within(&state.step(input), remaining - 1)))
}

#[test]
fn tiny_solutions_are_shortest_and_replay_exactly() {
    for (map, expected) in [
        ("######\n#@OG #\n######", vec![Right]),
        ("#####\n#@G##\n#####", vec![Shoot, Right, Wait]),
        (
            "#######\n#     #\n#@ G###\n#######",
            vec![Shoot, Right, Wait],
        ),
        (
            "######\n#  G##\n#@####\n######",
            vec![Jump, Right, Shoot, Right, Wait],
        ),
        (
            "#########\n#DG P@  #\n#########",
            vec![Left, Shoot, Left, Wait],
        ),
    ] {
        let initial = GameState::from_ascii(map).unwrap();
        let inputs = solution(&initial);
        assert_eq!(inputs, expected, "{map}");
        assert!(!can_win_within(&initial, inputs.len() - 1), "{map}");
        assert_eq!(solution(&initial), inputs, "search must be deterministic");
    }
}

#[test]
fn standard_search_shoots_only_when_grounded_and_fun_rules_are_explicit() {
    let map = "######\n#  G##\n#@####\n######";
    let standard = GameState::from_ascii(map).unwrap();
    let mut state = standard.clone();
    for input in solution(&standard) {
        if state.player().mode == PlayerMode::Aiming && matches!(input, Left | Right) {
            assert!(state.is_grounded());
        }
        state = state.step(input);
    }
    let fun = GameState::from_ascii_with_settings(
        "#####\n# G##\n#@###\n#####",
        GameSettings {
            allow_airborne_shooting: true,
            ..GameSettings::default()
        },
    )
    .unwrap();
    assert_eq!(solution(&fun), vec![Jump, Shoot, Right, Wait]);
}

#[test]
fn standard_search_cannot_push_in_midair_but_explicit_fun_rules_can() {
    let map = "#######\n#@OG###\n# ### #\n#######";
    let standard = GameState::from_ascii(map).unwrap();
    assert!(!standard.settings().allow_airborne_pushing);
    assert_eq!(
        solve(&standard, SolveOptions::default()).outcome,
        SolveOutcome::Unsolvable
    );
    let fun = GameState::from_ascii_with_settings(
        map,
        GameSettings {
            allow_airborne_pushing: true,
            ..GameSettings::default()
        },
    )
    .unwrap();
    assert_ne!(standard, fun);
    assert_eq!(solution(&fun), vec![Right]);
}

#[test]
fn starts_while_aiming_or_recovering_and_wins_during_recovery() {
    let initial = GameState::from_ascii("########\n#      #\n#@  G###\n########").unwrap();
    let aiming = initial.step(Shoot);
    let recovering = aiming.step(Right);
    assert_eq!(recovering.player().mode, PlayerMode::Recovering);
    assert_eq!(solution(&aiming), vec![Right, Wait, Wait]);
    assert_eq!(solution(&recovering), vec![Wait, Wait]);
    assert!(!can_win_within(&aiming, 2));

    let blocked_aim = GameState::from_ascii("######\n#@OG #\n######")
        .unwrap()
        .step(Shoot);
    assert_eq!(solution(&blocked_aim), vec![Shoot, Right]);
}

#[test]
fn terminal_starts_need_no_search_even_with_a_zero_cap() {
    let won = GameState::from_ascii("#####\n#@G##\n#####")
        .unwrap()
        .step(Shoot)
        .step(Right)
        .step(Wait);
    let dead = GameState::from_ascii("#####\n# C #\n# @ #\n#####")
        .unwrap()
        .step(Wait);
    assert_eq!(dead.status(), GameStatus::GameOver);
    for (initial, outcome) in [
        (won, SolveOutcome::Solved(vec![])),
        (dead, SolveOutcome::Unsolvable),
    ] {
        let result = solve(
            &initial,
            SolveOptions {
                max_states: Some(0),
            },
        );
        assert_eq!(result.outcome, outcome);
        assert_eq!(result.stats, SolveStats::default());
    }
}

#[test]
fn exhausts_cycles_and_blocked_actions_in_a_finite_unsolvable_map() {
    let initial = GameState::from_ascii("#####\n#@#G#\n#####").unwrap();
    let result = solve(&initial, SolveOptions { max_states: None });
    assert_eq!(result.outcome, SolveOutcome::Unsolvable);
    assert_eq!(
        result.stats,
        SolveStats {
            discovered_states: 2,
            expanded_states: 2,
        }
    );
    // Exactly filling the cap is allowed if no further distinct state is found.
    assert_eq!(
        solve(
            &initial,
            SolveOptions {
                max_states: Some(2),
            },
        ),
        result
    );
}

#[test]
fn discards_fatal_successors_without_spending_the_state_budget_on_them() {
    let initial = GameState::from_ascii("#####\n##C##\n##@##\n##G##\n#####").unwrap();
    assert_eq!(initial.step(Wait).status(), GameStatus::GameOver);
    let result = solve(
        &initial,
        SolveOptions {
            max_states: Some(2),
        },
    );
    assert_eq!(result.outcome, SolveOutcome::Unsolvable);
    // This unsupported start cannot enter aiming under the default rules.
    assert_eq!(result.stats.discovered_states, 1);
    assert_eq!(result.stats.expanded_states, 1);
}

#[test]
fn state_caps_include_the_start_and_the_winning_state() {
    assert_eq!(SolveOptions::default().max_states, Some(1_000_000));
    let initial = GameState::from_ascii("######\n#@OG #\n######").unwrap();
    for cap in [0, 1] {
        let result = solve(
            &initial,
            SolveOptions {
                max_states: Some(cap),
            },
        );
        assert_eq!(result.outcome, SolveOutcome::StateLimitReached);
        assert_eq!(result.stats.discovered_states, cap);
        assert_eq!(result.stats.expanded_states, cap);
    }
    let result = solve(
        &initial,
        SolveOptions {
            max_states: Some(2),
        },
    );
    assert_eq!(result.outcome, SolveOutcome::Solved(vec![Right]));
    assert_eq!(result.stats.discovered_states, 2);
    assert_eq!(result.stats.expanded_states, 1);
}

#[test]
fn open_maps_reach_the_limit_instead_of_claiming_unsolvability() {
    let initial = GameState::from_ascii("@ G").unwrap();
    let result = solve(
        &initial,
        SolveOptions {
            max_states: Some(100),
        },
    );
    assert_eq!(result.outcome, SolveOutcome::StateLimitReached);
    assert_eq!(result.stats.discovered_states, 100);
}

#[test]
fn can_solve_from_above_the_map_without_clipping_coordinates() {
    let initial = GameState::from_ascii("@OG\n###").unwrap().step(Jump);
    assert_eq!(initial.player().position.y, -1);
    let inputs = solution(&initial);
    assert_eq!(inputs.len(), 3);
    assert!(!can_win_within(&initial, 2));
}
