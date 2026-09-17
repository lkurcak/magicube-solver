use lk_math::arraynd::CharArray2d;

fn main() {
    let level_charmap = include_str!("../data/level-manual-labels/1.txt");
    let lines = level_charmap.lines().collect::<Vec<_>>();
    let width = lines
        .iter()
        .map(|line| line.chars().count())
        .max()
        .unwrap_or(0);
    let mut level = CharArray2d::with_dimensions(width, lines.len(), ' ');
    for (y, line) in lines.iter().enumerate() {
        for (x, tile) in line.chars().enumerate() {
            level[y * width + x] = tile;
        }
    }
    println!("{level}");
}
