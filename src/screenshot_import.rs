use image::{Rgba, RgbaImage, imageops};
use std::collections::BTreeMap;
use std::error::Error;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const TILE_SIZE: u32 = 8;
const MAX_NOISE_PIXELS: usize = 3;
/// The "LEVEL" caption of the in-game HUD in its 3x5 pixel font.
const HUD_CAPTION: [&str; 5] = [
    "#...###.#.#.###.#",
    "#...#...#.#.#...#",
    "#...##..#.#.##..#",
    "#...#...###.#...#",
    "###.###..#..###.###",
];
const HUD_GLYPH_ADVANCE: u32 = 4;
const HUD_MAX_DIGITS: u32 = 3;
/// The HUD's black box extends past the caption by this many pixels.
const HUD_PADDING_RIGHT: u32 = 3;
const HUD_PADDING_BOTTOM: u32 = 1;
/// What a HUD-covered cell is assumed to be when its visible pixels are inconclusive.
const HIDDEN_SYMBOL: char = '#';
/// Atlas-only symbol for the empty cell above an unpressed red pressure plate.
/// Its red plate top is the only difference from an ordinary plate, whose base
/// sprite is identical, so the marker turns the `P` below it into `R`.
const RED_PLATE_MARKER: char = '~';

#[derive(Debug)]
pub struct Atlas {
    templates: Vec<Template>,
    anchor_index: usize,
    template_paths: Vec<PathBuf>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrainingSummary {
    pub grid_offset_x: usize,
    pub grid_offset_y: usize,
    pub anchor_matches: usize,
    pub learned_variants: usize,
    /// Labels of cells covered by the HUD. They describe only this screenshot,
    /// so they are not learned as templates.
    pub occluded_labels: Vec<OccludedLabel>,
}

/// A manual label for a HUD-covered cell, in screenshot grid coordinates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OccludedLabel {
    pub grid_x: usize,
    pub grid_y: usize,
    pub symbol: char,
    pub label_x: usize,
    pub label_y: usize,
}

/// Candidate symbols for a HUD-covered tile, judged by its visible pixels.
#[derive(Debug, Clone, PartialEq, Eq)]
enum VisibleMatch {
    Unique(char),
    Ambiguous(Vec<char>),
    None,
}

/// A tile that is only partly visible, and what hides the rest of it.
#[derive(Debug, Clone, PartialEq, Eq)]
struct PartialTile {
    visible: Vec<bool>,
    cause: Occluder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Occluder {
    /// The level HUD covers part of the tile.
    Hud,
    /// The tile extends past the screenshot's edge, and may also be under the HUD.
    ScreenshotEdge,
}

/// Inclusive pixel bounds of the HUD's black box.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Occlusion {
    right: u32,
    bottom: u32,
}

#[derive(Debug)]
pub struct UnknownTile {
    pub hash: u64,
    pub image: RgbaImage,
    pub positions: Vec<(usize, usize)>,
    /// Why this visual pattern could not be assigned a unique symbol.
    pub reason: String,
}

impl Atlas {
    pub fn load(directory: &Path) -> Result<Self, Box<dyn Error>> {
        let manifest_path = directory.join("atlas.txt");
        let manifest = fs::read_to_string(&manifest_path)
            .map_err(|error| invalid_data(format!("{}: {error}", manifest_path.display())))?;
        let mut templates = Vec::new();
        let mut anchor_index = None;
        let mut template_paths = Vec::new();

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
            if !(is_label_symbol(symbol) || symbol == RED_PLATE_MARKER) || symbol == '?' {
                return Err(invalid_data(format!(
                    "{}:{}: unsupported tile symbol {symbol:?}",
                    manifest_path.display(),
                    line_index + 1
                )));
            }
            let template_path = directory.join(filename);
            let image = load_png(&template_path)?;
            require_tile_size(&image, &template_path)?;
            if is_anchor && anchor_index.replace(templates.len()).is_some() {
                return Err(invalid_data("atlas contains more than one anchor template"));
            }
            templates.push(Template {
                symbol,
                name: template_path.display().to_string(),
                image,
            });
            template_paths.push(template_path);
        }

        if templates.is_empty() {
            return Err(invalid_data("atlas has no templates"));
        }
        let anchor_index =
            anchor_index.ok_or_else(|| invalid_data("atlas has no anchor template"))?;
        Ok(Self {
            templates,
            anchor_index,
            template_paths,
        })
    }

    /// Files referenced by the atlas, including templates outside its directory.
    pub fn template_paths(&self) -> &[PathBuf] {
        &self.template_paths
    }

    pub fn anchor_name(&self) -> &str {
        &self.templates[self.anchor_index].name
    }

    fn anchor(&self) -> &RgbaImage {
        &self.templates[self.anchor_index].image
    }

    fn classify(&self, tile: &RgbaImage) -> Result<Option<char>, String> {
        let matches = self
            .templates
            .iter()
            .filter(|template| template.image.as_raw() == tile.as_raw())
            .collect::<Vec<_>>();
        let void = is_void(tile);
        if let Some(first) = matches.first()
            && (matches
                .iter()
                .any(|template| template.symbol != first.symbol)
                || (void && first.symbol != ' '))
        {
            let mut sources = matches
                .iter()
                .map(|template| format!("{:?} from {}", template.symbol, template.name))
                .collect::<Vec<_>>();
            if void {
                sources.push("' ' from the black-background rule".to_owned());
            }
            return Err(format!("conflicting labels: {}", sources.join("; ")));
        }
        if let Some(first) = matches.first() {
            return Ok(Some(first.symbol));
        }
        if void {
            return Ok(Some(' '));
        }

        let mut nearest_distance = MAX_NOISE_PIXELS + 1;
        let mut nearest = Vec::new();
        for template in &self.templates {
            let distance = pixel_distance(tile, &template.image, MAX_NOISE_PIXELS);
            if distance < nearest_distance {
                nearest_distance = distance;
                nearest.clear();
                nearest.push((template.symbol, template.name.as_str()));
            } else if distance == nearest_distance {
                nearest.push((template.symbol, template.name.as_str()));
            }
        }

        let black_distance = non_black_pixel_count(tile, MAX_NOISE_PIXELS);
        if black_distance < nearest_distance {
            nearest_distance = black_distance;
            nearest.clear();
            nearest.push((' ', "the black-background rule"));
        } else if black_distance == nearest_distance {
            nearest.push((' ', "the black-background rule"));
        }

        if nearest_distance > MAX_NOISE_PIXELS {
            return Ok(None);
        }
        let symbol = nearest[0].0;
        if nearest.iter().all(|candidate| candidate.0 == symbol) {
            return Ok(Some(symbol));
        }

        let sources = nearest
            .into_iter()
            .map(|(symbol, source)| format!("{symbol:?} from {source}"))
            .collect::<Vec<_>>();
        Err(format!(
            "ambiguous noise-tolerant match at {nearest_distance} changed pixels: {}",
            sources.join("; ")
        ))
    }

    /// Match only the pixels outside the HUD. With nothing visible, every
    /// template matches.
    fn classify_occluded(&self, tile: &RgbaImage, visible: &[bool]) -> VisibleMatch {
        let visible_equal = |template: &RgbaImage| {
            tile.pixels()
                .zip(template.pixels())
                .zip(visible)
                .all(|((left, right), &visible)| !visible || left.0[..3] == right.0[..3])
        };
        let mut symbols = self
            .templates
            .iter()
            .filter(|template| visible_equal(&template.image))
            .map(|template| template.symbol)
            .collect::<Vec<_>>();
        if tile
            .pixels()
            .zip(visible)
            .all(|(pixel, &visible)| !visible || pixel.0[..3] == [0, 0, 0])
        {
            symbols.push(' ');
        }
        symbols.sort_unstable();
        symbols.dedup();
        match symbols[..] {
            [] => VisibleMatch::None,
            [symbol] => VisibleMatch::Unique(symbol),
            _ => VisibleMatch::Ambiguous(symbols),
        }
    }

    pub fn learn_labeled_level(
        &mut self,
        image: &RgbaImage,
        labels: &str,
    ) -> Result<TrainingSummary, Box<dyn Error>> {
        self.learn_labeled_level_from(image, labels, "<labels>")
    }

    /// Learn authoritative examples, retaining their source for conflict reports.
    /// Validation and placement finish before any examples are added.
    pub fn learn_labeled_level_from(
        &mut self,
        image: &RgbaImage,
        labels: &str,
        source: &str,
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
                        let tile = crop_tile(image, xs[offset_x + x], ys[offset_y + y]);
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

        let occlusions = detect_hud_occlusions(image);
        let mut learned_variants = 0;
        let mut occluded_labels = Vec::new();
        for (y, row) in rows.iter().enumerate() {
            for (x, &symbol) in row.iter().enumerate() {
                if symbol == '?' {
                    continue;
                }
                let (grid_x, grid_y) = (offset_x + x, offset_y + y);
                if tile_visibility(image, xs[grid_x], ys[grid_y], &occlusions).is_some() {
                    occluded_labels.push(OccludedLabel {
                        grid_x,
                        grid_y,
                        symbol,
                        label_x: x,
                        label_y: y,
                    });
                    continue;
                }
                let tile = crop_tile(image, xs[grid_x], ys[grid_y]);
                // An unpressed red base looks like an ordinary base; the empty
                // cell above it carries the red marker instead.
                let symbol = match symbol {
                    'R' if self.classify(&tile) == Ok(Some('P')) => continue,
                    ' ' if rows.get(y + 1).and_then(|row| row.get(x)) == Some(&'R') => {
                        RED_PLATE_MARKER
                    }
                    symbol => symbol,
                };
                if !self.templates.iter().any(|template| {
                    template.image.as_raw() == tile.as_raw() && template.symbol == symbol
                }) {
                    self.templates.push(Template {
                        symbol,
                        name: format!("{source} at ({x}, {y})"),
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
            occluded_labels,
        })
    }
}

pub fn load_png(path: &Path) -> Result<RgbaImage, Box<dyn Error>> {
    image::open(path)
        .map(|image| image.to_rgba8())
        .map_err(|error| invalid_data(format!("{}: {error}", path.display())))
}

pub fn import_level(image: &RgbaImage, atlas: &Atlas) -> Result<ImportedLevel, Box<dyn Error>> {
    import_level_with_labels(image, atlas, &[])
}

/// Import a screenshot, resolving HUD-covered cells from their visible pixels
/// and, where those are inconclusive, from this screenshot's manual labels.
pub fn import_level_with_labels(
    image: &RgbaImage,
    atlas: &Atlas,
    occluded_labels: &[OccludedLabel],
) -> Result<ImportedLevel, Box<dyn Error>> {
    let phase = detect_grid_phase(image, atlas.anchor())?;
    let occlusions = detect_hud_occlusions(image);
    let mut rows = Vec::new();
    let mut unknown: BTreeMap<u64, UnknownTile> = BTreeMap::new();

    let ys = tile_origins(phase.y, image.height());
    let xs = tile_origins(phase.x, image.width());
    for (grid_y, &pixel_y) in ys.iter().enumerate() {
        let mut row = Vec::new();
        for (grid_x, &pixel_x) in xs.iter().enumerate() {
            let tile = crop_tile(image, pixel_x, pixel_y);
            let classification = match tile_visibility(image, pixel_x, pixel_y, &occlusions) {
                None => atlas.classify(&tile),
                Some(partial) => {
                    let label = occluded_labels
                        .iter()
                        .find(|label| (label.grid_x, label.grid_y) == (grid_x, grid_y));
                    resolve_occluded(
                        atlas.classify_occluded(&tile, &partial.visible),
                        label,
                        partial.cause,
                    )
                }
            };
            let symbol = if let Ok(Some(symbol)) = classification {
                symbol
            } else {
                let reason = classification
                    .err()
                    .unwrap_or_else(|| "unrecognized tile".to_owned());
                let hash = tile_hash(&tile);
                unknown
                    .entry(hash)
                    .or_insert_with(|| UnknownTile {
                        hash,
                        image: tile.clone(),
                        positions: Vec::new(),
                        reason,
                    })
                    .positions
                    .push((grid_x, grid_y));
                '?'
            };
            row.push(symbol);
        }
        rows.push(row);
    }
    resolve_red_plate_markers(&mut rows, &mut unknown, |grid_x, grid_y| {
        crop_tile(image, xs[grid_x], ys[grid_y])
    });

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

/// Each red-plate marker becomes empty space and marks the base below it red.
/// A marker without a base below is reported as unrecognized.
fn resolve_red_plate_markers(
    rows: &mut [Vec<char>],
    unknown: &mut BTreeMap<u64, UnknownTile>,
    crop: impl Fn(usize, usize) -> RgbaImage,
) {
    for y in 0..rows.len() {
        for x in 0..rows[y].len() {
            if rows[y][x] != RED_PLATE_MARKER {
                continue;
            }
            let below = rows.get_mut(y + 1).and_then(|row| row.get_mut(x));
            if let Some(base @ ('P' | 'R')) = below {
                *base = 'R';
                rows[y][x] = ' ';
                continue;
            }
            rows[y][x] = '?';
            let tile = crop(x, y);
            let hash = tile_hash(&tile);
            unknown
                .entry(hash)
                .or_insert_with(|| UnknownTile {
                    hash,
                    image: tile,
                    positions: Vec::new(),
                    reason: "red pressure-plate top without a pressure-plate base below".to_owned(),
                })
                .positions
                .push((x, y));
        }
    }
}

/// Labels take precedence over guesses. Cells the visible pixels cannot decide
/// are assumed to be walls under the HUD, as long as a wall fits what is visible.
/// Cells cut off by the screenshot edge are assumed to be void, because
/// screenshots crop only the black background around the level.
fn resolve_occluded(
    visible_match: VisibleMatch,
    label: Option<&OccludedLabel>,
    cause: Occluder,
) -> Result<Option<char>, String> {
    let (cause, fallbacks): (_, &[char]) = match cause {
        Occluder::Hud => ("occluded by the level HUD", &[HIDDEN_SYMBOL]),
        Occluder::ScreenshotEdge => ("cut off by the screenshot edge", &[' ', HIDDEN_SYMBOL]),
    };
    match (visible_match, label) {
        (VisibleMatch::Unique(symbol), Some(label)) if symbol != label.symbol => Err(format!(
            "{cause}: visible pixels match {symbol:?}, but the label at ({}, {}) is {:?}",
            label.label_x, label.label_y, label.symbol
        )),
        (VisibleMatch::Unique(symbol), _) => Ok(Some(symbol)),
        (_, Some(label)) => Ok(Some(label.symbol)),
        (VisibleMatch::Ambiguous(symbols), None)
            if let Some(&fallback) = fallbacks.iter().find(|symbol| symbols.contains(symbol)) =>
        {
            Ok(Some(fallback))
        }
        (VisibleMatch::Ambiguous(symbols), None) => Err(format!(
            "{cause}: visible pixels match {}; label this cell",
            symbols
                .iter()
                .map(|symbol| format!("{symbol:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        )),
        (VisibleMatch::None, None) => Err(format!(
            "{cause}: visible pixels match no template; label this cell"
        )),
    }
}

/// Find the black boxes drawn behind "LEVEL <number>" captions.
fn detect_hud_occlusions(image: &RgbaImage) -> Vec<Occlusion> {
    let caption_width = HUD_CAPTION.iter().map(|row| row.len()).max().unwrap() as u32;
    let caption_height = HUD_CAPTION.len() as u32;
    if image.width() < caption_width || image.height() < caption_height {
        return Vec::new();
    }

    let mut occlusions = Vec::new();
    for y in 0..=image.height() - caption_height {
        for x in 0..=image.width() - caption_width {
            let ink = *image.get_pixel(x, y);
            let is_caption = (0..caption_height).all(|dy| {
                let row = HUD_CAPTION[dy as usize].as_bytes();
                (0..caption_width).all(|dx| {
                    let pixel = image.get_pixel(x + dx, y + dy);
                    if row.get(dx as usize) == Some(&b'#') {
                        *pixel == ink
                    } else {
                        pixel.0[..3] == [0, 0, 0]
                    }
                })
            });
            if !is_caption || ink.0[..3] == [0, 0, 0] {
                continue;
            }

            // Digits follow one blank glyph. Count glyphs drawn purely in ink on black.
            let digits_x = x + caption_width + 1 + HUD_GLYPH_ADVANCE;
            let digits = (0..HUD_MAX_DIGITS)
                .take_while(|digit| {
                    let glyph_x = digits_x + digit * HUD_GLYPH_ADVANCE;
                    let pixels = (0..caption_height)
                        .flat_map(|dy| {
                            (0..HUD_GLYPH_ADVANCE - 1).map(move |dx| (glyph_x + dx, y + dy))
                        })
                        .filter(|&(px, py)| px < image.width() && py < image.height())
                        .map(|(px, py)| *image.get_pixel(px, py))
                        .collect::<Vec<_>>();
                    pixels.len() == ((HUD_GLYPH_ADVANCE - 1) * caption_height) as usize
                        && pixels.contains(&ink)
                        && pixels
                            .iter()
                            .all(|pixel| *pixel == ink || pixel.0[..3] == [0, 0, 0])
                })
                .count() as u32;
            let text_right = if digits == 0 {
                x + caption_width - 1
            } else {
                digits_x + digits * HUD_GLYPH_ADVANCE - 2
            };
            occlusions.push(Occlusion {
                right: text_right + HUD_PADDING_RIGHT,
                bottom: y + caption_height - 1 + HUD_PADDING_BOTTOM,
            });
        }
    }
    occlusions
}

/// Row-major visibility of a tile's pixels, or `None` when the whole tile is
/// inside the screenshot and nothing covers it.
fn tile_visibility(
    image: &RgbaImage,
    pixel_x: i64,
    pixel_y: i64,
    occlusions: &[Occlusion],
) -> Option<PartialTile> {
    let mut cut_off = false;
    let visible = (0..TILE_SIZE as i64)
        .flat_map(|dy| (0..TILE_SIZE as i64).map(move |dx| (pixel_x + dx, pixel_y + dy)))
        .map(|(x, y)| {
            let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
                cut_off = true;
                return false;
            };
            if x >= image.width() || y >= image.height() {
                cut_off = true;
                return false;
            }
            !occlusions
                .iter()
                .any(|occlusion| x <= occlusion.right && y <= occlusion.bottom)
        })
        .collect::<Vec<_>>();
    let cause = if cut_off {
        Occluder::ScreenshotEdge
    } else {
        Occluder::Hud
    };
    visible
        .contains(&false)
        .then_some(PartialTile { visible, cause })
}

/// The tile at a possibly out-of-bounds origin, with off-screenshot pixels black.
fn crop_tile(image: &RgbaImage, pixel_x: i64, pixel_y: i64) -> RgbaImage {
    let mut tile = RgbaImage::from_pixel(TILE_SIZE, TILE_SIZE, Rgba([0, 0, 0, 255]));
    imageops::replace(&mut tile, image, -pixel_x, -pixel_y);
    tile
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
    let ties = counts
        .iter()
        .flatten()
        .filter(|&&count| count == best.anchor_matches)
        .count();
    if ties > 1 {
        return Err(invalid_data(format!(
            "ambiguous grid alignment: {ties} phases tied with {} anchor matches",
            best.anchor_matches
        )));
    }
    Ok(best)
}

pub fn png_files(directory: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    files_with_extension(directory, "png")
}

/// Files directly in `directory` with the given extension, in natural stem order.
pub fn files_with_extension(
    directory: &Path,
    extension: &str,
) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let mut paths: Vec<_> = fs::read_dir(directory)?
        .collect::<Result<Vec<_>, _>>()?
        .into_iter()
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|found| found == extension))
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
    let mut index = String::from("hash\tfile\tpositions\treason\n");
    for tile in tiles {
        let filename = format!("{:016x}.png", tile.hash);
        tile.image.save(directory.join(&filename))?;
        let positions = tile
            .positions
            .iter()
            .map(|(x, y)| format!("{x},{y}"))
            .collect::<Vec<_>>()
            .join(" ");
        index.push_str(&format!(
            "{:016x}\t{filename}\t{positions}\t{}\n",
            tile.hash,
            tile.reason.replace(['\t', '\n', '\r'], " ")
        ));
    }
    fs::write(directory.join(format!("{level_name}.tsv")), index)?;
    Ok(())
}

/// Origins of every grid cell with at least one pixel inside the screenshot,
/// including cells cut off by its edges.
fn tile_origins(phase: u32, image_size: u32) -> Vec<i64> {
    let first = if phase == 0 {
        0
    } else {
        i64::from(phase) - i64::from(TILE_SIZE)
    };
    (first..i64::from(image_size))
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
    for (y, row) in rows.iter().enumerate() {
        for (x, &symbol) in row.iter().enumerate() {
            if !is_label_symbol(symbol) {
                return Err(invalid_data(format!(
                    "unsupported tile symbol {symbol:?} at ({x}, {y})"
                )));
            }
        }
    }
    Ok(rows)
}

fn is_label_symbol(symbol: char) -> bool {
    matches!(
        symbol,
        ' ' | '#'
            | 'D'
            | 'P'
            | 'R'
            | 'G'
            | 'S'
            | 't'
            | '?'
            | 'C'
            | 'g'
            | 'r'
            | 'b'
            | 'O'
            | '@'
            | '{'
            | '}'
            | '^'
            | 'v'
            | 'T'
    )
}

fn is_void(tile: &RgbaImage) -> bool {
    tile.pixels().all(|pixel| pixel.0[..3] == [0, 0, 0])
}

fn pixel_distance(left: &RgbaImage, right: &RgbaImage, limit: usize) -> usize {
    let mut distance = 0;
    for (left, right) in left.pixels().zip(right.pixels()) {
        if left.0[..3] != right.0[..3] {
            distance += 1;
            if distance > limit {
                break;
            }
        }
    }
    distance
}

fn non_black_pixel_count(tile: &RgbaImage, limit: usize) -> usize {
    let mut count = 0;
    for pixel in tile.pixels() {
        if pixel.0[..3] != [0, 0, 0] {
            count += 1;
            if count > limit {
                break;
            }
        }
    }
    count
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
    fn rejects_tied_grid_phases_and_missing_anchors() {
        let anchor = test_tile(1);
        let mut screenshot = RgbaImage::new(32, 16);
        imageops::replace(&mut screenshot, &anchor, 0, 0);
        imageops::replace(&mut screenshot, &anchor, 17, 8);
        assert!(
            detect_grid_phase(&screenshot, &anchor)
                .unwrap_err()
                .to_string()
                .contains("2 phases tied")
        );
        assert!(detect_grid_phase(&RgbaImage::new(16, 16), &anchor).is_err());
    }

    fn test_tile(seed: u8) -> RgbaImage {
        RgbaImage::from_fn(TILE_SIZE, TILE_SIZE, |x, y| {
            Rgba([seed, x as u8 + 1, y as u8 + 1, 255])
        })
    }

    fn test_atlas() -> Atlas {
        Atlas {
            templates: ['#', '@', 'G']
                .into_iter()
                .enumerate()
                .map(|(index, symbol)| Template {
                    symbol,
                    name: format!("{symbol}.png"),
                    image: test_tile(index as u8 + 1),
                })
                .collect(),
            anchor_index: 0,
            template_paths: Vec::new(),
        }
    }

    fn test_screenshot() -> RgbaImage {
        let mut screenshot = RgbaImage::new(32, 8);
        for i in 0..4 {
            imageops::replace(&mut screenshot, &test_tile(i + 1), i64::from(i) * 8, 0);
        }
        screenshot
    }

    /// Row 0 holds void, '@', '#', an unknown tile and two walls; row 1 is
    /// all walls. A "LEVEL 88" HUD box then covers pixels x <= 34, y <= 6.
    fn hud_screenshot() -> RgbaImage {
        let mut screenshot = RgbaImage::new(48, 16);
        for (x, seed) in [None, Some(2), Some(1), Some(9), Some(1), Some(1)]
            .into_iter()
            .enumerate()
        {
            if let Some(seed) = seed {
                imageops::replace(&mut screenshot, &test_tile(seed), x as i64 * 8, 0);
            }
            imageops::replace(&mut screenshot, &test_tile(1), x as i64 * 8, 8);
        }
        let ink = Rgba([255, 241, 232, 255]);
        for y in 0_u32..=6 {
            for x in 0_u32..=34 {
                let caption = HUD_CAPTION
                    .get(y.wrapping_sub(1) as usize)
                    .and_then(|row| row.as_bytes().get(x.wrapping_sub(1) as usize));
                let digit = (1..=5).contains(&y) && matches!(x, 25..=27 | 29..=31);
                let pixel = if caption == Some(&b'#') || digit {
                    ink
                } else {
                    Rgba([0, 0, 0, 255])
                };
                screenshot.put_pixel(x, y, pixel);
            }
        }
        screenshot
    }

    #[test]
    fn detects_the_hud_box_from_its_caption_and_digits() {
        assert_eq!(
            detect_hud_occlusions(&hud_screenshot()),
            [Occlusion {
                right: 34,
                bottom: 6,
            }]
        );
        assert!(detect_hud_occlusions(&test_screenshot()).is_empty());
    }

    #[test]
    fn hud_covered_cells_are_matched_by_their_visible_pixels() {
        let imported = import_level(&hud_screenshot(), &test_atlas()).unwrap();
        assert_eq!(imported.map, " @#?##\n######");
        assert_eq!(imported.unknown_tiles.len(), 1);
        assert_eq!(imported.unknown_tiles[0].positions, [(3, 0)]);
        assert!(
            imported.unknown_tiles[0]
                .reason
                .contains("occluded by the level HUD")
        );
    }

    #[test]
    fn inconclusive_hud_covered_cells_are_assumed_to_be_walls() {
        let atlas = test_atlas();
        let hidden = atlas.classify_occluded(&test_tile(9), &[false; 64]);
        assert_eq!(resolve_occluded(hidden, None, Occluder::Hud), Ok(Some('#')));

        let without_walls = Atlas {
            templates: atlas.templates[1..]
                .iter()
                .map(|template| Template {
                    symbol: template.symbol,
                    name: template.name.clone(),
                    image: template.image.clone(),
                })
                .collect(),
            anchor_index: 0,
            template_paths: Vec::new(),
        };
        let hidden = without_walls.classify_occluded(&test_tile(9), &[false; 64]);
        assert!(resolve_occluded(hidden, None, Occluder::Hud).is_err());
    }

    #[test]
    fn labels_resolve_hud_covered_cells_without_training_the_atlas() {
        let mut atlas = test_atlas();
        let screenshot = hud_screenshot();
        let summary = atlas
            .learn_labeled_level_from(&screenshot, " @#t##\n######", "hud.txt")
            .unwrap();
        assert_eq!(summary.learned_variants, 0);
        assert_eq!(summary.occluded_labels.len(), 5);
        assert_eq!(
            import_level(&screenshot, &atlas).unwrap().map,
            " @#?##\n######"
        );
        let imported =
            import_level_with_labels(&screenshot, &atlas, &summary.occluded_labels).unwrap();
        assert_eq!(imported.map, " @#t##\n######");
        assert!(imported.unknown_tiles.is_empty());

        let summary = atlas
            .learn_labeled_level_from(&screenshot, "#@#t##\n######", "wrong.txt")
            .unwrap();
        let imported =
            import_level_with_labels(&screenshot, &atlas, &summary.occluded_labels).unwrap();
        assert_eq!(imported.map, "?@#t##\n######");
        assert!(
            imported.unknown_tiles[0]
                .reason
                .contains("label at (0, 0) is '#'")
        );
    }

    /// '#', '@', 'G' and '#' with the first wall one pixel past the left edge,
    /// followed by two pixels of black background.
    fn cropped_screenshot() -> RgbaImage {
        let mut screenshot = RgbaImage::new(33, 8);
        for (x, seed) in [1, 2, 3, 1].into_iter().enumerate() {
            imageops::replace(&mut screenshot, &test_tile(seed), x as i64 * 8 - 1, 0);
        }
        screenshot
    }

    #[test]
    fn cells_cut_off_by_the_screenshot_edge_are_matched_by_their_visible_pixels() {
        let imported = import_level(&cropped_screenshot(), &test_atlas()).unwrap();
        assert_eq!(imported.phase.x, 7);
        assert_eq!(imported.map, "#@G#");
        assert!(imported.unknown_tiles.is_empty());
    }

    #[test]
    fn inconclusive_cells_cut_off_by_the_edge_are_assumed_to_be_void() {
        let atlas = test_atlas();
        let hidden = atlas.classify_occluded(&RgbaImage::new(8, 8), &[false; 64]);
        assert_eq!(
            resolve_occluded(hidden, None, Occluder::ScreenshotEdge),
            Ok(Some(' '))
        );
    }

    #[test]
    fn labels_resolve_cells_cut_off_by_the_edge_without_training_the_atlas() {
        let mut atlas = test_atlas();
        let mut screenshot = cropped_screenshot();
        imageops::replace(&mut screenshot, &test_tile(9), -1, 0);
        assert_eq!(import_level(&screenshot, &atlas).unwrap().map, "?@G#");

        let summary = atlas
            .learn_labeled_level_from(&screenshot, "t@G#", "cropped.txt")
            .unwrap();
        assert_eq!(summary.learned_variants, 0);
        assert_eq!(summary.occluded_labels.len(), 1);
        let imported =
            import_level_with_labels(&screenshot, &atlas, &summary.occluded_labels).unwrap();
        assert_eq!(imported.map, "t@G#");
    }

    /// Walls, an ordinary plate base, and a red plate top, laid out as rows.
    fn red_plate_screenshot(rows: &[&[u8]]) -> RgbaImage {
        let mut screenshot = RgbaImage::new(8 * rows[0].len() as u32, 8 * rows.len() as u32);
        for (y, row) in rows.iter().enumerate() {
            for (x, &seed) in row.iter().enumerate() {
                if seed != 0 {
                    imageops::replace(
                        &mut screenshot,
                        &test_tile(seed),
                        x as i64 * 8,
                        y as i64 * 8,
                    );
                }
            }
        }
        screenshot
    }

    fn red_plate_atlas() -> Atlas {
        let mut atlas = test_atlas();
        for (symbol, seed) in [('P', 4), (RED_PLATE_MARKER, 5)] {
            atlas.templates.push(Template {
                symbol,
                name: format!("{symbol}.png"),
                image: test_tile(seed),
            });
        }
        atlas
    }

    #[test]
    fn red_plate_tops_turn_the_base_below_red() {
        let screenshot = red_plate_screenshot(&[&[1, 5, 1, 5], &[1, 4, 1, 1]]);
        let imported = import_level(&screenshot, &red_plate_atlas()).unwrap();
        assert_eq!(imported.map, "# #?\n#R##");
        assert_eq!(imported.unknown_tiles.len(), 1);
        assert_eq!(imported.unknown_tiles[0].positions, [(3, 0)]);
        assert!(
            imported.unknown_tiles[0]
                .reason
                .contains("without a pressure-plate base below")
        );
    }

    #[test]
    fn red_plate_labels_teach_the_top_and_keep_the_shared_base_ordinary() {
        let mut atlas = red_plate_atlas();
        atlas.templates.pop();
        let screenshot = red_plate_screenshot(&[&[1, 5, 1], &[1, 4, 1]]);
        let summary = atlas
            .learn_labeled_level_from(&screenshot, "# #\n#R#", "red.txt")
            .unwrap();
        assert_eq!(summary.learned_variants, 1);
        assert_eq!(import_level(&screenshot, &atlas).unwrap().map, "# #\n#R#");

        let ordinary = red_plate_screenshot(&[&[1, 0, 1], &[1, 4, 1]]);
        assert_eq!(import_level(&ordinary, &atlas).unwrap().map, "# #\n#P#");
    }

    #[test]
    fn teaching_one_new_pattern_resolves_all_its_occurrences() {
        let mut atlas = test_atlas();
        let screenshot = test_screenshot();
        let mut repeated = RgbaImage::new(48, 8);
        imageops::replace(&mut repeated, &screenshot, 8, 0);
        imageops::replace(&mut repeated, &test_tile(4), 40, 0);
        let imported = import_level(&repeated, &atlas).unwrap();
        assert_eq!(imported.map, "#@G??");
        assert_eq!(imported.unknown_tiles.len(), 1);
        assert_eq!(imported.unknown_tiles[0].positions, [(3, 0), (4, 0)]);
        atlas
            .learn_labeled_level_from(&screenshot, "#@Gt", "example.txt")
            .unwrap();
        let imported = import_level(&repeated, &atlas).unwrap();
        assert_eq!(imported.map, "#@Gtt");
        assert!(imported.unknown_tiles.is_empty());
    }

    #[test]
    fn tolerates_up_to_three_noisy_pixels_on_any_tile() {
        let atlas = test_atlas();
        for template in &atlas.templates {
            for noise_pixels in 1..=MAX_NOISE_PIXELS {
                let mut tile = template.image.clone();
                for x in 0..noise_pixels as u32 {
                    tile.put_pixel(x, 0, Rgba([255, 0, 255, 255]));
                }
                assert_eq!(atlas.classify(&tile).unwrap(), Some(template.symbol));
            }
        }
    }

    #[test]
    fn noisy_black_background_does_not_expand_the_level() {
        let atlas = test_atlas();
        let mut screenshot = RgbaImage::new(24, 16);
        for x in 0..MAX_NOISE_PIXELS as u32 {
            screenshot.put_pixel(x, 0, Rgba([255, 163, 0, 255]));
        }
        for (x, seed) in [1, 2, 3].into_iter().enumerate() {
            imageops::replace(
                &mut screenshot,
                &test_tile(seed),
                x as i64 * i64::from(TILE_SIZE),
                i64::from(TILE_SIZE),
            );
        }

        let imported = import_level(&screenshot, &atlas).unwrap();
        assert_eq!(imported.map, "#@G");
        assert_eq!((imported.width, imported.height), (3, 1));
        assert!(imported.unknown_tiles.is_empty());
    }

    #[test]
    fn rejects_excessive_or_ambiguous_pixel_differences() {
        let atlas = test_atlas();
        let mut screenshot = test_screenshot();
        for x in 0..=MAX_NOISE_PIXELS as u32 {
            screenshot.put_pixel(TILE_SIZE + x, 0, Rgba([255, 0, 255, 255]));
        }
        assert_eq!(import_level(&screenshot, &atlas).unwrap().map, "#?G?");

        let base = RgbaImage::new(TILE_SIZE, TILE_SIZE);
        let mut alternate = base.clone();
        alternate.put_pixel(0, 0, Rgba([1, 0, 0, 0]));
        alternate.put_pixel(1, 0, Rgba([1, 0, 0, 0]));
        let ambiguous_atlas = Atlas {
            templates: vec![
                Template {
                    symbol: '#',
                    name: "wall.png".to_owned(),
                    image: base,
                },
                Template {
                    symbol: '@',
                    name: "player.png".to_owned(),
                    image: alternate,
                },
            ],
            anchor_index: 0,
            template_paths: Vec::new(),
        };
        let mut candidate = RgbaImage::new(TILE_SIZE, TILE_SIZE);
        candidate.put_pixel(0, 0, Rgba([1, 0, 0, 0]));
        let error = ambiguous_atlas.classify(&candidate).unwrap_err();
        assert!(error.contains("ambiguous noise-tolerant match"));
        assert!(error.contains("wall.png"));
        assert!(error.contains("player.png"));
    }

    #[test]
    fn conflicting_examples_are_unresolved_regardless_of_training_order() {
        let screenshot = test_screenshot();
        for examples in [
            [("#@Gt", "torch.txt"), ("#@GS", "skull.txt")],
            [("#@GS", "skull.txt"), ("#@Gt", "torch.txt")],
        ] {
            let mut atlas = test_atlas();
            for (labels, source) in examples {
                atlas
                    .learn_labeled_level_from(&screenshot, labels, source)
                    .unwrap();
            }
            let imported = import_level(&screenshot, &atlas).unwrap();
            assert_eq!(imported.map, "#@G?");
            let tile = &imported.unknown_tiles[0];
            assert_eq!(tile.positions, [(3, 0)]);
            assert!(tile.reason.contains("conflicting labels"));
            assert!(tile.reason.contains("torch.txt at (3, 0)"));
            assert!(tile.reason.contains("skull.txt at (3, 0)"));
        }
    }

    #[test]
    fn manual_labels_cannot_silently_override_the_atlas_or_black_background() {
        let mut atlas = test_atlas();
        let mut screenshot = test_screenshot();
        imageops::replace(&mut screenshot, &RgbaImage::new(8, 8), 24, 0);
        atlas
            .learn_labeled_level_from(&screenshot, "#CGt", "wrong.txt")
            .unwrap();
        let imported = import_level(&screenshot, &atlas).unwrap();
        assert_eq!(imported.map, "#?G?");
        assert_eq!(imported.unknown_tiles.len(), 2);
        assert!(
            imported
                .unknown_tiles
                .iter()
                .any(|tile| tile.reason.contains("black-background"))
        );
        assert!(
            imported
                .unknown_tiles
                .iter()
                .any(|tile| tile.reason.contains("@.png"))
        );
    }

    #[test]
    fn invalid_or_unplaceable_labels_do_not_partially_train_the_atlas() {
        let mut atlas = test_atlas();
        let screenshot = test_screenshot();
        for labels in ["#CG!", "#@Gtt", "@Gt"] {
            assert!(atlas.learn_labeled_level(&screenshot, labels).is_err());
            assert_eq!(import_level(&screenshot, &atlas).unwrap().map, "#@G?");
        }
        let mut repeated = RgbaImage::new(40, 8);
        imageops::replace(&mut repeated, &screenshot, 0, 0);
        imageops::replace(&mut repeated, &test_tile(1), 32, 0);
        assert!(
            atlas
                .learn_labeled_level(&repeated, "#")
                .unwrap_err()
                .to_string()
                .contains("placements tied")
        );
    }
}
