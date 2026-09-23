#![allow(dead_code)]

use magicube_solver::{Cube, CubeSource};

/// Turns an indented ASCII drawing into a level map.
///
/// The first and last blank lines are ignored, as is indentation shared by
/// every line. Spaces inside the drawing remain part of the level.
pub fn level(drawing: &str) -> String {
    let mut lines: Vec<&str> = drawing.lines().collect();

    while lines.first().is_some_and(|line| line.trim().is_empty()) {
        lines.remove(0);
    }
    while lines.last().is_some_and(|line| line.trim().is_empty()) {
        lines.pop();
    }

    let indentation = lines
        .iter()
        .filter(|line| !line.trim().is_empty())
        .map(|line| line.len() - line.trim_start().len())
        .min()
        .unwrap_or(0);

    lines
        .into_iter()
        .map(|line| line.get(indentation..).unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Compares a rendered level with an indented ASCII drawing.
pub fn assert_level_eq(actual: &str, expected: &str) {
    assert_eq!(level(actual), level(expected));
}

/// Compares cube layouts without depending on their internal storage order.
pub fn assert_cube_layout_eq(actual: &[Cube], expected: &[Cube]) {
    fn key(cube: &Cube) -> (isize, isize, u8) {
        let source = match cube.source {
            CubeSource::Map => 0,
            CubeSource::Player => 1,
            CubeSource::Glass => 2,
        };
        (cube.position.y, cube.position.x, source)
    }

    let mut actual = actual.to_vec();
    let mut expected = expected.to_vec();
    actual.sort_unstable_by_key(key);
    expected.sort_unstable_by_key(key);
    assert_eq!(actual, expected);
}
