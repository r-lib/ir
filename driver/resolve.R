# ir resolve driver
#
# Run by the `ir` Rust binary in a private, throw-away R session.
#
#   IR_RESOLVE_RESULT_FILE=<result_file> Rscript resolve.R
#
# Responsibilities (steps 1-4 of the `ir` pipeline):
#   1. Consume package refs from stdin, one ref per line.
#   2. Resolve dependencies with pak.
#   3. Hash the install refs to derive a content-addressed library path under
#      <cache_dir>.
#   4. Materialise that path as a light-weight library of symlinks into
#      renv's package cache via renv::use().
#
# The resulting library path is written to the temp result file named by
# IR_RESOLVE_RESULT_FILE. stdout/stderr stay available for pak progress.
# This session then exits; the Rust process launches the user's script in a
# fresh R session with the resolved library prepended to `.libPaths()`.
#
# The helpers below are pure and side-effect free. The pipeline runs only when
# this file is executed as a script -- `sys.nframe() == 0L` is false when the
# file is sourced. End-to-end coverage lives in the Rust CLI tests
# (tests/run.rs, tests/render.rs, and tests/tool.rs), which drive this resolver
# through real renders and package executions.

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

# Resolve dependency refs with pak, stopping if any ref fails to resolve.
ir_resolve_refs <- function(refs) {
  res <- pak::pkg_deps(refs, dependencies = NA, upgrade = TRUE)
  failed <- res[res$status != "OK", , drop = FALSE]
  if (nrow(failed))
    stop("pak could not resolve: ",
         paste(failed$ref, collapse = ", "), call. = FALSE)
  res
}

## --- cache location ---------------------------------------------------------

# The cache root: the standard per-package user cache directory, overridable
# with IR_CACHE_DIR. Holds `libraries/` (materialised libraries) and
# `resolutions/` (the resolution request cache).
ir_cache_dir <- function() {
  env <- Sys.getenv("IR_CACHE_DIR")
  if (nzchar(env)) env else tools::R_user_dir("ir", "cache")
}

## --- resolver tooling bootstrap ---------------------------------------------

# Packages the resolver itself needs. pak resolves dependencies, renv
# materialises the library, secretbase hashes the cache keys, and filelock
# serializes materialization. They are installed into a dedicated tooling library
# so users need not pre-install them.
ir_tooling_packages <- function() c("pak", "renv", "secretbase", "filelock")

# Repository for tooling installs: always the latest PPM snapshot, independent
# of the user's `exclude-newer`. ir's own tooling is not pinned to a user's
# reproducibility date. PPM serves binaries for Windows and macOS.
ir_tooling_repos <- function()
  c(CRAN = "https://packagemanager.posit.co/cran/latest")

# Path to the tooling library, keyed by R version and platform so compiled
# packages match the running R, mirroring renv's cache layout.
ir_tooling_lib <- function(cache_dir = ir_cache_dir())
  file.path(cache_dir, "tooling",
            paste0(getRversion(), "-", R.version$platform))

# Tooling packages not already usable by the resolver. Prefer the private
# tooling library, but accept ambient packages unless they come from R_LIBS_USER
# and were built under a different R minor version.
ir_missing_tooling <- function(packages = ir_tooling_packages(),
                               lib = ir_tooling_lib()) {
  r_libs_user <- Sys.getenv("R_LIBS_USER")
  user_libs <- character()
  if (nzchar(r_libs_user)) {
    user_libs <- strsplit(r_libs_user, .Platform$path.sep, fixed = TRUE)[[1L]]
    user_libs <- user_libs[nzchar(user_libs)]
    user_libs <- normalizePath(user_libs, winslash = "/", mustWork = FALSE)
  }

  current_r <- strsplit(as.character(getRversion()), ".", fixed = TRUE)[[1L]][1:2]
  missing <- character()
  bad_user_libs <- character()
  package_r_minor <- function(path) {
    metadata <- file.path(path, "Meta", "package.rds")
    info <- if (file.exists(metadata)) {
      tryCatch(readRDS(metadata), error = function(e) NULL)
    } else {
      NULL
    }

    built_r <- if (is.null(info)) character() else as.character(info$Built$R)
    if (length(built_r))
      built_r <- strsplit(built_r[[1L]], ".", fixed = TRUE)[[1L]][1:2]
    built_r
  }

  if (length(user_libs)) {
    user_secretbase <- find.package("secretbase", lib.loc = user_libs,
                                    quiet = TRUE)
    if (length(user_secretbase)) {
      pkg_lib <- normalizePath(dirname(user_secretbase[[1L]]), winslash = "/",
                               mustWork = FALSE)
      if (!identical(package_r_minor(user_secretbase[[1L]]), current_r))
        bad_user_libs <- c(bad_user_libs, pkg_lib)
    }
  }

  for (pkg in packages) {
    private_path <- find.package(pkg, lib.loc = lib, quiet = TRUE)
    if (length(private_path)) next

    path <- find.package(pkg, quiet = TRUE)
    if (!length(path)) {
      missing <- c(missing, pkg)
      next
    }

    pkg_lib <- normalizePath(dirname(path[[1L]]), winslash = "/",
                             mustWork = FALSE)
    if (pkg_lib %in% user_libs) {
      if (pkg_lib %in% bad_user_libs) {
        missing <- c(missing, pkg)
        next
      }

      if (!identical(package_r_minor(path[[1L]]), current_r)) {
        bad_user_libs <- c(bad_user_libs, pkg_lib)
        missing <- c(missing, pkg)
        next
      }
    }
  }

  if (length(bad_user_libs)) {
    bad_user_libs <- unique(bad_user_libs)
    current_libs <- .libPaths()
    current_libs_normalized <- normalizePath(current_libs, winslash = "/",
                                             mustWork = FALSE)
    .libPaths(current_libs[!current_libs_normalized %in% bad_user_libs])

    user_libs <- user_libs[!user_libs %in% bad_user_libs]
    if (length(user_libs))
      Sys.setenv(R_LIBS_USER = paste(user_libs, collapse = .Platform$path.sep))
    else
      Sys.setenv(R_LIBS_USER = "NULL")
  }

  missing
}

ir_tooling_lock_path <- function(cache_dir = ir_cache_dir())
  file.path(cache_dir, "locks", "tooling-bootstrap.lock")

ir_dir_lock_owner_path <- function(path)
  file.path(path, "owner.pid")

ir_dir_lock_reclaim_path <- function(path)
  paste0(path, ".reclaim")

ir_tooling_lock_timeout_seconds <- function() {
  env <- Sys.getenv("IR_TOOLING_LOCK_TIMEOUT_SECONDS")
  if (!nzchar(env)) return(3600)

  seconds <- suppressWarnings(as.numeric(env))
  if (is.na(seconds) || seconds <= 0)
    stop("IR_TOOLING_LOCK_TIMEOUT_SECONDS must be a positive number",
         call. = FALSE)

  seconds
}

ir_windows_process_alive <- function(pid) {
  out <- tryCatch(
    system2("tasklist",
            c("/FI", sprintf("PID eq %d", pid), "/FO", "CSV", "/NH"),
            stdout = TRUE, stderr = TRUE),
    error = function(e) {
      stop("could not query process liveness with tasklist: ",
           conditionMessage(e), call. = FALSE)
    }
  )

  status <- attr(out, "status")
  if (!is.null(status) && status != 0)
    stop("could not query process liveness with tasklist", call. = FALSE)

  rows <- tryCatch(
    utils::read.csv(text = paste(out, collapse = "\n"), header = FALSE,
                    stringsAsFactors = FALSE),
    error = function(e) NULL
  )
  if (is.null(rows) || ncol(rows) < 2L) return(FALSE)

  any(suppressWarnings(as.integer(rows[[2L]])) == pid)
}

ir_process_alive <- function(pid) {
  pid <- suppressWarnings(as.integer(pid))
  if (length(pid) != 1L || is.na(pid) || pid <= 0L) return(FALSE)

  if (.Platform$OS.type == "windows") return(ir_windows_process_alive(pid))

  isTRUE(tryCatch(tools::pskill(pid, 0), error = function(e) FALSE))
}

ir_dir_lock_owner_pid <- function(path) {
  owner <- ir_dir_lock_owner_path(path)
  if (!file.exists(owner)) return(NA_integer_)

  pid <- suppressWarnings(as.integer(readLines(owner, n = 1L, warn = FALSE)))
  if (length(pid) != 1L) return(NA_integer_)
  pid
}

ir_dir_lock_state <- function(path) {
  info <- file.info(path)
  mtime <- if (is.na(info$mtime)) NA_real_ else as.numeric(info$mtime)

  list(
    exists = dir.exists(path),
    owner_pid = ir_dir_lock_owner_pid(path),
    mtime = mtime
  )
}

ir_dir_lock_state_stale <- function(state) {
  if (!state$exists) return(FALSE)
  if (!is.na(state$owner_pid)) return(!ir_process_alive(state$owner_pid))

  if (is.na(state$mtime)) return(FALSE)
  (as.numeric(Sys.time()) - state$mtime) >= 5
}

ir_dir_lock_same_state <- function(path, state) {
  current <- ir_dir_lock_state(path)
  identical(current$exists, state$exists) &&
    identical(current$owner_pid, state$owner_pid) &&
    identical(current$mtime, state$mtime)
}

ir_dir_lock_write_owner <- function(path, label) {
  owner <- ir_dir_lock_owner_path(path)
  tryCatch(
    writeLines(as.character(Sys.getpid()), owner),
    error = function(e) {
      unlink(path, recursive = TRUE, force = TRUE)
      stop("could not write ", label, " lock owner: ", conditionMessage(e),
           call. = FALSE)
    }
  )
}

ir_dir_lock_reclaim_stale <- function(path, state) {
  if (!ir_dir_lock_state_stale(state)) return(FALSE)

  reclaim <- ir_dir_lock_reclaim_path(path)
  if (!dir.create(reclaim, showWarnings = FALSE)) return(FALSE)
  on.exit(unlink(reclaim, recursive = TRUE, force = TRUE), add = TRUE)

  if (!ir_dir_lock_same_state(path, state)) return(FALSE)
  if (!ir_dir_lock_state_stale(ir_dir_lock_state(path))) return(FALSE)

  unlink(path, recursive = TRUE, force = TRUE)
  TRUE
}

ir_dir_lock_release <- function(path) {
  if (identical(ir_dir_lock_owner_pid(path), as.integer(Sys.getpid())))
    unlink(path, recursive = TRUE, force = TRUE)
}

ir_dir_lock_acquire <- function(path, timeout_seconds, label) {
  stopifnot(length(path) == 1L, length(timeout_seconds) == 1L,
            length(label) == 1L)

  dir.create(dirname(path), recursive = TRUE, showWarnings = FALSE)
  started <- proc.time()[["elapsed"]]
  repeat {
    if (dir.create(path, showWarnings = FALSE)) {
      ir_dir_lock_write_owner(path, label)
      return(invisible())
    }

    state <- ir_dir_lock_state(path)
    if (ir_dir_lock_reclaim_stale(path, state))
      next

    if ((proc.time()[["elapsed"]] - started) >= timeout_seconds)
      stop("timed out waiting for ", label, " lock: ", path, call. = FALSE)

    Sys.sleep(0.1)
  }
}

ir_with_tooling_lock <- function(expr, cache_dir = ir_cache_dir()) {
  lock <- ir_tooling_lock_path(cache_dir)
  ir_dir_lock_acquire(lock, ir_tooling_lock_timeout_seconds(),
                      "resolver tooling")
  on.exit(ir_dir_lock_release(lock), add = TRUE)

  force(expr)
}

# Ensure resolver tooling is available. Any packages that are missing are
# installed into the tooling library, which is then put first on the search path.
ir_ensure_tooling <- function(cache_dir = ir_cache_dir(),
                              repos = ir_tooling_repos()) {
  lib <- ir_tooling_lib(cache_dir)
  dir.create(lib, recursive = TRUE, showWarnings = FALSE)
  .libPaths(c(lib, .libPaths()))

  missing <- ir_missing_tooling(lib = lib)
  if (!length(missing)) return(invisible())

  ir_with_tooling_lock({
    missing <- ir_missing_tooling(lib = lib)
    if (length(missing)) {
      utils::install.packages(missing, lib = lib, repos = repos)

      still_missing <- ir_missing_tooling(lib = lib)
      if (length(still_missing))
        stop("could not install resolver tooling into ", lib, ": ",
             paste(still_missing, collapse = ", "), call. = FALSE)
    }
  }, cache_dir = cache_dir)
  invisible()
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

# Legacy fallback key identifying a resolution request when Rust does not pass
# IR_RESOLUTION_MARKER. Normal CLI runs compute the marker path in Rust so warm
# caches can return before this R resolver is launched. Latest resolution keeps
# a stable key and stores the creation time in the marker value.
ir_input_key <- function(deps,
                         rversion      = getRversion(),
                         platform      = R.version$platform,
                         exclude_newer = NULL,
                         quarto        = FALSE) {
  source_key <- if (is.null(exclude_newer))
    "latest"
  else
    sprintf("exclude-newer: %s", exclude_newer)

  # `quarto` folds in only when TRUE: a Quarto render may inject rmarkdown, so
  # its resolved set differs from a plain run of the same deps. Omitting the
  # marker for non-Quarto runs keeps their existing keys (and cache) stable.
  secretbase::sha256(paste(c(sort(deps),
                             source_key,
                             if (quarto) "quarto" else NULL,
                             as.character(rversion),
                             platform),
                           collapse = "\n"))
}

ir_current_utc_seconds <- function()
  as.numeric(Sys.time())

ir_latest_resolution_max_age_seconds <- function() {
  value <- Sys.getenv("IR_LATEST_RESOLUTION_MAX_AGE_SECONDS", unset = NA_character_)
  if (is.na(value) || !nzchar(value)) return(24 * 60 * 60)

  if (!grepl("^[0-9]+$", value))
    stop("IR_LATEST_RESOLUTION_MAX_AGE_SECONDS must be an integer",
         call. = FALSE)
  as.numeric(value)
}

ir_marker_source <- function(exclude_newer,
                             created_at = ir_current_utc_seconds()) {
  if (is.null(exclude_newer))
    sprintf("latest: %.0f", floor(created_at))
  else
    sprintf("exclude-newer: %s", exclude_newer)
}

ir_marker_source_current <- function(source, exclude_newer) {
  if (!is.null(exclude_newer))
    return(identical(source, ir_marker_source(exclude_newer)))

  if (!startsWith(source, "latest: ")) return(FALSE)
  created_at <- suppressWarnings(as.numeric(sub("^latest: ", "", source)))
  if (is.na(created_at)) return(FALSE)

  now <- ir_current_utc_seconds()
  if (created_at > now) return(FALSE)
  now - created_at <= ir_latest_resolution_max_age_seconds()
}

ir_is_standard_input_ref <- function(ref) {
  stopifnot(length(ref) == 1L)

  ref <- trimws(ref)
  grepl(paste0("^",
               "[[:alpha:]]([[:alnum:].]*[[:alnum:]])?",
               "(@(>=)?([0-9]+[-.][0-9]+([-.][0-9]+)*|current|last))?",
               "$"),
        ref)
}

ir_has_nonstandard_input_ref <- function(refs) {
  any(!vapply(refs, ir_is_standard_input_ref, logical(1)))
}

ir_is_standard_resolved_ref <- function(res) {
  stopifnot("type" %in% names(res))

  tolower(res$type) == "standard"
}

ir_install_spec <- function(res, i) {
  if (ir_is_standard_resolved_ref(res[i, , drop = FALSE]))
    return(sprintf("%s@%s", res$package[[i]], res$version[[i]]))

  res$ref[[i]]
}

ir_install_specs <- function(res) {
  sort(unique(vapply(seq_len(nrow(res)), function(i) ir_install_spec(res, i),
                     character(1))))
}

## --- materialization lock ----------------------------------------------------

ir_materialize_lock_path <- function(cache_dir = ir_cache_dir())
  file.path(cache_dir, "locks", "renv-materialize.lock")

ir_materialize_lock_timeout_seconds <- function() {
  env <- Sys.getenv("IR_MATERIALIZE_LOCK_TIMEOUT_SECONDS")
  if (!nzchar(env)) return(3600)

  seconds <- suppressWarnings(as.numeric(env))
  if (is.na(seconds) || seconds <= 0)
    stop("IR_MATERIALIZE_LOCK_TIMEOUT_SECONDS must be a positive number",
         call. = FALSE)

  seconds
}

ir_materialize_lock_acquire <- function(path) {
  dir.create(dirname(path), recursive = TRUE, showWarnings = FALSE)
  lock <- filelock::lock(path,
                         timeout = ir_materialize_lock_timeout_seconds() * 1000)
  if (is.null(lock))
    stop("timed out waiting for resolver materialization lock: ", path,
         call. = FALSE)

  lock
}

ir_with_materialize_lock <- function(expr, cache_dir = ir_cache_dir()) {
  lock <- ir_materialize_lock_path(cache_dir)
  handle <- ir_materialize_lock_acquire(lock)
  on.exit(filelock::unlock(handle), add = TRUE)

  force(expr)
}

ir_needs_materialize <- function(library_path, pkgs, has_source_ref) {
  if (!length(pkgs)) return(FALSE)

  have <- list.files(library_path)
  has_source_ref || !all(pkgs %in% have)
}

## --- pipeline ---------------------------------------------------------------

ir_resolve_main <- function() {

  ## 0. Ensure the resolver's own tooling is available before any tooling use
  ## below.
  ir_ensure_tooling()

  deps        <- readLines(file("stdin"), warn = FALSE)
  result_file <- ir_env_optional("IR_RESOLVE_RESULT_FILE")
  package_result_file <- ir_env_optional("IR_RESOLVE_PACKAGE_RESULT_FILE")
  stopifnot(!is.null(result_file))
  cache_dir   <- ir_cache_dir()

  ## 1. Consume inputs parsed by Rust from script frontmatter
  exclude_newer <- ir_exclude_newer(ir_env_optional("IR_EXCLUDE_NEWER"))
  repos <- ir_repos(exclude_newer)
  options(repos = repos)

  # A Quarto render needs rmarkdown for the knitr engine; Rust sets
  # IR_QUARTO_RENDER so the resolver can inject it when the resolved set does not
  # already provide it. (Distinct from IR_QUARTO, the quarto executable path.)
  quarto <- !is.null(ir_env_optional("IR_QUARTO_RENDER"))

  ## 1b. Resolution cache: Rust checks this marker before launching this
  ## resolver. Keep the in-resolver check as the fallback for direct driver runs
  ## and races where another process warms the marker first. The marker is
  ## written only after a successful materialise (below), so its presence implies
  ## a complete library.
  primary_ref <- if (length(deps)) deps[[1L]] else NULL
  cache_resolution <- !ir_has_nonstandard_input_ref(deps)
  marker <- ir_env_optional("IR_RESOLUTION_MARKER")
  if (is.null(marker) && cache_resolution) {
    marker <- file.path(cache_dir, "resolutions",
                        ir_input_key(deps, exclude_newer = exclude_newer,
                                     quarto = quarto))
  }
  package_marker <- ir_env_optional("IR_PRIMARY_PACKAGE_MARKER")
  if (is.null(package_marker) && !is.null(marker) && !is.null(primary_ref)) {
    package_marker <- file.path(cache_dir, "resolutions",
                                paste0(basename(marker), "-primary-",
                                       secretbase::sha256(primary_ref)))
  }
  if (!is.null(marker) && file.exists(marker)) {
    cached <- readLines(marker, n = 2L, warn = FALSE)
    if (length(cached) >= 2L &&
        ir_marker_source_current(cached[[1L]], exclude_newer) &&
        nzchar(cached[[2L]]) &&
        dir.exists(cached[[2L]])) {
      if (!is.null(package_result_file) &&
          (is.null(package_marker) || !file.exists(package_marker))) {
        # The library is reusable, but this caller needs primary-package
        # metadata that older cache entries did not record.
      } else {
        writeLines(cached[[2L]], result_file)
        if (!is.null(package_result_file)) {
          package <- readLines(package_marker, n = 1L, warn = FALSE)
          writeLines(package, package_result_file)
        }
        return(invisible())
      }
    }
  }

  ## 2. Resolve with pak
  # A script may legitimately declare no dependencies; a non-Quarto run then gets
  # an empty resolved library. If the user requested `--isolated`, undeclared
  # library() calls fail loudly instead of borrowing from the user library. A
  # Quarto render still resolves rmarkdown (injected below).
  primary_package <- NULL
  refs_in <- deps
  res <- if (length(refs_in)) ir_resolve_refs(refs_in) else NULL

  if (!is.null(package_result_file)) {
    if (is.null(res))
      stop("cannot resolve a primary package without dependencies",
           call. = FALSE)
    primary <- unique(res$package[res$direct & res$ref == refs_in[[1L]]])
    if (length(primary) != 1L)
      stop("package ref must resolve to exactly one R package: ",
           deps[[1L]], call. = FALSE)
    primary_package <- primary[[1L]]
  }

  ## 2b. Quarto's knitr engine needs rmarkdown. Inject it (latest) only when the
  ## resolved set does not already provide it -- whether the user declared it
  ## directly or it arrived as a transitive dependency of a declared package.
  if (quarto) {
    have_rmarkdown <- !is.null(res) && "rmarkdown" %in% res$package
    if (!have_rmarkdown) {
      refs_in <- c(refs_in, "rmarkdown")
      res <- ir_resolve_refs(refs_in)
    }
  }

  if (is.null(res)) {
    pkgs     <- character()
    install_specs <- character()
    has_source_ref <- FALSE
  } else {
    # Drop base / recommended packages: those are supplied by R itself.
    keep <- is.na(res$priority) | !(res$priority %in% c("base", "recommended"))
    res <- res[keep, , drop = FALSE]
    pkgs     <- res$package
    install_specs <- ir_install_specs(res)
    has_source_ref <- any(!ir_is_standard_resolved_ref(res))
  }

  ## 3. Hash install specs -> content-addressed library path
  # Bind the hash to the R version and platform: the symlinks point into the
  # renv cache, whose layout is itself keyed by R version and platform.
  key <- paste(c(install_specs,
                 as.character(getRversion()),
                 R.version$platform),
               collapse = "\n")
  library_path <- file.path(cache_dir, "libraries", secretbase::sha256(key))

  ## 4. Materialise the symlinked library via renv::use()
  # Skip when the library already holds every resolved package: repeat runs of
  # an unchanged script then cost nothing beyond resolution.
  dir.create(library_path, recursive = TRUE, showWarnings = FALSE)
  if (ir_needs_materialize(library_path, pkgs, has_source_ref)) {
    # renv::use() installs into the renv cache and links the packages into
    # `library` as symlinks. Because `library` lives in our cache (not the R
    # temp dir), renv leaves it in place when the session ends.
    ir_with_materialize_lock({
      # Another resolver may have populated this library while we waited.
      if (ir_needs_materialize(library_path, pkgs, has_source_ref)) {
        do.call(renv::use, c(
          as.list(install_specs),
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
    }, cache_dir = cache_dir)
  }

  ## 4b. Record the resolution so an identical request skips pak.
  if (!is.null(marker)) {
    dir.create(dirname(marker), recursive = TRUE, showWarnings = FALSE)
    writeLines(c(ir_marker_source(exclude_newer), library_path), marker)
  }
  if (!is.null(primary_package) && !is.null(package_marker)) {
    writeLines(primary_package, package_marker)
  }
  writeLines(library_path, result_file)
  if (!is.null(package_result_file)) {
    writeLines(primary_package, package_result_file)
  }
  invisible()
}

if (sys.nframe() == 0L) ir_resolve_main()
