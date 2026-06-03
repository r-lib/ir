# Design: `ir run` for Quarto documents

**Date:** 2026-06-03
**Status:** Approved, pending implementation plan

## Goal

Extend `ir run` so it renders standalone Quarto documents the same way it runs
standalone R scripts today: `ir run doc.qmd` resolves the document's declared
dependencies into a cached, isolated package library and runs `quarto render`
against that library and the selected R.

Dependencies are declared in the document's YAML frontmatter under an `ir:`
key, reusing the resolver's existing vocabulary:

```yaml
---
ir:
  dependencies:
    - dplyr>=1.0
    - gt@1.0
  R: ">= 4.6"
  exclude after: "2024-01-15"
---
```

## Background: how `ir run script.R` works today

Two phases (see `README.md`):

1. **Resolve + materialise** (private R session). Rust extracts the frontmatter,
   the dependencies are resolved with pak into concrete versions, hashed into a
   content-addressed library path under the cache dir, and materialised as a
   light-weight library of symlinks into renv's cache.
2. **Run** (ordinary R session). The script runs as `Rscript script.R` with
   `R_LIBS` set to the materialised library, which prepends it to `.libPaths()`.

The R binary `ir` uses is "the selected Rscript" — today `IR_RSCRIPT` or
`Rscript` on PATH. R-version *selection* is not implemented; `R:` is only a
soft check inside the resolver.

### Built on PR #14

This design assumes the architecture of open PR #14
(`t-kalinowski/ir`, branch `move-yaml-parsing-to-rust`), which moves YAML
parsing from R into Rust:

- Rust parses the frontmatter with `saphyr` into
  `ScriptSpec { dependencies, exclude_after, r_requirement }`
  (`parse_frontmatter`, helpers `frontmatter_dependencies` /
  `frontmatter_optional_string`).
- `resolve.R` no longer parses YAML. It receives dependency specs on **stdin**
  (one per line) plus `IR_EXCLUDE_AFTER`, `IR_R_REQUIREMENT`, and
  `IR_RESOLVE_RESULT_FILE` environment variables.
- Version-operator translation (`dplyr>=1.0` → pak ref) stays in `resolve.R`,
  fed from those stdin lines.

Because of #14, the qmd flow produces the **identical** stdin + env inputs that
a script produces. `resolve.R` is untouched by this work.

## How Quarto consumes the selected R and library

Verified against quarto-cli source (`src/core/resources.ts`, `src/execute/rmd.ts`)
and triangulated with deepwiki and quarto-web docs:

- **R binary selection** (`resources.ts:100-164`): resolution order is
  `QUARTO_R` → `R_HOME` → PATH → Windows registry → Program Files. Setting
  `QUARTO_R` pins the R quarto's knitr engine uses. It accepts either an
  `Rscript` file path or its `bin` directory.
- **Library path** (`rmd.ts:440`): quarto spawns `Rscript` via `execProcess`,
  inheriting the parent environment. `R_LIBS` set in `ir`'s process passes
  through to that R subprocess and prepends `.libPaths()` — the same mechanism
  `ir run script.R` already uses. No quarto-specific library configuration is
  needed.

**Invariant:** `QUARTO_R` must be the exact Rscript `ir` resolved/materialised
the library against. The library is content-addressed by resolved versions + R
version + platform; using a different R for rendering would not match it.

## Architecture

Rust-only changes. **Zero changes to `resolve.R`.** Phase 1 (resolve +
materialise) is unchanged. Phase 2 dispatches by file extension.

### Components

1. **`ScriptSpec` model — reused unchanged.** The qmd `ir:` block maps to the
   same three fields (`dependencies`, `exclude_after` from `exclude after`,
   `r_requirement` from `R`).

2. **Frontmatter source, dispatched by extension.** Keep
   `read_op_frontmatter_to_string` (the `#| ` line reader) for `.R`. Add a
   reader that captures the leading `---` … `---` YAML block for `.qmd` / `.Rmd`.
   The reader is chosen by the script's extension (case-insensitive).

3. **`parse_frontmatter` gains a nested path.** For qmd, the spec mapping node
   is `doc["ir"]` rather than the top-level document. Since
   `frontmatter_dependencies` and `frontmatter_optional_string` already accept a
   `&Yaml` node, they are handed the `ir:` sub-node. An absent or null `ir:`
   key yields `ScriptSpec::default()` (no dependencies). All other quarto keys
   (`title`, `format`, …) are ignored for free.

4. **Phase-2 dispatch in `cmd_run`.** By extension: `.R` → existing
   `run_script`; `.qmd` / `.Rmd` → new `run_quarto`; any other extension → a
   clear error. Phase 1 (`resolve_library`) runs identically for both.

5. **`run_quarto`.** Locate `quarto` on PATH. Build
   `quarto render <doc> <passthrough args>`. Set `QUARTO_R=<selected Rscript>`
   and, when dependencies resolved, `R_LIBS=<materialised library>`. Use the
   same platform split as `run_script` (exec on Unix, spawn + status on
   Windows). Propagate the exit code.

### Selected-Rscript seam

`ir` already has one notion of "the Rscript to run against" (today `IR_RSCRIPT`
or PATH `Rscript`; future: a colleague's rig integration). It feeds: phase-1
resolve, the `.R` run (`R_LIBS` + exec), and the qmd run (becomes `QUARTO_R`).
Keeping a single source enforces the invariant above. R-version *selection*
itself is out of scope — this work only plumbs the chosen Rscript to `QUARTO_R`.

## Data flow (qmd)

```
doc.qmd
  → extract leading `---` … `---` block (Rust)
  → parse_frontmatter, descend into `ir:` → ScriptSpec
  → deps on stdin + IR_EXCLUDE_AFTER + IR_R_REQUIREMENT → resolve.R
  → resolve + materialise content-addressed library → library path
  → run_quarto: QUARTO_R=<rscript>, R_LIBS=<library>
  → quarto render doc.qmd
  → quarto knitr spawns QUARTO_R Rscript, inherits R_LIBS
  → .libPaths() prepended → document renders
```

## Error handling and edge cases

- **No `ir:` key / no dependencies** → `R_LIBS` is not set; quarto renders with
  the ambient R. Parallels a no-dependency script.
- **`quarto` not on PATH** → clear error, surfaced before phase 1 work.
- **Document inside an renv-activated project** → the document's `.Rprofile`
  can re-set `.libPaths()` and shadow `R_LIBS`. Known limitation; `ir` targets
  *standalone* documents, the same standalone assumption made for scripts.
  Documented, not solved here.

## Passthrough arguments

`ir run doc.qmd --to pdf` → `quarto render doc.qmd --to pdf`. Extra arguments
are appended to the render command, mirroring how `script_args` already flow to
a script.

## Testing

- **Unit:** `---`-block frontmatter extraction; `parse_frontmatter` descent into
  `ir:` (present, absent, null); extension dispatch.
- **Integration:** a `.qmd` declaring `ir: { dependencies }` renders, and a
  package present only in the resolved library is usable from an R chunk during
  the render. Reuse the existing test harness in `tests/cli.rs`.

## Out of scope

- R-version *selection* (separate rig integration; this work only carries the
  selected Rscript to `QUARTO_R`).
- Quarto verbs other than `render` (e.g. `preview`).
- An `IR_QUARTO` override for locating quarto (PATH only for now; add later only
  if needed).
- Jupyter / `.ipynb` documents (Python engine).

## Dev workflow / sequencing

- PR #14 is **open, not merged**. Base the implementation branch on #14's branch
  (`move-yaml-parsing-to-rust`), or land after #14 merges. Building on current
  `main` would mean reworking against the old R-side-parse code.
- Push to the appropriate remote, open a PR against `main` (or #14's branch
  while it is open), and link the work to the relevant tracking issue.

## Rejected alternative

Parse the `---` block and extract the `ir:` subtree, but keep YAML descent in
R. Rejected: after #14 there is no YAML parser left in `resolve.R`. Re-adding
one to descend into `ir:` would duplicate parsing and diverge from #14's
direction. Rust-side descent reuses #14's helpers and leaves `resolve.R`
untouched.

## Naming note

`ScriptSpec` / `read_script_spec` read awkwardly once documents are involved.
Recommendation: keep #14's names to minimise diff against an open PR. A
`RunSpec` rename is optional churn that can be skipped.
