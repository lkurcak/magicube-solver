# Tile template atlas

`atlas.txt` maps an output character to each 8x8 PNG template. Multiple templates
may map to the same character because sprites can animate or overdraw neighboring
cells. The special character `_` represents a space (semantic void).

Exact template matches take priority. Otherwise, the importer tolerates particles
or similar screenshot noise by accepting the uniquely nearest template when at
most three of the crop's 64 RGB pixels differ. Black background participates as
a space template. Larger differences and equal-distance matches for different
characters remain unresolved and are reported as `?`.

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
