use magicube_solver::GameInput::{MoveLeft, MoveRight};
use magicube_solver::GameState;

#[test]
fn pushes_single_cubes_and_mixed_chains_in_both_directions() {
    for (before, input, after) in [
        ("#@C     #", MoveRight, "# @C    #"),
        ("#     O@#", MoveLeft, "#    O@ #"),
        ("#@COCC  #", MoveRight, "# @COCC #"),
        ("#  CCOC@#", MoveLeft, "# CCOC@ #"),
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
    for (row, input) in [("#@COC#", MoveRight), ("#COC@#", MoveLeft)] {
        let initial = GameState::from_ascii(&format!("######\n{row}\n######")).unwrap();
        assert_eq!(initial.step(input), initial);
    }
}

#[test]
fn cube_pushed_off_a_ledge_falls_in_the_same_update() {
    let initial = GameState::from_ascii("#######\n#@CO  #\n####  #\n#     #\n#######").unwrap();
    let pushed = initial.step(MoveRight);
    assert_eq!(
        pushed.to_ascii(),
        "#######\n# @C  #\n####  #\n#   O #\n#######"
    );
}
