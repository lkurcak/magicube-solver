# Tile template atlas

`atlas.txt` maps an output character to each 8x8 PNG template. Multiple templates
may map to the same character because sprites can animate or overdraw neighboring
cells. The special character `_` represents a space (semantic void).

Exactly one template must start with `anchor`. The importer searches for exact
copies of that distinctive tile to find the grid alignment of every screenshot.

Unknown cells are written to `data/levels/unknown-tiles/`. To teach the importer a
new tile, add an authoritative manual crop to `data/tiles-manual/` and reference
it from `atlas.txt`, or label the corresponding cell in a manual level map.

Before conversion, every `data/level-manual-labels/<name>.txt` is paired with its
level screenshot and learned as authoritative training data. Explicit spaces in a
labeled line are meaningful; omitted trailing spaces are not trained.
