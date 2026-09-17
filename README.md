# Magicube solver

A work-in-progress solver for Magicube, including a small computer-vision tool
that converts pixel-perfect level screenshots into character tile maps.

The importer uses 8x8 image templates and detects each screenshot's grid phase
independently. Manually labeled levels are authoritative training fixtures;
unknown visual variants are emitted as `?` for later labeling.

## Usage

Generate maps under the ignored `data/levels/` directory:

```sh
cargo run --bin screenshots-to-maps
```

Run the solver prototype:

```sh
cargo run --bin magicube-solver
```

Run the test suite, including exact reproduction of all authoritative levels:

```sh
cargo test --workspace
```

See [`data/tile-templates/README.md`](data/tile-templates/README.md) for the
template and manual-label workflow.
