# Agent Map

`ir` is a Rust CLI for self-describing R scripts and Quarto documents. Flow:
parse CLI -> read source metadata -> resolve packages with the embedded R driver
-> materialize a cached library -> launch R or Quarto.

## Source Of Truth

- User behavior: `README.md`, `docs/*.qmd`, and help snapshots in
  `tests/snapshots/`.
- Public CLI coverage: `tests/cli.rs`; fixtures live in `tests/fixtures/run/`.
- Resolver behavior: Rust orchestration in `src/runtime.rs`; R implementation in
  `driver/resolve.R`.
- Release/install surfaces: `scripts/` and `.github/workflows/`.

## Rust Map

- `src/main.rs`: entrypoint and top-level routing only.
- `src/cli.rs`: clap definitions and argument scanning.
- `src/script.rs`: R source detection and R script frontmatter extraction.
- `src/spec.rs`: runtime metadata model and shared YAML frontmatter parsing.
- `src/runtime.rs`: Rscript selection, dependency resolution, cache roots, and
  user code execution.
- `src/tool.rs`: `ir tool run/install`, executable discovery, and launcher
  generation.
- `src/cache.rs`: `ir cache` commands.
- `src/resolve_cache.rs`: resolution cache keys and marker reads.
- `src/rig.rs`: R version selection through `rig`.
- `src/quarto.rs`: Quarto detection, source frontmatter extraction, and rendering.
- `src/bin/rx.rs`: `rx` shim into `ir tool rx`.

## Conventions

- Keep `main.rs` small; place behavior in the owning module.
- Preserve public CLI behavior unless tests and snapshots change intentionally.
- Prefer direct, actionable errors over fallback chains when invoking R, `rig`,
  or Quarto.
- Keep this file a compact map; put durable details in README, docs, or tests.
