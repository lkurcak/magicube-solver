use magicube_solver::level_catalog::generate_catalog;
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
    for input in catalog.inputs {
        println!("cargo::rerun-if-changed={}", input.display());
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
    generated.push_str("];\n");
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    fs::write(out_dir.join("bundled_levels.rs"), generated)?;
    Ok(())
}
