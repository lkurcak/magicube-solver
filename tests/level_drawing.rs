mod support;

use support::{assert_level_eq, level};

#[test]
fn level_drawings_ignore_test_indentation() {
    let drawing = level(
        r#"
            #####
            # @ #
            #####
        "#,
    );

    assert_level_eq(
        &drawing,
        r#"
            #####
            # @ #
            #####
        "#,
    );
}
