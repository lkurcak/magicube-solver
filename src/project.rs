//! Trusted project inputs, disposable import artifacts, and solver-result cache.

use std::collections::{BTreeSet, HashMap};
use std::error::Error;
use std::fs::{self, File};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::level_catalog::{CatalogEntry, LevelCatalog, generate_catalog};
use crate::screenshot_import::save_unknown_tiles;
use crate::{
    GameInput, GameState, GameStatus, SolveOptions, SolveOutcome, SolveResult, SolveStats,
};

pub const SOLVER_CACHE_VERSION: u32 = 1;

#[derive(Debug, Clone)]
pub struct ProjectPaths {
    pub manifest: PathBuf,
    pub screenshots: PathBuf,
    pub labels: PathBuf,
    pub atlas: PathBuf,
    pub cache: PathBuf,
}

impl ProjectPaths {
    pub fn from_root(root: &Path) -> Self {
        Self {
            manifest: root.join("data/level-manifest.txt"),
            screenshots: root.join("data/level-screenshots"),
            labels: root.join("data/level-manual-labels"),
            atlas: root.join("data/tile-templates"),
            cache: root.join("cache"),
        }
    }

    pub fn levels_cache(&self) -> PathBuf {
        self.cache.join("levels")
    }

    pub fn solver_cache(&self) -> PathBuf {
        self.cache.join("solver-results")
    }
}

#[derive(Debug, Clone)]
pub struct ProjectLevel {
    pub id: String,
    pub name: String,
    pub listed: bool,
    pub screenshot_present: bool,
    pub map: Option<String>,
    pub width: Option<usize>,
    pub height: Option<usize>,
    pub unknown_tiles: usize,
    pub issues: Vec<String>,
}

impl ProjectLevel {
    pub fn is_clean(&self) -> bool {
        self.listed && self.map.is_some() && self.issues.is_empty()
    }
}

#[derive(Debug, Clone)]
pub struct ProjectImport {
    pub levels: Vec<ProjectLevel>,
    pub expected_count: usize,
}

/// One non-empty, non-comment level ID per line. IDs are also screenshot stems.
pub fn load_manifest(path: &Path) -> Result<Vec<String>, Box<dyn Error>> {
    let text = fs::read_to_string(path)
        .map_err(|error| invalid_data(format!("{}: {error}", path.display())))?;
    let mut ids = Vec::new();
    let mut seen = BTreeSet::new();
    for (line_index, line) in text.lines().enumerate() {
        let id = line.trim();
        if id.is_empty() || id.starts_with('#') {
            continue;
        }
        if !id
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || matches!(character, '-' | '_'))
        {
            return Err(invalid_data(format!(
                "{}:{}: level IDs may contain only ASCII letters, digits, '-' and '_'",
                path.display(),
                line_index + 1
            )));
        }
        if !seen.insert(id.to_owned()) {
            return Err(invalid_data(format!(
                "{}:{}: duplicate level ID {id:?}",
                path.display(),
                line_index + 1
            )));
        }
        ids.push(id.to_owned());
    }
    if ids.is_empty() {
        return Err(invalid_data(format!(
            "{}: manifest contains no level IDs",
            path.display()
        )));
    }
    Ok(ids)
}

/// Rebuild all inferred maps and diagnostics without modifying trusted inputs.
pub fn import_project(paths: &ProjectPaths) -> Result<ProjectImport, Box<dyn Error>> {
    let expected = load_manifest(&paths.manifest)?;
    let catalog = generate_catalog(&paths.screenshots, &paths.labels, &paths.atlas)?;
    let levels_cache = paths.levels_cache();
    if levels_cache.exists() {
        fs::remove_dir_all(&levels_cache)?;
    }
    write_catalog_cache(&catalog, &levels_cache)?;

    let mut entries = catalog
        .entries
        .iter()
        .map(|entry| (entry.id.clone(), entry))
        .collect::<HashMap<_, _>>();
    let mut levels = Vec::new();
    for id in &expected {
        match entries.remove(id) {
            Some(entry) => levels.push(project_level(entry, true)),
            None => levels.push(ProjectLevel {
                id: id.clone(),
                name: format!("Level {id}"),
                listed: true,
                screenshot_present: false,
                map: None,
                width: None,
                height: None,
                unknown_tiles: 0,
                issues: vec![format!(
                    "missing screenshot {}",
                    paths.screenshots.join(format!("{id}.png")).display()
                )],
            }),
        }
    }
    // Do not hide accidental inputs: show them after the goal levels, but never solve them.
    for entry in &catalog.entries {
        if let Some(entry) = entries.remove(&entry.id) {
            let mut level = project_level(entry, false);
            level.issues.insert(
                0,
                "screenshot is not listed in the project manifest".to_owned(),
            );
            levels.push(level);
        }
    }
    Ok(ProjectImport {
        levels,
        expected_count: expected.len(),
    })
}

fn project_level(entry: &CatalogEntry, listed: bool) -> ProjectLevel {
    ProjectLevel {
        id: entry.id.clone(),
        name: entry.name.clone(),
        listed,
        screenshot_present: true,
        map: entry.map().map(str::to_owned),
        width: entry.imported.as_ref().map(|imported| imported.width),
        height: entry.imported.as_ref().map(|imported| imported.height),
        unknown_tiles: entry
            .imported
            .as_ref()
            .map_or(0, |imported| imported.unknown_tiles.len()),
        issues: entry.issues.clone(),
    }
}

/// Write inferred maps, report, and diagnostics into an output directory.
/// Callers that own a disposable directory may remove it first to clear outputs
/// for screenshots that no longer exist.
pub fn write_catalog_cache(
    catalog: &LevelCatalog,
    directory: &Path,
) -> Result<usize, Box<dyn Error>> {
    fs::create_dir_all(directory)?;
    let unknown_dir = directory.join("unknown-tiles");
    if unknown_dir.exists() {
        fs::remove_dir_all(&unknown_dir)?;
    }
    let mut report = String::new();
    let mut corrupted = 0;
    for entry in &catalog.entries {
        let status = if entry.is_clean() {
            "Clean"
        } else {
            corrupted += 1;
            "Corrupted"
        };
        report.push_str(&format!("{}: {status}\n", entry.screenshot.display()));
        if let Some(imported) = &entry.imported {
            atomic_write(
                &directory.join(format!("{}.txt", entry.id)),
                format!("{}\n", imported.map).as_bytes(),
            )?;
            save_unknown_tiles(&unknown_dir, &entry.id, &imported.unknown_tiles)?;
            report.push_str(&format!(
                "  phase ({}, {}), {} anchor matches, {}x{} map\n",
                imported.phase.x,
                imported.phase.y,
                imported.phase.anchor_matches,
                imported.width,
                imported.height
            ));
        } else {
            match fs::remove_file(directory.join(format!("{}.txt", entry.id))) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(error) => return Err(error.into()),
            }
        }
        for issue in &entry.issues {
            report.push_str(&format!("  {issue}\n"));
        }
    }
    atomic_write(&directory.join("import-report.txt"), report.as_bytes())?;
    Ok(corrupted)
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SolverCacheRecord {
    pub format_version: u32,
    pub level_id: String,
    pub map: String,
    pub max_states: Option<usize>,
    pub executable_fingerprint: String,
    pub outcome: CachedSolveOutcome,
    pub stats: CachedSolveStats,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum CachedSolveOutcome {
    Solved { inputs: Vec<String> },
    Unsolvable,
    StateLimitReached,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct CachedSolveStats {
    pub discovered_states: usize,
    pub expanded_states: usize,
}

impl From<SolveStats> for CachedSolveStats {
    fn from(stats: SolveStats) -> Self {
        Self {
            discovered_states: stats.discovered_states,
            expanded_states: stats.expanded_states,
        }
    }
}

impl SolverCacheRecord {
    pub fn from_result(
        level_id: &str,
        map: &str,
        options: SolveOptions,
        executable_fingerprint: &str,
        result: &SolveResult,
    ) -> Self {
        let outcome = match &result.outcome {
            SolveOutcome::Solved(inputs) => CachedSolveOutcome::Solved {
                inputs: inputs
                    .iter()
                    .copied()
                    .map(input_name)
                    .map(str::to_owned)
                    .collect(),
            },
            SolveOutcome::Unsolvable => CachedSolveOutcome::Unsolvable,
            SolveOutcome::StateLimitReached => CachedSolveOutcome::StateLimitReached,
        };
        Self {
            format_version: SOLVER_CACHE_VERSION,
            level_id: level_id.to_owned(),
            map: map.to_owned(),
            max_states: options.max_states,
            executable_fingerprint: executable_fingerprint.to_owned(),
            outcome,
            stats: result.stats.into(),
        }
    }

    pub fn load(path: &Path) -> io::Result<Self> {
        let bytes = fs::read(path)?;
        let record: Self = serde_json::from_slice(&bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!("invalid cache JSON: {error}"),
            )
        })?;
        if record.format_version != SOLVER_CACHE_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("unsupported solver cache version {}", record.format_version),
            ));
        }
        Ok(record)
    }

    pub fn save(&self, path: &Path) -> io::Result<()> {
        let mut bytes = serde_json::to_vec_pretty(self).map_err(io::Error::other)?;
        bytes.push(b'\n');
        atomic_write(path, &bytes)
    }

    pub fn solved_inputs_for(&self, current_map: &str) -> Option<Vec<GameInput>> {
        let CachedSolveOutcome::Solved { inputs } = &self.outcome else {
            return None;
        };
        let inputs = inputs
            .iter()
            .map(|input| parse_input(input))
            .collect::<Option<Vec<_>>>()?;
        let mut state = GameState::from_ascii(current_map).ok()?;
        for input in &inputs {
            state = state.step(*input);
        }
        (state.status() == GameStatus::Won).then_some(inputs)
    }

    pub fn reusable_failure(
        &self,
        level_id: &str,
        current_map: &str,
        options: SolveOptions,
        fingerprint: &str,
    ) -> bool {
        !matches!(self.outcome, CachedSolveOutcome::Solved { .. })
            && self.level_id == level_id
            && self.map == current_map
            && self.max_states == options.max_states
            && self.executable_fingerprint == fingerprint
    }
}

pub fn executable_fingerprint() -> io::Result<String> {
    let path = std::env::current_exe()?;
    let mut file = File::open(path)?;
    let mut buffer = [0_u8; 64 * 1024];
    let mut hash = 0xcbf29ce484222325_u64;
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        for byte in &buffer[..count] {
            hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
        }
    }
    Ok(format!("fnv1a64:{hash:016x}"))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let filename = path.file_name().unwrap_or_default().to_string_lossy();
    let temporary = path.with_file_name(format!(".{filename}.tmp-{}", std::process::id()));
    let result = (|| {
        let mut file = File::create(&temporary)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&temporary, path)
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

fn input_name(input: GameInput) -> &'static str {
    match input {
        GameInput::Left => "left",
        GameInput::Right => "right",
        GameInput::Jump => "jump",
        GameInput::Shoot => "shoot",
        GameInput::Wait => "wait",
    }
}

fn parse_input(input: &str) -> Option<GameInput> {
    match input {
        "left" => Some(GameInput::Left),
        "right" => Some(GameInput::Right),
        "jump" => Some(GameInput::Jump),
        "shoot" => Some(GameInput::Shoot),
        "wait" => Some(GameInput::Wait),
        _ => None,
    }
}

fn invalid_data(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_rejects_duplicates_and_unsafe_ids() {
        let root = std::env::temp_dir().join(format!("magicube-manifest-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("levels.txt");
        fs::write(&path, "# goal\n1\n2\n").unwrap();
        assert_eq!(load_manifest(&path).unwrap(), ["1", "2"]);
        fs::write(&path, "1\n1\n").unwrap();
        assert!(
            load_manifest(&path)
                .unwrap_err()
                .to_string()
                .contains("duplicate")
        );
        fs::write(&path, "../1\n").unwrap();
        assert!(load_manifest(&path).is_err());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn successful_cache_is_replayed_but_failures_require_exact_fingerprint() {
        let map = "#####\n#@G##\n#####";
        let result = SolveResult {
            outcome: SolveOutcome::Solved(vec![GameInput::Shoot, GameInput::Right]),
            stats: SolveStats {
                discovered_states: 2,
                expanded_states: 1,
            },
        };
        let record =
            SolverCacheRecord::from_result("1", map, SolveOptions::default(), "old", &result);
        assert_eq!(
            record.solved_inputs_for(map),
            Some(vec![GameInput::Shoot, GameInput::Right])
        );
        assert!(record.solved_inputs_for("#####\n#@###\n#####").is_none());

        let failed = SolverCacheRecord {
            outcome: CachedSolveOutcome::StateLimitReached,
            ..record
        };
        assert!(failed.reusable_failure("1", map, SolveOptions::default(), "old"));
        assert!(!failed.reusable_failure("1", map, SolveOptions::default(), "new"));
    }
}
