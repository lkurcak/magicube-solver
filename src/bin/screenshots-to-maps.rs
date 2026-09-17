use magicube_solver::screenshot_import::{
    Atlas, import_level, load_png, png_files, save_unknown_tiles,
};
use std::env;
use std::error::Error;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() > 3 {
        return Err("usage: screenshots_to_maps [screenshots-dir] [levels-dir] [atlas-dir]".into());
    }

    let screenshots_dir = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/level-screenshots"));
    let levels_dir = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/levels"));
    let atlas_dir = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/tile-templates"));
    let unknown_dir = levels_dir.join("unknown-tiles");

    let mut atlas = Atlas::load(&atlas_dir)?;
    let labels_dir = PathBuf::from("data/level-manual-labels");
    if labels_dir.exists() {
        let mut label_paths = fs::read_dir(&labels_dir)?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().is_some_and(|extension| extension == "txt"))
            .collect::<Vec<_>>();
        label_paths.sort();
        for label_path in label_paths {
            let name = label_path
                .file_stem()
                .expect("label path has a file stem")
                .to_string_lossy();
            let screenshot_path = screenshots_dir.join(format!("{name}.png"));
            let screenshot = load_png(&screenshot_path)?;
            let labels = fs::read_to_string(&label_path)?;
            let training = atlas.learn_labeled_level(&screenshot, &labels)?;
            println!(
                "Learned {} variants from {} at grid offset ({}, {}), using {} anchor matches",
                training.learned_variants,
                label_path.display(),
                training.grid_offset_x,
                training.grid_offset_y,
                training.anchor_matches
            );
        }
    }
    let screenshots = png_files(&screenshots_dir)?;
    if screenshots.is_empty() {
        return Err(format!("no PNG screenshots found in {}", screenshots_dir.display()).into());
    }
    fs::create_dir_all(&levels_dir)?;

    println!("Using {} as the grid-alignment anchor", atlas.anchor_name());
    for screenshot_path in screenshots {
        let name = screenshot_path
            .file_stem()
            .expect("PNG path has a file stem")
            .to_string_lossy();
        let image = load_png(&screenshot_path)?;
        let imported = import_level(&image, &atlas)?;
        fs::write(
            levels_dir.join(format!("{name}.txt")),
            format!("{}\n", imported.map),
        )?;
        save_unknown_tiles(&unknown_dir, &name, &imported.unknown_tiles)?;
        println!(
            "{}: phase ({}, {}), {} anchor matches, {}x{} map, {} unknown tile variants",
            screenshot_path.display(),
            imported.phase.x,
            imported.phase.y,
            imported.phase.anchor_matches,
            imported.width,
            imported.height,
            imported.unknown_tiles.len()
        );
    }

    Ok(())
}
