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
# CRAN and Bioconductor snapshots instead of the latest repositories.
ir_exclude_newer <- function(value) {
  if (is.null(value)) return(NULL)

  value <- trimws(as.character(value)[[1L]])
  if (!grepl("^[0-9]{4}-[0-9]{2}-[0-9]{2}$", value))
    stop("`exclude-newer` must be a date string in YYYY-MM-DD format",
         call. = FALSE)

  value
}

# Resolve dependency refs with pak, stopping if any ref fails to resolve.
ir_resolve_refs <- function(refs, dependencies = NA) {
  res <- pak::pkg_deps(refs, dependencies = dependencies, upgrade = TRUE)
  failed <- res[res$status != "OK", , drop = FALSE]
  if (nrow(failed))
    stop("pak could not resolve: ",
         paste(failed$ref, collapse = ", "), call. = FALSE)
  res
}

ir_resolve_primary_package <- function(res, primary_ref) {
  packages <- unique(res$package[res$direct])
  if (length(packages) != 1L) {
    primary <- ir_resolve_refs(primary_ref, dependencies = FALSE)
    packages <- unique(primary$package[primary$direct])
  }
  if (length(packages) != 1L || !(packages[[1L]] %in% res$package))
    stop("package ref must resolve to exactly one R package: ",
         primary_ref, call. = FALSE)
  packages[[1L]]
}

## --- repositories -----------------------------------------------------------

ir_named_value <- function(values, name) {
  if (is.null(values) || is.null(names(values)) || !(name %in% names(values)))
    return(NULL)
  unname(values[[name]])
}

ir_repo_resolve <- function(spec) {
  pak::repo_resolve(spec)
}

ir_linux_host <- function()
  identical(unname(Sys.info()[["sysname"]]), "Linux")

ir_public_ppm_latest_url <- function(repo)
  identical(sub("/+$", "", repo), "https://packagemanager.posit.co/cran/latest")

ir_ppm_snapshot_url <- function(exclude_newer) {
  if (!ir_linux_host())
    return(sprintf("https://packagemanager.posit.co/cran/%s", exclude_newer))

  unname(ir_repo_resolve(sprintf("PPM@%s", exclude_newer))[[1L]])
}

ir_ppm_latest_repos <- function() {
  c(CRAN = ir_ppm_snapshot_url("latest"))
}

ir_repos <- function(exclude_newer = NULL, repos = getOption("repos")) {
  if (!is.null(exclude_newer))
    return(c(CRAN = ir_ppm_snapshot_url(exclude_newer)))

  if (is.null(repos) || !length(repos))
    return(ir_ppm_latest_repos())

  if (is.null(names(repos))) {
    if (length(repos) == 1L) names(repos) <- "CRAN"
    else return(repos)
  }

  cran <- ir_named_value(repos, "CRAN")
  if (is.null(cran) || is.na(cran) || !nzchar(cran) ||
      identical(cran, "@CRAN@") || ir_public_ppm_latest_url(cran))
    repos[["CRAN"]] <- ir_ppm_snapshot_url("latest")

  repos
}

ir_ppm_bioconductor_mirror <- function(cran_repo, exclude_newer) {
  stopifnot(length(cran_repo) == 1L, !is.na(cran_repo), nzchar(cran_repo),
            length(exclude_newer) == 1L, !is.na(exclude_newer),
            nzchar(exclude_newer))

  cran_path <- regexpr("/cran(?:/|$)", cran_repo, perl = TRUE)
  if (cran_path[[1L]] == -1L)
    stop("could not derive the PPM root from CRAN repository `",
         cran_repo, "`", call. = FALSE)

  ppm_root <- substr(cran_repo, 1L, cran_path[[1L]] - 1L)
  paste0(ppm_root, "/bioconductor/", exclude_newer)
}

ir_effective_repositories <- function() {
  repositories <- pak::repo_get()
  stopifnot(is.data.frame(repositories),
            all(c("name", "url") %in% names(repositories)),
            all(!is.na(repositories$name)),
            all(nzchar(repositories$name)),
            all(!is.na(repositories$url)),
            all(nzchar(repositories$url)))

  setNames(as.character(repositories$url),
           as.character(repositories$name))
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
                         quarto        = FALSE,
                         quarto_reticulate = FALSE,
                         library_root  = NULL) {
  source_key <- if (is.null(exclude_newer))
    "latest"
  else
    sprintf("exclude-newer: %s", exclude_newer)

  # `quarto` flags fold in only when TRUE: a Quarto render may inject rmarkdown
  # or reticulate, so its resolved set differs from a plain run of the same deps.
  # Omitting the marker for non-Quarto runs keeps their existing keys stable.
  secretbase::sha256(paste(c(sort(deps),
                             source_key,
                             if (quarto) "quarto" else NULL,
                             if (quarto_reticulate)
                               "quarto-reticulate" else NULL,
                             if (!is.null(library_root))
                               paste0("library-root: ", library_root) else NULL,
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

ir_marker_source <- function(created_at = ir_current_utc_seconds()) {
  sprintf("latest: %.0f", floor(created_at))
}

ir_marker_source_current <- function(source) {
  if (!startsWith(source, "latest: ")) return(FALSE)
  max_age_seconds <- ir_latest_resolution_max_age_seconds()

  created_at <- suppressWarnings(as.numeric(sub("^latest: ", "", source)))
  if (is.na(created_at)) return(FALSE)

  now <- ir_current_utc_seconds()
  if (created_at > now) return(FALSE)
  now - created_at < max_age_seconds
}

ir_is_network_locator <- function(locator) {
  uri <- grepl("^[[:alpha:]][[:alnum:]+.-]*://", locator) &&
    !grepl("^file:", locator, ignore.case = TRUE)
  scp <- grepl("^[^/@:[:space:]]+@[^/@:[:space:]]+:.+", locator)
  uri || scp
}

ir_resolved_locators <- function(res, i) {
  locators <- res$sources[[i]]
  if ("mirror" %in% names(res))
    locators <- c(locators, res$mirror[[i]])
  locators <- as.character(locators)
  locators[!is.na(locators) & nzchar(locators)]
}

ir_assert_remote_install_sources <- function(res) {
  if (is.null(res) || !nrow(res)) return(invisible())

  stopifnot(
    is.data.frame(res),
    all(c("ref", "type", "direct", "package", "sources") %in% names(res)),
    is.list(res$sources),
    all(!is.na(res$ref)),
    all(!is.na(res$type)),
    all(!is.na(res$direct)),
    all(!is.na(res$package))
  )

  file_sources <- vapply(seq_len(nrow(res)), function(i) {
    any(grepl("^file:", ir_resolved_locators(res, i), ignore.case = TRUE))
  }, logical(1))
  local <- tolower(res$type) == "local" | file_sources
  if (!any(local)) return(invisible())

  rows <- which(local)
  origins <- vapply(rows, function(i) {
    locators <- ir_resolved_locators(res, i)
    files <- locators[grepl("^file:", locators, ignore.case = TRUE)]
    if (length(files)) files[[1L]] else res$ref[[i]]
  }, character(1))
  roles <- ifelse(res$direct[rows], "requested package", "dependency")
  details <- unique(sprintf("%s `%s` from `%s`",
                            roles, res$package[rows], origins))

  stop(
    "IR_NO_LOCAL_SOURCES is set, but installing this environment would use ",
    "packages from the local file system:\n- ",
    paste(details, collapse = "\n- "),
    "\nUse a remote package source or unset IR_NO_LOCAL_SOURCES.",
    call. = FALSE
  )
}

ir_resolution_is_cacheable <- function(res) {
  if (is.null(res) || !nrow(res))
    return(TRUE)

  stopifnot(
    is.data.frame(res),
    all(c("sources", "params") %in% names(res)),
    is.list(res$sources),
    is.list(res$params)
  )

  if (any(lengths(res$params))) return(FALSE)

  locators <- unlist(res$sources, use.names = FALSE)
  if ("mirror" %in% names(res))
    locators <- c(locators, res$mirror)
  locators <- locators[!is.na(locators) & nzchar(locators)]
  any(vapply(locators, ir_is_network_locator, logical(1)))
}

ir_invalidate_primary_package_markers <- function(marker) {
  directory <- dirname(marker)
  if (!dir.exists(directory)) return(invisible())

  entries <- list.files(directory, all.files = TRUE, full.names = TRUE)
  prefix <- paste0(basename(marker), "-primary-")
  markers <- entries[startsWith(basename(entries), prefix)]
  if (length(markers) && unlink(markers) != 0L)
    stop("could not invalidate previous primary package markers",
         call. = FALSE)
  invisible()
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

## --- pipeline ---------------------------------------------------------------

ir_resolve_main <- function() {
  # R startup files can set this after the parent process launches R.
  # pak's effective repository set is authoritative for renv.
  Sys.unsetenv("RENV_CONFIG_REPOS_OVERRIDE")

  # renv currently drops exact package versions when its pak integration is
  # enabled: https://github.com/rstudio/renv/issues/2341
  options(renv.config.pak.enabled = FALSE)

  cache_dir <- ir_cache_dir()
  library_root <- ir_env_optional("IR_LIBRARY_ROOT")
  # Rust decides this policy before R startup profiles can mutate the
  # resolver's environment.
  driver_args <- base::commandArgs(trailingOnly = TRUE)
  stopifnot(all(driver_args %in% "--ir-no-local-sources"))
  no_local_sources <- "--ir-no-local-sources" %in% driver_args
  ir_configure_child_tempdir()
  on.exit(ir_close_pak_remote(), add = TRUE)

  deps        <- readLines(file("stdin"), warn = FALSE)
  result_file <- ir_env_optional("IR_RESOLVE_RESULT_FILE")
  package_result_file <- ir_env_optional("IR_RESOLVE_PACKAGE_RESULT_FILE")
  python_result_file <- ir_env_optional("IR_PYTHON_RESULT_FILE")
  stopifnot(!is.null(result_file) || !is.null(python_result_file))

  ## 1. Consume inputs parsed by Rust from script frontmatter
  exclude_newer <- ir_exclude_newer(ir_env_optional("IR_EXCLUDE_NEWER"))

  if (!is.null(result_file)) {
    ## 0. Bootstrap pak before repository normalization. On Linux PPM URLs are
    ## resolved through pak::repo_resolve(), so pak must be available first.
    ir_ensure_tooling(packages = "pak", cache_dir = cache_dir)
    repos <- ir_repos(exclude_newer)
    options(repos = repos)

    ## Ensure the rest of the resolver's own tooling is available before any
    ## secretbase/pak/renv use below.
    ir_ensure_tooling(
      min_versions = c(renv = "1.2.0"),
      cache_dir = cache_dir
    )

    if (!is.null(exclude_newer)) {
      options(
        BioC_mirror = ir_ppm_bioconductor_mirror(
          repos[["CRAN"]],
          exclude_newer
        )
      )
    }
  }

  if (!is.null(python_result_file)) {
    python_packages_file <- ir_env_optional("IR_PYTHON_PACKAGES_FILE")
    stopifnot(!is.null(python_packages_file))
    python_packages <- readLines(python_packages_file, warn = FALSE)
    python_version <- ir_env_optional("IR_PYTHON_VERSION")
    python_exclude_newer <- ir_env_optional("IR_PYTHON_EXCLUDE_NEWER")
    python <- ir_resolve_python_env(
      packages = python_packages,
      python_version = python_version,
      exclude_newer = python_exclude_newer
    )
    writeLines(python, python_result_file)
  }

  if (is.null(result_file)) return(invisible())

  # A Quarto render needs rmarkdown for the knitr engine; Rust sets
  # IR_QUARTO_RENDER so the resolver can inject it when the resolved set does not
  # already provide it. (Distinct from IR_QUARTO, the quarto executable path.)
  quarto <- !is.null(ir_env_optional("IR_QUARTO_RENDER"))
  quarto_reticulate <- !is.null(ir_env_optional("IR_QUARTO_RETICULATE"))

  ## 1b. Resolution cache: Rust checks its marker before launching this resolver.
  ## Wrapper Rscript CLI runs and direct driver invocations use an R-derived
  ## fallback key and check it here instead.
  primary_ref <- if (length(deps)) deps[[1L]] else NULL
  refresh <- !is.null(ir_env_optional("IR_REFRESH"))
  marker <- ir_env_optional("IR_RESOLUTION_MARKER")
  marker_from_rust <- !is.null(marker)
  if (is.null(marker)) {
    marker <- file.path(cache_dir, "resolutions",
                        ir_input_key(deps, exclude_newer = exclude_newer,
                                     quarto = quarto,
                                     quarto_reticulate = quarto_reticulate,
                                     library_root = library_root))
  }
  package_marker <- ir_env_optional("IR_PRIMARY_PACKAGE_MARKER")
  if (!is.null(package_result_file) &&
      is.null(package_marker) &&
      !is.null(primary_ref)) {
    package_marker <- file.path(cache_dir, "resolutions",
                                paste0(basename(marker), "-primary-",
                                       secretbase::sha256(primary_ref)))
  }
  if (!marker_from_rust && !refresh) {
    cache_marker <- if (is.null(package_result_file)) marker else package_marker
    required_lines <- if (is.null(package_result_file)) 2L else 3L
    cached <- if (!is.null(cache_marker) && file.exists(cache_marker))
      readLines(cache_marker, n = required_lines, warn = FALSE)
    else
      character()
    if (length(cached) >= required_lines &&
        ir_marker_source_current(cached[[1L]]) &&
        nzchar(cached[[2L]]) &&
        dir.exists(cached[[2L]])) {
      package_is_current <- is.null(package_result_file) ||
        nzchar(cached[[3L]])
      if (package_is_current) {
        writeLines(cached[[2L]], result_file)
        if (!is.null(package_result_file))
          writeLines(cached[[3L]], package_result_file)
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
    primary_package <- ir_resolve_primary_package(res, refs_in[[1L]])
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
  if (quarto_reticulate) {
    have_reticulate <- !is.null(res) && "reticulate" %in% res$package
    if (!have_reticulate) {
      refs_in <- c(refs_in, "reticulate")
      res <- ir_resolve_refs(refs_in)
    }
  }

  cache_resolution <- ir_resolution_is_cacheable(res)
  if (cache_resolution)
    ir_latest_resolution_max_age_seconds()
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
  if (is.null(library_root)) library_root <- cache_dir
  library_path <- file.path(library_root, "libraries", secretbase::sha256(key))

  ## 4. Materialise the symlinked library via renv::use()
  # Skip when the library already holds every resolved package: repeat runs of
  # an unchanged script then cost nothing beyond resolution.
  dir.create(library_path, recursive = TRUE, showWarnings = FALSE)
  have <- list.files(library_path)
  if (length(pkgs) && (has_source_ref || !all(pkgs %in% have))) {
    # Cache and complete-library reuse do not run package installation code.
    # Check sources only when this invocation will ask renv to materialise them.
    if (no_local_sources)
      ir_assert_remote_install_sources(res)

    # renv::use() installs into the renv cache and links the packages into
    # `library` as symlinks. Because `library` lives in our cache (not the R
    # temp dir), renv leaves it in place when the session ends.
    effective_repositories <- ir_effective_repositories()
    do.call(renv::use, c(
      as.list(install_specs),
      list(
        library = library_path,
        repos   = effective_repositories,
        attach  = FALSE,
        sandbox = FALSE,
        isolate = TRUE,
        verbose = TRUE
      )
    ))
  }

  ## 4b. Record the resolution so an identical request skips pak.
  if (cache_resolution) {
    dir.create(dirname(marker), recursive = TRUE, showWarnings = FALSE)
    marker_source <- ir_marker_source()
    writeLines(c(marker_source, library_path), marker)
    if (!is.null(primary_package) && !is.null(package_marker))
      writeLines(c(marker_source, library_path, primary_package),
                 package_marker)
  } else {
    ir_invalidate_primary_package_markers(marker)
    if (unlink(marker) != 0L)
      stop("could not invalidate the previous resolution marker",
           call. = FALSE)
  }
  writeLines(library_path, result_file)
  if (!is.null(package_result_file)) {
    writeLines(primary_package, package_result_file)
  }
  invisible()
}

if (sys.nframe() == 0L) ir_resolve_main()
