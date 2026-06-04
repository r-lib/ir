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

# Optional date-bounded resolution. `exclude-newer` is a YAML mapping key whose
# value is an ISO date; resolution then uses that day's Posit Package Manager
# CRAN snapshot instead of the latest CRAN repository.
ir_exclude_newer <- function(value) {
  if (is.null(value)) return(NULL)

  value <- trimws(as.character(value)[[1L]])
  if (!grepl("^[0-9]{4}-[0-9]{2}-[0-9]{2}$", value))
    stop("`exclude-newer` must be a date string in YYYY-MM-DD format",
         call. = FALSE)

  value
}

# Optional per-package Suggests resolution. `--with-suggests` packages arrive as
# a comma-separated env value; this splits them into the bare package names whose
# Suggests should join the resolution. Order independent and de-duplicated.
ir_with_suggests <- function(value) {
  if (is.null(value)) return(character())
  parts <- trimws(strsplit(value, ",", fixed = TRUE)[[1L]])
  unique(parts[nzchar(parts)])
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

# The bare package name from a dependency spec, used to match `--with-suggests`
# entries against resolved package names. Strips a trailing version constraint
# (`dplyr>=1.0`) or pak version suffix (`dplyr@1.0`). Other ref forms such as
# `r-lib/dplyr` are left for the resolved-set check to reject with a clear
# message, so `--with-suggests` takes a package name.
ir_pkg_name <- function(spec) {
  sub("^[[:space:]]*([A-Za-z][A-Za-z0-9.]*).*$", "\\1", spec)
}

# Suggests package names declared by `pkg`, read from a pak::pkg_deps() result.
# Returns NULL when `pkg` is absent from the resolved set (the caller treats that
# as an error) and character() when present but suggesting nothing. Dependency
# `type`s in pak's `deps` column are lower case; "R" is not an installable
# package.
ir_suggested_packages <- function(res, pkg) {
  row <- res[res$package == pkg, , drop = FALSE]
  if (!nrow(row)) return(NULL)
  deps <- row$deps[[1L]]
  if (is.null(deps) || !nrow(deps)) return(character())
  suggested <- deps$package[tolower(deps$type) == "suggests"]
  setdiff(unique(suggested), "R")
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

ir_ppm_snapshot_url <- function(exclude_newer) {
  sprintf("https://packagemanager.posit.co/cran/%s", exclude_newer)
}

ir_repos <- function(exclude_newer = NULL, repos = getOption("repos")) {
  if (!is.null(exclude_newer))
    return(c(CRAN = ir_ppm_snapshot_url(exclude_newer)))

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
                         exclude_newer = NULL,
                         with_suggests = character()) {
  source_key <- if (is.null(exclude_newer))
    as.character(date)
  else
    sprintf("exclude-newer: %s", exclude_newer)

  # Pulling a package's Suggests yields a different closure for the same declared
  # deps, so it gets its own resolution marker. Omitted when empty, leaving the
  # key identical to a no-suggests request and reusing existing cache entries.
  suggests_key <- if (length(with_suggests))
    sprintf("with-suggests: %s",
            paste(sort(unique(with_suggests)), collapse = ","))
  else
    NULL

  secretbase::sha256(paste(c(sort(deps),
                             source_key,
                             as.character(rversion),
                             platform,
                             suggests_key),
                           collapse = "\n"))
}

## --- pipeline ---------------------------------------------------------------

ir_resolve_main <- function() {

  deps        <- readLines(file("stdin"), warn = FALSE)
  result_file <- ir_env_optional("IR_RESOLVE_RESULT_FILE")
  package_result_file <- ir_env_optional("IR_RESOLVE_PACKAGE_RESULT_FILE")
  stopifnot(!is.null(result_file))
  cache_dir   <- ir_cache_dir()

  ## 1. Consume inputs parsed by Rust from script frontmatter
  exclude_newer <- ir_exclude_newer(ir_env_optional("IR_EXCLUDE_NEWER"))
  repos <- ir_repos(exclude_newer)
  options(repos = repos)

  # Packages whose Suggests should join the closure (from `--with-suggests`),
  # reduced to bare package names so they match the resolved set.
  suggest_names <- unique(vapply(ir_with_suggests(ir_env_optional("IR_WITH_SUGGESTS")),
                                 ir_pkg_name, character(1L), USE.NAMES = FALSE))

  ## 1b. Resolution cache: if this exact request was resolved already and its
  ## library still exists, reuse it and skip pak entirely. The marker is written
  ## only after a successful materialise (below), so its presence implies a
  ## complete library.
  primary_ref <- if (length(deps)) ir_to_ref(deps[[1L]]) else NULL
  marker <- file.path(cache_dir, "resolutions",
                      ir_input_key(deps, exclude_newer = exclude_newer,
                                   with_suggests = suggest_names))
  package_marker <- if (!is.null(primary_ref)) {
    file.path(cache_dir, "resolutions",
              paste0(basename(marker), "-primary-", secretbase::sha256(primary_ref)))
  } else {
    NULL
  }
  if (file.exists(marker)) {
    cached <- readLines(marker, n = 1L, warn = FALSE)
    if (length(cached) && nzchar(cached) && dir.exists(cached)) {
      if (!is.null(package_result_file) &&
          (is.null(package_marker) || !file.exists(package_marker))) {
        # The library is reusable, but this caller needs primary-package
        # metadata that older cache entries did not record.
      } else {
        writeLines(cached, result_file)
        if (!is.null(package_result_file)) {
          package <- readLines(package_marker, n = 1L, warn = FALSE)
          writeLines(package, package_result_file)
        }
        return(invisible())
      }
    }
  }

  ## 2. Resolve with pak
  # A script may legitimately declare no dependencies; it then gets an empty
  # but still isolated library (base R only), so undeclared library() calls
  # fail loudly instead of silently borrowing the user's packages.
  primary_package <- NULL
  if (length(deps)) {
    refs_in <- vapply(deps, ir_to_ref, character(1L), USE.NAMES = FALSE)
    res <- pak::pkg_deps(refs_in, dependencies = NA, upgrade = TRUE)

    failed <- res[res$status != "OK", , drop = FALSE]
    if (nrow(failed))
      stop("pak could not resolve: ",
           paste(failed$ref, collapse = ", "), call. = FALSE)

    if (!is.null(package_result_file)) {
      primary <- unique(res$package[res$direct & res$ref == refs_in[[1L]]])
      if (length(primary) != 1L)
        stop("package ref must resolve to exactly one R package: ",
             deps[[1L]], call. = FALSE)
      primary_package <- primary[[1L]]
    }

    # Per-package Suggests: promote the Suggests of the requested packages to
    # direct refs and re-solve, so just those packages' suggested dependencies
    # (and the hard dependencies they pull) join the closure. Unlike pak's global
    # `dependencies = TRUE`, this does not add every direct package's Suggests.
    if (length(suggest_names)) {
      missing <- setdiff(suggest_names, res$package)
      if (length(missing))
        stop("`--with-suggests` package(s) not among resolved dependencies: ",
             paste(missing, collapse = ", "),
             " (declare them, or add with `--with`)", call. = FALSE)
      extra <- unique(unlist(lapply(suggest_names,
                                    function(p) ir_suggested_packages(res, p)),
                             use.names = FALSE))
      # Skip Suggests already resolved, and those absent from the configured
      # repos -- as a direct ref an unavailable Suggests would fail the whole
      # solve, whereas a soft dependency is meant to be skipped when unavailable.
      extra <- setdiff(extra, res$package)
      if (length(extra)) {
        available <- tryCatch(rownames(available.packages(repos = repos, type = "source")),
                              error = function(e) character())
        extra <- intersect(extra, available)
      }
      if (length(extra)) {
        res <- pak::pkg_deps(c(refs_in, extra), dependencies = NA, upgrade = TRUE)
        failed <- res[res$status != "OK", , drop = FALSE]
        if (nrow(failed))
          stop("pak could not resolve suggested dependencies: ",
               paste(failed$ref, collapse = ", "), call. = FALSE)
      }
    }

    # Drop base / recommended packages: those are supplied by R itself.
    keep <- is.na(res$priority) | !(res$priority %in% c("base", "recommended"))
    res <- res[keep, , drop = FALSE]

    pkgs     <- res$package
    resolved <- sort(unique(sprintf("%s@%s", res$package, res$version)))
  } else {
    pkgs     <- character()
    resolved <- character()
    # `--with-suggests` augments an existing dependency, so with nothing declared
    # there is nothing for it to attach to. Reject it for the same reason the
    # resolution path does, rather than silently ignoring the request.
    if (length(suggest_names))
      stop("`--with-suggests` package(s) not among resolved dependencies: ",
           paste(suggest_names, collapse = ", "),
           " (declare them, or add with `--with`)", call. = FALSE)
    if (!is.null(package_result_file))
      stop("cannot resolve a primary package without dependencies",
           call. = FALSE)
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
  if (!is.null(primary_package)) {
    writeLines(primary_package, package_marker)
  }

  writeLines(library_path, result_file)
  if (!is.null(package_result_file)) {
    writeLines(primary_package, package_result_file)
  }
  invisible()
}

if (sys.nframe() == 0L) ir_resolve_main()
