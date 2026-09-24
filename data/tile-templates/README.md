# Tile template atlas

`atlas.txt` maps an output character to each 8x8 PNG template. Multiple templates
may map to the same character because sprites can animate or overdraw neighboring
cells. The special character `_` represents a space (semantic void).

Exact template matches take priority. Otherwise, the importer tolerates particles
or similar screenshot noise by accepting the uniquely nearest template when at
most three of the crop's 64 RGB pixels differ. Black background participates as
a space template. Larger differences and equal-distance matches for different
characters remain unresolved and are reported as `?`.

An unpressed red pressure-plate base is pixel-identical to an ordinary one; only
the red plate drawn in the empty cell above tells them apart. Templates of that
cell use the atlas-only character `~`. After recognition, each `~` becomes empty
space and turns the `P` below it into `R`; a `~` with no base below is reported
as unrecognized. A pressed red base shows the red plate itself and maps to `R`
directly. In manual labels, write `R` for the base and a space above it: an `R`
label on a tile the atlas already knows as `P` is not learned, and a space label
directly above an `R` is learned as `~`.

Exactly one template must start with `anchor`. The importer searches for exact
copies of that distinctive tile to find the grid alignment of every screenshot.

Running `cargo run --bin screenshots-to-maps` exports unresolved tile crops to
`cache/levels/unknown-tiles/` and diagnostics to `cache/levels/import-report.txt`.
Each level's TSV index lists crop filenames, zero-based map coordinates, and the
reason recognition failed. To teach the importer a
new tile, add an authoritative manual crop to `data/tiles-manual/` and reference
it from `atlas.txt`, or label the corresponding cell in a manual level map.

Before conversion, every screenshot is paired with
`data/level-manual-labels/<name>.txt` when present. All matching labels are learned
before any level is imported, so one new example can repair multiple levels.
Explicit spaces in a labeled line are meaningful; `?` and omitted trailing spaces
are not trained. A label file without a matching screenshot is not used.

Some screenshots include the in-game `LEVEL <number>` caption, whose black box
hides the tiles beneath it. The importer finds the caption by its pixel font and
treats every cell touching the box as occluded. Occluded cells are matched using
only their visible pixels and resolve when all matching templates agree on one
character. If the visible pixels are inconclusive but consistent with a wall, the
cell is assumed to be a wall; if they match no template, it stays unresolved. A
label in that level's manual map takes precedence over the wall assumption. Labels
of occluded cells apply only to their own screenshot and are never learned as
templates; a label that contradicts the visible pixels is reported.

The atlas and manual level labels are both authoritative. If identical pixels
have conflicting labels, the importer emits `?` and reports the conflicting
source files and label coordinates instead of choosing one. Black background is
empty space; a contradictory label is also reported. Resolve the source labels
to fix the conflict. Tied grid phases or ambiguous placement of a manual map are
reported as errors, rather than silently picking a placement.

Keep authored examples here and in `data/level-manual-labels/`; generated maps
remain inferred output and are never automatically fed back into training. The
game needs only the resolved maps and corruption status, not tile provenance.

The player runs this same import process in its build script. Add screenshots and
rebuild to include them; exporting diagnostic crops is only necessary when you
need to repair an import. Clean entries are playable, while corrupted entries
remain visible for preview. No generated maps need to be committed.
