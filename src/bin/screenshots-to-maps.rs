use magicube_solver::level_catalog::generate_catalog;
use magicube_solver::screenshot_import::save_unknown_tiles;
use std::env;
use std::error::Error;
use std::fs;
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
        .unwrap_or_else(|| PathBuf::from("data/levels"));
    let atlas_dir = args
        .get(2)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/tile-templates"));
    let unknown_dir = levels_dir.join("unknown-tiles");
    let labels_dir = args
        .get(3)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data/level-manual-labels"));
    let catalog = generate_catalog(&screenshots_dir, &labels_dir, &atlas_dir)?;
    if catalog.entries.is_empty() {
        return Err(format!("no PNG screenshots found in {}", screenshots_dir.display()).into());
    }
    fs::create_dir_all(&levels_dir)?;

    let mut report = String::new();
    let mut corrupted = 0;
    for entry in catalog.entries {
        let status = if entry.is_clean() {
            "Clean"
        } else {
            corrupted += 1;
            "Corrupted"
        };
        report.push_str(&format!("{}: {status}\n", entry.screenshot.display()));
        let map_path = levels_dir.join(format!("{}.txt", entry.id));
        if let Some(imported) = entry.imported {
            fs::write(map_path, format!("{}\n", imported.map))?;
            save_unknown_tiles(&unknown_dir, &entry.id, &imported.unknown_tiles)?;
            report.push_str(&format!(
                "  phase ({}, {}), {} anchor matches, {}x{} map\n",
                imported.phase.x,
                imported.phase.y,
                imported.phase.anchor_matches,
                imported.width,
                imported.height,
            ));
        } else {
            // Do not leave an older successful map behind after an import fails.
            match fs::remove_file(map_path) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
            save_unknown_tiles(&unknown_dir, &entry.id, &[])?;
        }
        for issue in entry.issues {
            report.push_str(&format!("  {issue}\n"));
        }
    }
    fs::write(levels_dir.join("import-report.txt"), &report)?;
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
