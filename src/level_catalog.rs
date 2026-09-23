//! Discover and import screenshot levels for both build-time bundling and inspection.

use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};

use crate::GameState;
use crate::screenshot_import::{
    Atlas, ImportedLevel, import_level_with_labels, load_png, png_files,
};

#[derive(Debug)]
pub struct LevelCatalog {
    pub entries: Vec<CatalogEntry>,
    /// Input directories and external template files to watch for changes.
    pub inputs: Vec<PathBuf>,
}

#[derive(Debug)]
pub struct CatalogEntry {
    pub id: String,
    pub name: String,
    pub screenshot: PathBuf,
    /// Import failures may leave no preview; unresolved tiles still produce a map.
    pub imported: Option<ImportedLevel>,
    pub issues: Vec<String>,
}

impl CatalogEntry {
    pub fn map(&self) -> Option<&str> {
        self.imported.as_ref().map(|imported| imported.map.as_str())
    }

    pub fn is_clean(&self) -> bool {
        self.imported.is_some() && self.issues.is_empty()
    }
}

/// Rebuild the catalog from source inputs only. Individual bad screenshots or
/// labels become entry issues; unreadable shared configuration is an error.
pub fn generate_catalog(
    screenshots_dir: &Path,
    labels_dir: &Path,
    atlas_dir: &Path,
) -> Result<LevelCatalog, Box<dyn Error>> {
    let mut atlas = Atlas::load(atlas_dir)?;
    let screenshots = png_files(screenshots_dir)?;
    let mut inputs = vec![
        screenshots_dir.to_owned(),
        labels_dir.to_owned(),
        atlas_dir.to_owned(),
    ];
    inputs.extend(atlas.template_paths().iter().cloned());

    // Load once, then learn all manual examples before importing any screenshot.
    let mut sources = Vec::new();
    for screenshot in screenshots {
        let id = screenshot
            .file_stem()
            .unwrap()
            .to_str()
            .ok_or("screenshot filenames must be UTF-8")?
            .to_owned();
        let mut entry = CatalogEntry {
            name: format!("Level {id}"),
            id,
            screenshot,
            imported: None,
            issues: Vec::new(),
        };
        let image = match load_png(&entry.screenshot) {
            Ok(image) => Some(image),
            Err(error) => {
                entry
                    .issues
                    .push(format!("cannot read screenshot: {error}"));
                None
            }
        };
        let label_path = labels_dir.join(format!("{}.txt", entry.id));
        let mut occluded_labels = Vec::new();
        match fs::read_to_string(&label_path) {
            Ok(labels) => {
                if let Some(image) = &image {
                    match atlas.learn_labeled_level_from(
                        image,
                        &labels,
                        &label_path.display().to_string(),
                    ) {
                        Ok(summary) => occluded_labels = summary.occluded_labels,
                        Err(error) => entry
                            .issues
                            .push(format!("{}: {error}", label_path.display())),
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => entry
                .issues
                .push(format!("{}: {error}", label_path.display())),
        }
        sources.push((entry, image, occluded_labels));
    }

    let entries = sources
        .into_iter()
        .map(|(mut entry, image, occluded_labels)| {
            if let Some(image) = image {
                match import_level_with_labels(&image, &atlas, &occluded_labels) {
                    Ok(imported) => {
                        for tile in &imported.unknown_tiles {
                            entry.issues.push(format!(
                                "{} (tile {:016x}, map coordinates {:?})",
                                tile.reason, tile.hash, tile.positions,
                            ));
                        }
                        entry.issues.extend(validate_map(&imported.map));
                        entry.imported = Some(imported);
                    }
                    Err(error) => entry.issues.push(error.to_string()),
                }
            }
            entry
        })
        .collect();

    Ok(LevelCatalog { entries, inputs })
}

fn validate_map(map: &str) -> Vec<String> {
    let mut issues = Vec::new();
    let has_goal = match GameState::from_ascii(map) {
        Ok(game) => game.level().has_goal(),
        Err(error) => {
            issues.push(format!("invalid game map: {error}"));
            map.chars().any(|symbol| symbol == 'G')
        }
    };
    if !has_goal {
        issues.push("map has no goal".to_owned());
    }
    issues
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_manual_examples_are_clean_and_reproduced() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let labels_dir = root.join("data/level-manual-labels");
        let catalog = generate_catalog(
            &root.join("data/level-screenshots"),
            &labels_dir,
            &root.join("data/tile-templates"),
        )
        .unwrap();
        let mut manual_count = 0;
        for entry in &catalog.entries {
            if let Ok(labels) = fs::read_to_string(labels_dir.join(format!("{}.txt", entry.id))) {
                assert!(entry.is_clean(), "{}: {:?}", entry.name, entry.issues);
                assert_eq!(entry.map().unwrap(), labels.trim_end(), "{}", entry.name);
                manual_count += 1;
            }
        }
        assert!(manual_count >= 8);
        assert_eq!(catalog.entries.len(), 45);
        for id in ["9", "10", "45"] {
            assert!(catalog.entries.iter().any(|entry| entry.id == id));
        }
    }

    #[test]
    fn validates_simulator_requirements_and_goal_presence() {
        for map in ["#@ #\n##G#", "#@D#\n##G#"] {
            assert!(validate_map(map).is_empty(), "{map:?}");
        }
        for map in ["", "#G#", "#@@G#", "#@OOG#", "#@!G#", "#@#"] {
            assert!(!validate_map(map).is_empty(), "{map:?}");
        }
    }

    #[test]
    fn gate_above_pedestal_repairs_level_15() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let catalog = generate_catalog(
            &root.join("data/level-screenshots"),
            &root.join("data/level-manual-labels"),
            &root.join("data/tile-templates"),
        )
        .unwrap();
        let level = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "15")
            .unwrap();

        assert!(level.is_clean(), "{:?}", level.issues);
        let map = level.map().unwrap().lines().collect::<Vec<_>>();
        assert_eq!(map[2].chars().nth(15), Some('D'));
        assert_eq!(map[3].chars().nth(15), Some('G'));
    }

    #[test]
    fn occupied_pressure_plate_and_hidden_goal_import_level_20() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR"));
        let catalog = generate_catalog(
            &root.join("data/level-screenshots"),
            &root.join("data/level-manual-labels"),
            &root.join("data/tile-templates"),
        )
        .unwrap();
        let level = catalog
            .entries
            .iter()
            .find(|entry| entry.id == "20")
            .unwrap();

        assert!(level.is_clean(), "{:?}", level.issues);
        let map = level.map().unwrap().lines().collect::<Vec<_>>();
        assert_eq!(map[4].chars().nth(2), Some('C'));
        assert_eq!(map[5].chars().nth(2), Some('P'));
        assert!(level.map().unwrap().contains('G'));
    }
}
