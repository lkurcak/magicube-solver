use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

use image::{Rgba, RgbaImage, imageops};
use magicube_solver::level_catalog::{LevelCatalog, generate_catalog};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "magicube-catalog-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed),
        ));
        for dir in ["screenshots", "labels", "atlas", "tiles"] {
            fs::create_dir_all(root.join(dir)).unwrap();
        }
        for seed in 1..=3 {
            tile(seed)
                .save(root.join(format!("tiles/{seed}.png")))
                .unwrap();
        }
        fs::write(
            root.join("atlas/atlas.txt"),
            "anchor # ../tiles/1.png\n@ ../tiles/2.png\nG ../tiles/3.png\n",
        )
        .unwrap();
        Self(root)
    }

    fn screenshot(&self, id: &str, seeds: &[u8]) {
        let mut image = RgbaImage::new(seeds.len() as u32 * 8, 8);
        for (index, &seed) in seeds.iter().enumerate() {
            imageops::replace(&mut image, &tile(seed), index as i64 * 8, 0);
        }
        image
            .save(self.0.join(format!("screenshots/{id}.png")))
            .unwrap();
    }

    fn catalog(&self) -> LevelCatalog {
        generate_catalog(
            &self.0.join("screenshots"),
            &self.0.join("labels"),
            &self.0.join("atlas"),
        )
        .unwrap()
    }

    fn converter(&self) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_screenshots-to-maps"))
            .arg(self.0.join("screenshots"))
            .arg(self.0.join("output"))
            .arg(self.0.join("atlas"))
            .arg(self.0.join("labels"))
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn tile(seed: u8) -> RgbaImage {
    RgbaImage::from_fn(8, 8, |x, y| Rgba([seed, x as u8 + 1, y as u8 + 1, 255]))
}

#[test]
fn discovers_natural_order_and_isolates_bad_levels_without_stale_entries() {
    let fixture = Fixture::new();
    for id in ["10", "1"] {
        fixture.screenshot(id, &[1, 2, 3]);
    }
    fixture.screenshot("2", &[1, 2, 3, 4]); // Unknown pattern.
    fs::write(fixture.0.join("screenshots/3.png"), "broken PNG").unwrap();
    fixture.screenshot("4", &[1, 3]); // Missing player.
    fixture.screenshot("5", &[1, 2]); // Missing goal.
    fixture.screenshot("6", &[2, 3]); // Missing anchor.
    fs::write(fixture.0.join("screenshots/notes.txt"), "not a level").unwrap();
    let catalog = fixture.catalog();
    assert_eq!(
        catalog
            .entries
            .iter()
            .map(|entry| entry.id.as_str())
            .collect::<Vec<_>>(),
        ["1", "2", "3", "4", "5", "6", "10"]
    );
    for entry in &catalog.entries {
        assert_eq!(
            entry.is_clean(),
            matches!(entry.id.as_str(), "1" | "10"),
            "{entry:?}"
        );
    }
    assert_eq!(catalog.entries[1].map(), Some("#@G?"));
    assert!(catalog.entries[2].map().is_none());
    assert!(catalog.entries[5].map().is_none());
    assert!(
        catalog
            .inputs
            .iter()
            .any(|path| path.ends_with("tiles/1.png"))
    );

    fs::remove_file(fixture.0.join("screenshots/10.png")).unwrap();
    fs::rename(
        fixture.0.join("screenshots/1.png"),
        fixture.0.join("screenshots/20.png"),
    )
    .unwrap();
    let catalog = fixture.catalog();
    assert_eq!(catalog.entries.last().unwrap().id, "20");
    assert!(
        !catalog
            .entries
            .iter()
            .any(|entry| matches!(entry.id.as_str(), "1" | "10"))
    );
}

#[test]
fn a_new_teaching_example_repairs_other_levels_and_conflicts_remain_visible() {
    let fixture = Fixture::new();
    for id in ["1", "2"] {
        fixture.screenshot(id, &[1, 2, 3, 4]);
    }
    assert!(
        fixture
            .catalog()
            .entries
            .iter()
            .all(|entry| !entry.is_clean())
    );
    fs::write(fixture.0.join("labels/2.txt"), "#@Gt").unwrap();
    assert!(
        fixture
            .catalog()
            .entries
            .iter()
            .all(|entry| entry.is_clean() && entry.map() == Some("#@Gt"))
    );

    fs::write(fixture.0.join("labels/1.txt"), "#@GS").unwrap();
    let catalog = fixture.catalog();
    for entry in catalog.entries {
        assert!(!entry.is_clean());
        let report = entry.issues.join("\n");
        assert!(report.contains("labels/1.txt at (3, 0)"));
        assert!(report.contains("labels/2.txt at (3, 0)"));
    }
    fs::write(fixture.0.join("labels/1.txt"), "#@G?").unwrap();
    assert!(
        fixture
            .catalog()
            .entries
            .iter()
            .all(|entry| entry.is_clean())
    );

    // A malformed training file affects its own entry but cannot poison others.
    fs::write(fixture.0.join("labels/1.txt"), "#CG!").unwrap();
    let catalog = fixture.catalog();
    assert!(!catalog.entries[0].is_clean());
    assert!(catalog.entries[1].is_clean());
    assert_eq!(catalog.entries[1].map(), Some("#@Gt"));
}

#[test]
fn broken_shared_configuration_is_a_catalog_error() {
    let fixture = Fixture::new();
    fs::write(fixture.0.join("atlas/atlas.txt"), "anchor # missing.png").unwrap();
    assert!(
        generate_catalog(
            &fixture.0.join("screenshots"),
            &fixture.0.join("labels"),
            &fixture.0.join("atlas")
        )
        .is_err()
    );
}

#[test]
fn converter_exports_repairs_and_removes_a_map_after_a_failed_reimport() {
    let fixture = Fixture::new();
    fixture.screenshot("1", &[1, 2, 3]);
    fixture.screenshot("2", &[1, 2, 3, 4]);
    let output = fixture.converter();
    assert!(!output.status.success());
    let report = fs::read_to_string(fixture.0.join("output/import-report.txt")).unwrap();
    assert!(report.contains("1.png: Clean"));
    assert!(report.contains("2.png: Corrupted"));
    assert!(report.contains("unrecognized tile"));
    assert_eq!(
        fs::read_to_string(fixture.0.join("output/2.txt")).unwrap(),
        "#@G?\n"
    );
    let index = fs::read_to_string(fixture.0.join("output/unknown-tiles/2.tsv")).unwrap();
    let fields = index
        .lines()
        .nth(1)
        .unwrap()
        .split('\t')
        .collect::<Vec<_>>();
    assert!(
        fixture
            .0
            .join("output/unknown-tiles")
            .join(fields[1])
            .is_file()
    );
    assert_eq!(fields[2], "3,0");

    fs::write(fixture.0.join("labels/2.txt"), "#@Gt").unwrap();
    assert!(fixture.converter().status.success());
    fs::write(fixture.0.join("screenshots/1.png"), "broken PNG").unwrap();
    assert!(!fixture.converter().status.success());
    assert!(!fixture.0.join("output/1.txt").exists());
    assert!(fixture.0.join("output/2.txt").is_file());
}
