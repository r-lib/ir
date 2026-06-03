# ir resolve driver
#
# Run by the `ir` Rust binary in a private, throw-away R session.
#
#   IR_RESOLVE_RESULT_FILE=<result_file> Rscript resolve.R
#
# Responsibilities (steps 1-4 of the `ir` pipeline):
#   1. Consume package dependency specs from stdin, one dependency per line.
#   2. Resolve the declared dependencies into concrete versions with pak.
#   3. Hash the resolved set to derive a content-addressed library path
#      under <cache_dir>.
#   4. Materialise that path as a light-weight library of symlinks into
#      renv's package cache via renv::use().
#
# The resulting library path is written to the temp result file named by
# IR_RESOLVE_RESULT_FILE. stdout/stderr stay available for pak progress.
# This session then exits; the Rust process launches the user's script in a
# fresh, isolated R session pointed at the library.
#
# The helpers below are pure and side-effect free so they can be unit tested
# (see tests/test-resolve.R). The pipeline runs only when this file is executed
# as a script -- `sys.nframe() == 0L` is false when the file is sourced.

## --- resolver input ---------------------------------------------------------

ir_env_optional <- function(name) {
  value <- Sys.getenv(name, unset = NA_character_)
  if (is.na(value) || !nzchar(value)) NULL else value
}

# Optional date-bounded resolution. `exclude after` is a YAML mapping key whose
# value is an ISO date; resolution then uses that day's Posit Package Manager
# CRAN snapshot instead of the latest CRAN repository.
ir_exclude_after <- function(value) {
  if (is.null(value)) return(NULL)

  value <- trimws(as.character(value)[[1L]])
  if (!grepl("^[0-9]{4}-[0-9]{2}-[0-9]{2}$", value))
    stop("`exclude after` must be a date string in YYYY-MM-DD format",
         call. = FALSE)

  date <- as.Date(value, format = "%Y-%m-%d")
  if (is.na(date) || !identical(format(date, "%Y-%m-%d"), value))
    stop("`exclude after` must be a date string in YYYY-MM-DD format",
         call. = FALSE)

  value
}

# Soft-check the optional `R:` version constraint against the running R; warn
# on a mismatch but never stop (this prototype does not select R versions).
ir_check_r_version <- function(req = NULL, current = getRversion()) {
  if (is.null(req)) return(invisible())
  req <- trimws(as.character(req)[[1L]])
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
# `resolutions/` (the resolution request cache).
ir_cache_dir <- function() {
  env <- Sys.getenv("IR_CACHE_DIR")
  if (nzchar(env)) env else tools::R_user_dir("ir", "cache")
}

## --- repositories -----------------------------------------------------------

ir_ppm_snapshot_url <- function(exclude_after) {
  sprintf("https://packagemanager.posit.co/cran/%s", exclude_after)
}

ir_repos <- function(exclude_after = NULL, repos = getOption("repos")) {
  if (!is.null(exclude_after))
    return(c(CRAN = ir_ppm_snapshot_url(exclude_after)))

  cran <- if (!is.null(repos)) repos[["CRAN"]] else NULL
  if (is.null(cran) || is.na(cran) || !nzchar(cran) || identical(cran, "@CRAN@"))
    c(CRAN = "https://cran.r-project.org")
  else
    repos
}

## --- resolution cache -------------------------------------------------------

# Key identifying a resolution request: the declared dependency specs (order
# independent), the resolution source, and the R version / platform. Latest
# resolution includes the current day so newly published versions are picked up
# at most once per day. Dated PPM snapshot resolution uses only the snapshot date
# because that repository state is immutable. Order independent so reordering
# deps doesn't bust the cache.
ir_input_key <- function(deps,
                         date          = Sys.Date(),
                         rversion      = getRversion(),
                         platform      = R.version$platform,
                         exclude_after = NULL) {
  source_key <- if (is.null(exclude_after))
    as.character(date)
  else
    sprintf("exclude after: %s", exclude_after)

  secretbase::sha256(paste(c(sort(deps),
                             source_key,
                             as.character(rversion),
                             platform),
                           collapse = "\n"))
}

## --- pipeline ---------------------------------------------------------------

ir_resolve_main <- function() {

  deps        <- readLines(stdin(), warn = FALSE)
  result_file <- ir_env_optional("IR_RESOLVE_RESULT_FILE")
  stopifnot(!is.null(result_file))
  cache_dir   <- ir_cache_dir()

  ## 1. Consume inputs parsed by Rust from script frontmatter
  exclude_after <- ir_exclude_after(ir_env_optional("IR_EXCLUDE_AFTER"))
  ir_check_r_version(ir_env_optional("IR_R_REQUIREMENT"))
  repos <- ir_repos(exclude_after)
  options(repos = repos)

  ## 1b. Resolution cache: if this exact request was resolved already and its
  ## library still exists, reuse it and skip pak entirely. The marker is written
  ## only after a successful materialise (below), so its presence implies a
  ## complete library.
  marker <- file.path(cache_dir, "resolutions",
                      ir_input_key(deps, exclude_after = exclude_after))
  if (file.exists(marker)) {
    cached <- readLines(marker, n = 1L, warn = FALSE)
    if (length(cached) && nzchar(cached) && dir.exists(cached)) {
      writeLines(cached, result_file)
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

  ## 4b. Record the resolution so an identical request skips pak.
  dir.create(dirname(marker), recursive = TRUE, showWarnings = FALSE)
  writeLines(library_path, marker)

  writeLines(library_path, result_file)
  invisible()
}

if (sys.nframe() == 0L) ir_resolve_main()
