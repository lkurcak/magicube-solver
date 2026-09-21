use magicube_solver::level_catalog::generate_catalog;
use magicube_solver::project::write_catalog_cache;
use std::env;
use std::error::Error;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = env::args_os().skip(1).collect();
    if args.len() > 4 {
        return Err(
            "usage: screenshots-to-maps [screenshots-dir] [levels-dir] [atlas-dir] [labels-dir]"
                .into(),
        );
    }

    let screenshots_dir = args
        .first()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/level-screenshots"));
    let levels_dir = args
        .get(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("cache/levels"));
    let default_cache = args.get(1).is_none();
    let atlas_dir = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/tile-templates"));
    let labels_dir = args
        .get(3)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/level-manual-labels"));
    let catalog = generate_catalog(&screenshots_dir, &labels_dir, &atlas_dir)?;
    if catalog.entries.is_empty() {
        return Err(format!("no PNG screenshots found in {}", screenshots_dir.display()).into());
    }
    if default_cache && levels_dir.exists() {
        std::fs::remove_dir_all(&levels_dir)?;
    }
    let corrupted = write_catalog_cache(&catalog, &levels_dir)?;
    let report = std::fs::read_to_string(levels_dir.join("import-report.txt"))?;
    print!("{report}");
    if corrupted > 0 {
        return Err(format!(
            "{corrupted} corrupted level(s); see {}/import-report.txt",
            levels_dir.display()
        )
        .into());
    }
    Ok(())
}
