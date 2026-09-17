use image::{RgbaImage, imageops};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const TILE_SIZE: u32 = 8;

#[derive(Debug)]
pub struct Atlas {
    templates: Vec<Template>,
    anchor_index: usize,
}

#[derive(Debug)]
struct Template {
    symbol: char,
    name: String,
    image: RgbaImage,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPhase {
    pub x: u32,
    pub y: u32,
    pub anchor_matches: usize,
}

#[derive(Debug)]
pub struct ImportedLevel {
    pub map: String,
    pub phase: GridPhase,
    pub width: usize,
    pub height: usize,
    pub unknown_tiles: Vec<UnknownTile>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrainingSummary {
    pub grid_offset_x: usize,
    pub grid_offset_y: usize,
    pub anchor_matches: usize,
    pub learned_variants: usize,
}

#[derive(Debug)]
pub struct UnknownTile {
    pub hash: u64,
    pub image: RgbaImage,
    pub positions: Vec<(usize, usize)>,
}

impl Atlas {
    pub fn load(directory: &Path) -> Result<Self, Box<dyn Error>> {
        let manifest_path = directory.join("atlas.txt");
        let manifest = fs::read_to_string(&manifest_path)?;
        let mut templates = Vec::new();
        let mut anchor_index = None;

        for (line_index, line) in manifest.lines().enumerate() {
            let line = line.trim();
            if line.is_empty() || line.starts_with("//") {
                continue;
            }

            let fields: Vec<_> = line.split_whitespace().collect();
            if fields.len() != 2 && fields.len() != 3 {
                return Err(invalid_data(format!(
                    "{}:{}: expected `[anchor] <character> <png>`",
                    manifest_path.display(),
                    line_index + 1
                )));
            }

            let (is_anchor, symbol_text, filename) = if fields.len() == 3 {
                if fields[0] != "anchor" {
                    return Err(invalid_data(format!(
                        "{}:{}: the optional first field must be `anchor`",
                        manifest_path.display(),
                        line_index + 1
                    )));
                }
                (true, fields[1], fields[2])
            } else {
                (false, fields[0], fields[1])
            };

            let mut chars = symbol_text.chars();
            let symbol = match (chars.next(), chars.next()) {
                (Some('_'), None) => ' ',
                (Some(symbol), None) => symbol,
                _ => {
                    return Err(invalid_data(format!(
                        "{}:{}: tile character must be one character (`_` means space)",
                        manifest_path.display(),
                        line_index + 1
                    )));
                }
            };
            let template_path = directory.join(filename);
            let image = image::open(&template_path)?.to_rgba8();
            require_tile_size(&image, &template_path)?;
            if is_anchor && anchor_index.replace(templates.len()).is_some() {
                return Err(invalid_data("atlas contains more than one anchor template"));
            }
            templates.push(Template {
                symbol,
                name: filename.to_owned(),
                image,
            });
        }

        if templates.is_empty() {
            return Err(invalid_data("atlas has no templates"));
        }
        let anchor_index =
            anchor_index.ok_or_else(|| invalid_data("atlas has no anchor template"))?;
        Ok(Self {
            templates,
            anchor_index,
        })
    }

    pub fn anchor_name(&self) -> &str {
        &self.templates[self.anchor_index].name
    }

    fn anchor(&self) -> &RgbaImage {
        &self.templates[self.anchor_index].image
    }

    fn classify(&self, tile: &RgbaImage) -> Option<char> {
        self.templates
            .iter()
            .find(|template| template.image.as_raw() == tile.as_raw())
            .map(|template| template.symbol)
    }

    pub fn learn_labeled_level(
        &mut self,
        image: &RgbaImage,
        labels: &str,
    ) -> Result<TrainingSummary, Box<dyn Error>> {
        let rows = labeled_rows(labels)?;
        let target_width = rows.iter().map(Vec::len).max().unwrap_or(0);
        let target_height = rows.len();
        let phase = detect_grid_phase(image, self.anchor())?;
        let xs = tile_origins(phase.x, image.width());
        let ys = tile_origins(phase.y, image.height());
        if target_width > xs.len() || target_height > ys.len() {
            return Err(invalid_data(
                "labeled map is larger than the screenshot's tile grid",
            ));
        }

        let anchor_symbol = self.templates[self.anchor_index].symbol;
        let mut placements = Vec::new();
        let mut best_score = 0;
        for offset_y in 0..=ys.len() - target_height {
            for offset_x in 0..=xs.len() - target_width {
                let score = rows
                    .iter()
                    .enumerate()
                    .flat_map(|(y, row)| {
                        row.iter()
                            .enumerate()
                            .filter(move |(_, symbol)| **symbol == anchor_symbol)
                            .map(move |(x, _)| (x, y))
                    })
                    .filter(|(x, y)| {
                        let tile = imageops::crop_imm(
                            image,
                            xs[offset_x + x],
                            ys[offset_y + y],
                            TILE_SIZE,
                            TILE_SIZE,
                        )
                        .to_image();
                        tile.as_raw() == self.anchor().as_raw()
                    })
                    .count();
                if score > best_score {
                    best_score = score;
                    placements.clear();
                    placements.push((offset_x, offset_y));
                } else if score == best_score {
                    placements.push((offset_x, offset_y));
                }
            }
        }

        if best_score == 0 {
            return Err(invalid_data(
                "could not place labeled map: none of its anchor cells matched the screenshot",
            ));
        }
        if placements.len() != 1 {
            return Err(invalid_data(format!(
                "could not place labeled map unambiguously: {} placements tied with {best_score} anchor matches",
                placements.len()
            )));
        }
        let (offset_x, offset_y) = placements[0];

        let mut learned_variants = 0;
        for (y, row) in rows.iter().enumerate() {
            for (x, &symbol) in row.iter().enumerate() {
                if symbol == '?' {
                    continue;
                }
                let tile = imageops::crop_imm(
                    image,
                    xs[offset_x + x],
                    ys[offset_y + y],
                    TILE_SIZE,
                    TILE_SIZE,
                )
                .to_image();
                if let Some(template) = self
                    .templates
                    .iter_mut()
                    .find(|template| template.image.as_raw() == tile.as_raw())
                {
                    template.symbol = symbol;
                } else {
                    self.templates.push(Template {
                        symbol,
                        name: format!("learned-{x}-{y}"),
                        image: tile,
                    });
                    learned_variants += 1;
                }
            }
        }

        Ok(TrainingSummary {
            grid_offset_x: offset_x,
            grid_offset_y: offset_y,
            anchor_matches: best_score,
            learned_variants,
        })
    }
}

pub fn load_png(path: &Path) -> Result<RgbaImage, Box<dyn Error>> {
    Ok(image::open(path)?.to_rgba8())
}

pub fn import_level(image: &RgbaImage, atlas: &Atlas) -> Result<ImportedLevel, Box<dyn Error>> {
    let phase = detect_grid_phase(image, atlas.anchor())?;
    let mut rows = Vec::new();
    let mut unknown: BTreeMap<u64, UnknownTile> = BTreeMap::new();

    let ys = tile_origins(phase.y, image.height());
    let xs = tile_origins(phase.x, image.width());
    for (grid_y, &pixel_y) in ys.iter().enumerate() {
        let mut row = Vec::new();
        for (grid_x, &pixel_x) in xs.iter().enumerate() {
            let tile = imageops::crop_imm(image, pixel_x, pixel_y, TILE_SIZE, TILE_SIZE).to_image();
            let symbol = if is_void(&tile) {
                ' '
            } else if let Some(symbol) = atlas.classify(&tile) {
                symbol
            } else {
                let hash = tile_hash(&tile);
                unknown
                    .entry(hash)
                    .or_insert_with(|| UnknownTile {
                        hash,
                        image: tile.clone(),
                        positions: Vec::new(),
                    })
                    .positions
                    .push((grid_x, grid_y));
                '?'
            };
            row.push(symbol);
        }
        rows.push(row);
    }

    let trimmed = trim_void(rows)?;
    for tile in unknown.values_mut() {
        for (x, y) in &mut tile.positions {
            *x -= trimmed.offset_x;
            *y -= trimmed.offset_y;
        }
    }
    let map = trimmed
        .rows
        .into_iter()
        .map(|row| row.into_iter().collect::<String>().trim_end().to_owned())
        .collect::<Vec<_>>()
        .join("\n");

    Ok(ImportedLevel {
        map,
        phase,
        width: trimmed.width,
        height: trimmed.height,
        unknown_tiles: unknown.into_values().collect(),
    })
}

pub fn detect_grid_phase(
    image: &RgbaImage,
    anchor: &RgbaImage,
) -> Result<GridPhase, Box<dyn Error>> {
    require_tile_size(anchor, Path::new("anchor template"))?;
    if image.width() < TILE_SIZE || image.height() < TILE_SIZE {
        return Err(invalid_data("screenshot is smaller than one tile"));
    }

    let mut counts = [[0_usize; TILE_SIZE as usize]; TILE_SIZE as usize];
    for y in 0..=image.height() - TILE_SIZE {
        for x in 0..=image.width() - TILE_SIZE {
            let matches = (0..TILE_SIZE).all(|tile_y| {
                (0..TILE_SIZE).all(|tile_x| {
                    image.get_pixel(x + tile_x, y + tile_y) == anchor.get_pixel(tile_x, tile_y)
                })
            });
            if matches {
                counts[(y % TILE_SIZE) as usize][(x % TILE_SIZE) as usize] += 1;
            }
        }
    }

    let mut best = GridPhase {
        x: 0,
        y: 0,
        anchor_matches: 0,
    };
    for y in 0..TILE_SIZE {
        for x in 0..TILE_SIZE {
            let count = counts[y as usize][x as usize];
            if count > best.anchor_matches {
                best = GridPhase {
                    x,
                    y,
                    anchor_matches: count,
                };
            }
        }
    }

    if best.anchor_matches == 0 {
        return Err(invalid_data("anchor tile was not found in screenshot"));
    }
    Ok(best)
}

pub fn png_files(directory: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut paths: Vec<_> = fs::read_dir(directory)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|extension| extension == "png"))
        .collect();
    paths.sort_by(|a, b| natural_stem(a).cmp(&natural_stem(b)).then_with(|| a.cmp(b)));
    Ok(paths)
}

pub fn save_unknown_tiles(
    directory: &Path,
    level_name: &str,
    tiles: &[UnknownTile],
) -> Result<(), Box<dyn Error>> {
    fs::create_dir_all(directory)?;
    let mut index = String::from("hash\tfile\tpositions\n");
    for tile in tiles {
        let filename = format!("{:016x}.png", tile.hash);
        tile.image.save(directory.join(&filename))?;
        let positions = tile
            .positions
            .iter()
            .map(|(x, y)| format!("{x},{y}"))
            .collect::<Vec<_>>()
            .join(" ");
        index.push_str(&format!("{:016x}\t{filename}\t{positions}\n", tile.hash));
    }
    fs::write(directory.join(format!("{level_name}.tsv")), index)?;
    Ok(())
}

fn tile_origins(phase: u32, image_size: u32) -> Vec<u32> {
    (phase..image_size.saturating_sub(TILE_SIZE - 1))
        .step_by(TILE_SIZE as usize)
        .collect()
}

fn labeled_rows(labels: &str) -> Result<Vec<Vec<char>>, Box<dyn Error>> {
    let rows = labels
        .trim_end_matches(['\n', '\r'])
        .lines()
        .map(|line| line.trim_end_matches('\r').chars().collect::<Vec<_>>())
        .collect::<Vec<_>>();
    if rows.is_empty() || rows.iter().all(Vec::is_empty) {
        return Err(invalid_data("labeled map is empty"));
    }
    Ok(rows)
}

fn is_void(tile: &RgbaImage) -> bool {
    tile.pixels().all(|pixel| pixel.0[..3] == [0, 0, 0])
}

fn tile_hash(tile: &RgbaImage) -> u64 {
    tile.as_raw().iter().fold(0xcbf29ce484222325, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}

struct TrimmedGrid {
    rows: Vec<Vec<char>>,
    width: usize,
    height: usize,
    offset_x: usize,
    offset_y: usize,
}

fn trim_void(rows: Vec<Vec<char>>) -> Result<TrimmedGrid, Box<dyn Error>> {
    let occupied: Vec<_> = rows
        .iter()
        .enumerate()
        .flat_map(|(y, row)| {
            row.iter()
                .enumerate()
                .filter(|(_, symbol)| **symbol != ' ')
                .map(move |(x, _)| (x, y))
        })
        .collect();
    let min_x = occupied.iter().map(|(x, _)| x).min().copied();
    let max_x = occupied.iter().map(|(x, _)| x).max().copied();
    let min_y = occupied.iter().map(|(_, y)| y).min().copied();
    let max_y = occupied.iter().map(|(_, y)| y).max().copied();
    let (Some(min_x), Some(max_x), Some(min_y), Some(max_y)) = (min_x, max_x, min_y, max_y) else {
        return Err(invalid_data("screenshot contains no non-void tiles"));
    };

    let trimmed = rows[min_y..=max_y]
        .iter()
        .map(|row| row[min_x..=max_x].to_vec())
        .collect::<Vec<_>>();
    Ok(TrimmedGrid {
        rows: trimmed,
        width: max_x - min_x + 1,
        height: max_y - min_y + 1,
        offset_x: min_x,
        offset_y: min_y,
    })
}

fn require_tile_size(image: &RgbaImage, path: &Path) -> Result<(), Box<dyn Error>> {
    if image.dimensions() != (TILE_SIZE, TILE_SIZE) {
        return Err(invalid_data(format!(
            "{} is {}x{}; templates must be {TILE_SIZE}x{TILE_SIZE}",
            path.display(),
            image.width(),
            image.height()
        )));
    }
    Ok(())
}

fn natural_stem(path: &Path) -> (bool, u64, String) {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    match stem.parse::<u64>() {
        Ok(number) => (false, number, String::new()),
        Err(_) => (true, 0, stem.into_owned()),
    }
}

fn invalid_data(message: impl Into<String>) -> Box<dyn Error> {
    Box::new(io::Error::new(io::ErrorKind::InvalidData, message.into()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn detects_phase_from_repeated_anchor() {
        let mut anchor = RgbaImage::new(TILE_SIZE, TILE_SIZE);
        anchor.put_pixel(2, 3, Rgba([255, 0, 0, 255]));
        let mut screenshot = RgbaImage::new(30, 25);
        imageops::replace(&mut screenshot, &anchor, 3, 5);
        imageops::replace(&mut screenshot, &anchor, 11, 13);

        assert_eq!(
            detect_grid_phase(&screenshot, &anchor).unwrap(),
            GridPhase {
                x: 3,
                y: 5,
                anchor_matches: 2,
            }
        );
    }

    #[test]
    fn trims_only_the_outer_void() {
        let rows = vec![
            vec![' ', ' ', ' ', ' '],
            vec![' ', '#', '#', ' '],
            vec![' ', '#', ' ', ' '],
        ];
        let trimmed = trim_void(rows).unwrap();
        assert_eq!(trimmed.rows, vec![vec!['#', '#'], vec!['#', ' ']]);
        assert_eq!((trimmed.width, trimmed.height), (2, 2));
        assert_eq!((trimmed.offset_x, trimmed.offset_y), (1, 1));
    }

    #[test]
    fn reproduces_all_authoritative_levels() {
        let mut atlas = Atlas::load(Path::new("data/tile-templates")).unwrap();
        let fixtures = [
            (
                "data/level-screenshots/1.png",
                "data/level-manual-labels/1.txt",
            ),
            (
                "data/level-screenshots/2.png",
                "data/level-manual-labels/2.txt",
            ),
        ];

        for (screenshot_path, labels_path) in fixtures {
            let screenshot = load_png(Path::new(screenshot_path)).unwrap();
            let labels = fs::read_to_string(labels_path).unwrap();
            atlas.learn_labeled_level(&screenshot, &labels).unwrap();
        }

        for (screenshot_path, labels_path) in fixtures {
            let screenshot = load_png(Path::new(screenshot_path)).unwrap();
            let labels = fs::read_to_string(labels_path).unwrap();
            let imported = import_level(&screenshot, &atlas).unwrap();

            assert_eq!(imported.map, labels.trim_end(), "fixture {labels_path}");
            assert!(
                imported.unknown_tiles.is_empty(),
                "fixture {labels_path} still has unknown tiles"
            );
        }
    }
}
