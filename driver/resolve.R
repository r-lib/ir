# ir resolve driver
#
# Run by the `ir` Rust binary in a private, throw-away R session.
#
#   Rscript --vanilla resolve.R <script_path> <cache_dir> <out_file>
#
# Responsibilities (steps 1-4 of the `ir` pipeline):
#   1. Parse the commented YAML frontmatter of <script_path> with yaml12.
#   2. Resolve the declared dependencies into concrete versions with pak.
#   3. Hash the resolved set to derive a content-addressed library path
#      under <cache_dir>.
#   4. Materialise that path as a light-weight library of symlinks into
#      renv's package cache via renv::use().
#
# The resulting library path is written to <out_file> for the Rust process
# to pick up. This session then exits; the Rust process launches the user's
# script in a fresh, isolated R session pointed at the library.
#
# The helpers below are pure and side-effect free so they can be unit tested
# (see tests/test-resolve.R). The pipeline runs only when this file is executed
# as a script -- `sys.nframe() == 0L` is false when the file is sourced.

`%||%` <- function(x, y) if (is.null(x)) y else x

## --- frontmatter parsing ----------------------------------------------------

# Extract the YAML frontmatter text from a script's lines: drop a leading
# shebang, take the leading contiguous block of `#` comments, and strip the
# `#` (plus one optional space) from each.
ir_frontmatter <- function(lines) {
  if (length(lines) && grepl("^#!", lines[[1L]]))
    lines <- lines[-1L]
  is_comment <- grepl("^[[:space:]]*#", lines)
  stop_at <- which(!is_comment)
  block <- if (length(stop_at)) lines[seq_len(stop_at[[1L]] - 1L)] else lines
  paste(sub("^[[:space:]]*#[[:space:]]?", "", block), collapse = "\n")
}

# Parse frontmatter text into a spec list. A non-mapping result (e.g. a prose
# comment that parses to a scalar) is treated as an absent header, but invalid
# YAML is an error.
ir_read_spec <- function(yaml_text) {
  spec <- tryCatch(
    if (nzchar(yaml_text)) yaml12::parse_yaml(yaml_text) else list(),
    error = function(e)
      stop(sprintf("could not parse script frontmatter as YAML: %s",
                   conditionMessage(e)), call. = FALSE)
  )
  if (is.list(spec)) spec else list()
}

# The declared dependency specs. Accepts both a YAML list (`- dplyr`) and a
# whitespace-separated scalar (`dplyr>=1.0 tidyr`); package refs are expected
# to be whitespace-free.
ir_deps <- function(spec) {
  deps <- as.character(spec$dependencies %||% character())
  deps <- as.character(unlist(strsplit(trimws(deps), "[[:space:]]+")))
  deps[nzchar(deps)]
}

# Soft-check the optional `R:` version constraint against the running R; warn
# on a mismatch but never stop (this prototype does not select R versions).
ir_check_r_version <- function(spec, current = getRversion()) {
  if (is.null(spec$R)) return(invisible())
  req <- trimws(as.character(spec$R)[[1L]])
  m <- regmatches(req, regexec("^(>=|>|<=|<|==)?[[:space:]]*([0-9][0-9.-]*)$", req))[[1L]]
  if (length(m) == 3L) {
    op <- if (nzchar(m[[2L]])) m[[2L]] else ">="
    ok <- do.call(op, list(current, numeric_version(m[[3L]])))
    if (!isTRUE(ok))
      warning(sprintf("script requests R %s but running R %s", req, current),
              call. = FALSE, immediate. = TRUE)
  }
  invisible()
}

## --- pak ref normalisation --------------------------------------------------

# Translate one dependency spec into a pak package reference:
#   `pkg`         -> `pkg`         (latest)
#   `pkg>=1.0`    -> `pkg@>=1.0`   (lower bound; solver picks)
#   `pkg==1.0`    -> `pkg@1.0`     (exact version)
# Native pak refs, GitHub refs, and URL refs are passed through untouched.
# Unsupported version operators such as `pkg<=1.2` are also passed to pak
# unchanged, so pak remains the source of truth for supported refs.
ir_to_ref <- function(d) {
  d <- trimws(d)
  m <- regmatches(d, regexec(
    "^([A-Za-z][A-Za-z0-9.]*[A-Za-z0-9])[[:space:]]*(>=|==)[[:space:]]*([0-9][0-9.-]*)$",
    d
  ))[[1L]]
  if (length(m) != 4L) return(d)
  if (m[[3L]] == ">=") sprintf("%s@>=%s", m[[2L]], m[[4L]])
  else sprintf("%s@%s", m[[2L]], m[[4L]])
}

## --- cache location ---------------------------------------------------------

# The cache root: the standard per-package user cache directory, overridable
# with IR_CACHE_DIR. Holds `libraries/` (materialised libraries) and
# `resolutions/` (the daily resolution cache).
ir_cache_dir <- function() {
  env <- Sys.getenv("IR_CACHE_DIR")
  if (nzchar(env)) env else tools::R_user_dir("ir", "cache")
}

## --- resolution cache -------------------------------------------------------

# Key identifying a resolution request: the declared dependency specs (order
# independent), the day, and the R version / platform. Including the date forces
# a fresh resolution -- and so picks up newly published versions -- at most once
# per day; until then an identical request reuses its previous result without
# invoking pak. Order independent so reordering deps doesn't bust the cache.
ir_input_key <- function(deps,
                         date     = Sys.Date(),
                         rversion = getRversion(),
                         platform = R.version$platform) {
  secretbase::sha256(paste(c(sort(deps),
                             as.character(date),
                             as.character(rversion),
                             platform),
                           collapse = "\n"))
}

## --- pipeline ---------------------------------------------------------------

ir_resolve_main <- function() {

  args <- commandArgs(trailingOnly = TRUE)
  if (length(args) < 2L)
    stop("usage: resolve.R <script_path> <out_file>", call. = FALSE)
  script_path <- args[[1L]]
  out_file    <- args[[2L]]
  cache_dir   <- ir_cache_dir()

  # We run with --vanilla, so no repository is configured. Fall back to a
  # canonical CRAN mirror when the session has none.
  repos <- getOption("repos")
  cran  <- if (!is.null(repos)) repos[["CRAN"]] else NULL
  if (is.null(cran) || is.na(cran) || !nzchar(cran) || identical(cran, "@CRAN@")) {
    repos <- c(CRAN = "https://cran.r-project.org")
    options(repos = repos)
  }

  ## 1. Parse frontmatter
  spec <- ir_read_spec(ir_frontmatter(readLines(script_path, warn = FALSE)))
  deps <- ir_deps(spec)
  ir_check_r_version(spec)

  ## 1b. Resolution cache: if this exact request was resolved earlier today and
  ## its library still exists, reuse it and skip pak entirely. The marker is
  ## written only after a successful materialise (below), so its presence
  ## implies a complete library.
  marker <- file.path(cache_dir, "resolutions", ir_input_key(deps))
  if (file.exists(marker)) {
    cached <- readLines(marker, n = 1L, warn = FALSE)
    if (length(cached) && nzchar(cached) && dir.exists(cached)) {
      writeLines(cached, out_file)
      return(invisible())
    }
  }

  ## 2. Resolve with pak
  # A script may legitimately declare no dependencies; it then gets an empty
  # but still isolated library (base R only), so undeclared library() calls
  # fail loudly instead of silently borrowing the user's packages.
  if (length(deps)) {
    refs_in <- vapply(deps, ir_to_ref, character(1L), USE.NAMES = FALSE)
    res <- pak::pkg_deps(refs_in, dependencies = NA, upgrade = TRUE)

    failed <- res[res$status != "OK", , drop = FALSE]
    if (nrow(failed))
      stop("pak could not resolve: ",
           paste(failed$ref, collapse = ", "), call. = FALSE)

    # Drop base / recommended packages: those are supplied by R itself.
    keep <- is.na(res$priority) | !(res$priority %in% c("base", "recommended"))
    res <- res[keep, , drop = FALSE]

    pkgs     <- res$package
    resolved <- sort(unique(sprintf("%s@%s", res$package, res$version)))
  } else {
    pkgs     <- character()
    resolved <- character()
  }

  ## 3. Hash the resolved set -> content-addressed library path
  # Bind the hash to the R version and platform: the symlinks point into the
  # renv cache, whose layout is itself keyed by R version and platform.
  key <- paste(c(resolved,
                 as.character(getRversion()),
                 R.version$platform),
               collapse = "\n")
  library_path <- file.path(cache_dir, "libraries", secretbase::sha256(key))

  ## 4. Materialise the symlinked library via renv::use()
  # Skip when the library already holds every resolved package: repeat runs of
  # an unchanged script then cost nothing beyond resolution.
  dir.create(library_path, recursive = TRUE, showWarnings = FALSE)
  have <- list.files(library_path)
  if (length(pkgs) && !all(pkgs %in% have)) {
    # renv::use() installs into the renv cache and links the packages into
    # `library` as symlinks. Because `library` lives in our cache (not the R
    # temp dir), renv leaves it in place when the session ends.
    do.call(renv::use, c(
      as.list(resolved),
      list(
        library = library_path,
        repos   = repos,
        attach  = FALSE,
        sandbox = FALSE,
        isolate = TRUE,
        verbose = TRUE
      )
    ))
  }

  ## 4b. Record the resolution so an identical request today skips pak.
  dir.create(dirname(marker), recursive = TRUE, showWarnings = FALSE)
  writeLines(library_path, marker)

  writeLines(library_path, out_file)
  invisible()
}

if (sys.nframe() == 0L) ir_resolve_main()
