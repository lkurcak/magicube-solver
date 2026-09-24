use magicube_solver::level_catalog::{generate_catalog, load_custom_levels};
use std::env;
use std::error::Error;
use std::fmt::Write;
use std::fs;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let catalog = generate_catalog(
        &root.join("data/level-screenshots"),
        &root.join("data/level-manual-labels"),
        &root.join("data/tile-templates"),
    )?;
    let custom_dir = root.join("data/custom-levels");
    let custom_levels = load_custom_levels(&custom_dir)?;
    for input in catalog.inputs {
        println!("cargo::rerun-if-changed={}", input.display());
    }
    // Watching the directory catches added and removed files; each file catches edits.
    println!("cargo::rerun-if-changed={}", custom_dir.display());
    for level in &custom_levels {
        println!("cargo::rerun-if-changed={}", level.path.display());
    }

    // Replace the entire generated module so removed screenshots cannot linger.
    // Debug formatting safely escapes input strings as Rust string literals.
    let mut generated = String::from("const BUNDLED_LEVELS: &[BundledLevel] = &[\n");
    for entry in catalog.entries {
        writeln!(
            generated,
            "BundledLevel {{ id: {:?}, name: {:?}, map: {:?}, issues: &{:?} }},",
            entry.id,
            entry.name,
            entry.map(),
            entry.issues,
        )?;
    }
    // Hand-drawn levels follow the screenshot levels.
    for level in custom_levels {
        writeln!(
            generated,
            "BundledLevel {{ id: {:?}, name: {:?}, map: {:?}, issues: &{:?} }},",
            level.id,
            level.name,
            level.map.as_deref(),
            level.issues,
        )?;
    }
    generated.push_str("];\n");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out_dir.join("bundled_levels.rs"), generated)?;
    Ok(())
}
