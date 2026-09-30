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
#      renv's package cache via renv::install().
#
# The resulting library path is written to the temp result file named by
# IR_RESOLVE_RESULT_FILE. stdout/stderr stay available for pak progress.
# This session then exits; the Rust process launches the user's script in a
# fresh R session with the resolved library prepended to `.libPaths()`.
#
# The pipeline runs only when
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
  # Run against pak's bundled pkgdepends, in its existing isolated subprocess.
  # No installed package functions or namespaces are modified.
  remote <- get("remote", asNamespace("pak"), inherits = FALSE)
  remote(ir_resolve_artifacts, list(refs, dependencies,
                                  getOption("ir.prefer.binaries", TRUE)))
}

ir_resolve_artifacts <- function(refs, dependencies, prefer_binaries) {
  lib <- tempfile("ir-solve-")
  dir.create(lib)
  on.exit(unlink(lib, recursive = TRUE), add = TRUE)
  parsed <- pkgdepends::parse_pkg_refs(refs)
  minimum <- vapply(parsed, function(ref)
    ref$type %in% c("standard", "cran", "bioc") &&
      identical(ref$atleast, ">="), logical(1))
  discovery_refs <- refs
  # pkgdepends parses ranges but its archive lookup rejects them. Discover
  # current repository candidates, then give the original constraint to its
  # solver. This does not search historical archives for range requirements.
  discovery_refs[minimum] <- sub("@>=[0-9][-0-9.]*", "", refs[minimum])
  proposal <- pkgdepends::new_pkg_installation_proposal(
    discovery_refs, config = list(library = lib, dependencies = dependencies,
                       sysreqs = FALSE), policy = "upgrade")
  proposal$resolve()

  # pkgdepends exposes candidate discovery and downloads, but no public hook
  # for changing candidate preferences or excluding a failed artifact. Keep
  # that adaptation here: use its constraint builder, solver and diagnostics.
  # In particular, dependencies must be rebuilt after removing hard preference
  # rules, because pkgdepends omits dependencies of candidates it ruled out.
  ns <- asNamespace("pkgdepends")
  internal <- function(name) get(name, ns, inherits = FALSE)
  plan <- proposal$.__enclos_env__$private$plan
  state <- plan$.__enclos_env__$private
  candidates <- proposal$get_resolution()
  # pkgcache appends speculative CRAN archive / mac.cran.dev fallback URLs.
  # The first URL is the repository's advertised artifact (including a custom
  # DownloadURL). Other configured repositories have their own candidate rows.
  # Resolve again from those known candidates if that artifact is unavailable.
  repository <- candidates$type %in% c("standard", "cran", "bioc")
  candidates$sources[repository] <- lapply(candidates$sources[repository], head, 1L)
  constraints <- list()
  for (j in which(minimum)) {
    rows <- which(candidates$direct & candidates$ref == discovery_refs[j])
    constraint <- candidates[rows, , drop = FALSE]
    constraint$ref <- rep(refs[j], length(rows))
    constraint$remote <- rep(list(parsed[[j]]), length(rows))
    constraints[[length(constraints) + 1L]] <- constraint
  }
  if (length(constraints)) {
    # Retain unversioned candidates for dependency edges, but only the actual
    # user refs impose direct requirements (including multiple minimums).
    candidates$direct[candidates$ref %in% setdiff(discovery_refs[minimum], refs)] <- FALSE
    candidates <- do.call(rbind, c(list(candidates), constraints))
  }
  artifact_key <- function(data) vapply(seq_len(nrow(data)), function(i)
    paste(c(data$package[i], data$version[i], data$platform[i],
            data$sources[[i]]), collapse = "\n"), character(1))
  keys <- artifact_key(candidates)
  unavailable <- character()

  missing_artifact <- function(error) {
    if (inherits(error, c("async_http_404", "async_http_410"))) return(TRUE)
    if (inherits(error, "download_one_of_error"))
      return(length(error$errors) > 0L &&
               all(vapply(error$errors, missing_artifact, logical(1))))
    if (inherits(error$parent, "condition")) return(missing_artifact(error$parent))
    FALSE
  }
  download_diagnostic <- function(error) {
    unique(c(conditionMessage(error),
             unlist(lapply(error$errors, download_diagnostic)),
             if (inherits(error$parent, "condition"))
               download_diagnostic(error$parent)))
  }

  repeat {
    data <- candidates
    failed <- keys %in% unavailable
    data$status[failed] <- "FAILED"
    data$error[failed] <- lapply(which(failed), function(i)
      simpleError(paste("Artifact unavailable:", paste(data$sources[[i]], collapse = ", "))))
    problem <- internal("pkgplan_i_create_lp_problem")(
      data, state$config, "upgrade")
    if (prefer_binaries) {
      preferences <- c("direct-update", "choose-latest", "prefer-binary",
                       "prefer-new-binary", "dependency")
      problem$conds <- Filter(function(x) !(x$type %in% preferences), problem$conds)
      excluded <- Filter(function(x)
        identical(x$op, "==") && x$rhs == 0 && length(x$vars) == 1L,
        problem$conds)
      problem$ruled_out <- unique(unlist(lapply(excluded, `[[`, "vars")))
      problem <- internal("pkgplan_i_lp_dependencies")(problem, state$config)
      # Prefer binaries where repositories offer them, then version freshness,
      # then smaller plans. Source-only dependencies must not penalize a newer
      # source fallback (or a binary) merely for having more dependencies.
      n <- nrow(data)
      cost <- rep(1, n)
      for (package in unique(data$package[data$status == "OK"])) {
        rows <- which(data$package == package & data$status == "OK")
        versions <- rank(package_version(data$version[rows]), ties.method = "min")
        cost[rows] <- 1 + (max(versions) - versions) * (n + 1)
      }
      binaries <- which(data$type %in% c("standard", "cran", "bioc") &
                          data$platform != "source" & data$status == "OK")
      binary_packages <- data$package[setdiff(binaries, problem$ruled_out)]
      avoid_source <- data$platform == "source" & data$package %in% binary_packages
      cost <- cost + ifelse(avoid_source, n * (n + 1)^2 + 1, 0)
      dummy_cost <- max(internal("solve_dummy_obj"), sum(cost) + 1)
      problem$obj <- c(cost, rep(dummy_cost, problem$num_direct))
    }
    solved <- if (problem$total == 0L) list(status = 0L, solution = numeric()) else
      internal("pkgplan_i_solve_lp_problem")(problem)
    stopifnot(solved$status == 0L)
    selected <- as.logical(solved$solution[seq_len(nrow(data))])
    dummy <- tail(solved$solution, problem$num_direct)
    solution <- list(status = if (any(dummy != 0)) "FAILED" else "OK",
                     data = data[selected, , drop = FALSE],
                     problem = problem, solution = solved)
    if (solution$status != "OK") {
      solution$failures <- internal("describe_solution_error")(data, solution)
    }
    state$solution <- list(result = solution)
    proposal$stop_for_solution_error()
    proposal$download()
    downloads <- proposal$get_downloads()
    failed <- which(tolower(downloads$download_status) == "failed")
    if (!length(failed)) {
      downloads$extra <- NULL
      return(downloads)
    }
    for (i in failed) {
      error <- downloads$download_error[[i]]
      binary <- downloads$type[i] %in% c("standard", "cran", "bioc") &&
        downloads$platform[i] != "source"
      if (!binary || !missing_artifact(error)) {
        error$message <- paste(c(paste("Failed to download", downloads$package[i],
                                      downloads$version[i]),
                                 download_diagnostic(error)), collapse = "\n")
        stop(error)
      }
      cli::cli_alert_warning("Binary unavailable for {downloads$package[i]} {downloads$version[i]}; resolving again.")
    }
    missing <- artifact_key(downloads[failed, , drop = FALSE])
    stopifnot(!any(missing %in% unavailable))
    unavailable <- c(unavailable, missing)
  }
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
                             "ir-artifact-resolution-v2",
                             paste0("prefer-binaries: ",
                                    getOption("ir.prefer.binaries", TRUE)),
                             paste0("platforms: ", Sys.getenv("PKG_PLATFORMS")),
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

  tolower(res$type) %in% c("standard", "cran", "bioc")
}

ir_install_spec <- function(res, i) {
  # Include both locator and content identity. In particular, an old source
  # installation must never satisfy a newly selected binary of that version.
  path <- res$fulltarget[[i]]
  digest <- if (file.exists(path)) unname(tools::md5sum(path)) else res$sha[[i]]
  paste(c(res$package[[i]], res$version[[i]], res$platform[[i]],
          res$sources[[i]], digest), collapse = "\n")
}

ir_install_records <- function(res) {
  records <- lapply(seq_len(nrow(res)), function(i) {
    # A local record tells renv to consume the already downloaded artifact.
    # Keep pak's provenance fields (including pinned Git SHAs and subdirs).
    # Local package directories stay non-cacheable, as in renv's own records.
    record <- as.list(res$metadata[[i]])
    record$Package <- res$package[[i]]
    record$Version <- res$version[[i]]
    repository <- ir_is_standard_resolved_ref(res[i, , drop = FALSE])
    record$Source <- if (repository) "Local" else res$type[[i]]
    record$Path <- if (res$type[[i]] == "local") res$remote[[i]]$path else
      if (file.exists(res$fulltarget[[i]])) res$fulltarget[[i]] else
        res$fulltarget_tree[[i]]
    record$Cacheable <- res$type[[i]] != "local"
    record$Hash <- secretbase::sha256(ir_install_spec(res, i))
    if (repository || res$type[[i]] == "url")
      record$RemoteUrl <- res$sources[[i]][[1L]]
    # renv uses DESCRIPTION fields to order installations, even when dependency
    # discovery is disabled. Supply the selected candidate's requirements.
    deps <- res$deps[[i]]
    for (type in c("Depends", "Imports", "LinkingTo")) {
      rows <- which(tolower(deps$type) == tolower(type))
      if (length(rows)) record[[type]] <- paste(vapply(rows, function(j) {
        if (nzchar(deps$version[j]))
          sprintf("%s (%s %s)", deps$package[j], deps$op[j], deps$version[j])
        else deps$package[j]
      }, character(1)), collapse = ", ")
    }
    record
  })
  setNames(records, res$package)
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
  stopifnot(all(driver_args %in% c("--ir-no-local-sources",
                                  "--ir-prefer-binaries", "--ir-prefer-newest")))
  no_local_sources <- "--ir-no-local-sources" %in% driver_args
  policy <- if ("--ir-prefer-newest" %in% driver_args) "0" else
    if ("--ir-prefer-binaries" %in% driver_args) "1" else
      Sys.getenv("IR_PREFER_BINARIES", "1")
  stopifnot(policy %in% c("", "0", "1"))
  options(ir.prefer.binaries = policy != "0")
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
    ir_ensure_tooling(packages = "pak", min_versions = c(pak = "0.11.1"),
                       cache_dir = cache_dir)
    repos <- ir_repos(exclude_newer)
    options(repos = repos)

    ## Ensure the rest of the resolver's own tooling is available before any
    ## secretbase/pak/renv use below.
    ir_ensure_tooling(
      min_versions = c(pak = "0.11.1", renv = "1.2.0"),
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
        dir.exists(cached[[2L]]) &&
        !file.exists(file.path(cached[[2L]], ".ir-incomplete"))) {
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

  ## 2b. Quarto's knitr engine needs rmarkdown. Inject it only when the
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
  key <- paste(c("ir-artifact-library-v2", install_specs,
                 as.character(getRversion()),
                 R.version$platform),
               collapse = "\n")
  if (is.null(library_root)) library_root <- cache_dir
  library_path <- file.path(library_root, "libraries", secretbase::sha256(key))

  ## 4. Materialise the symlinked library via renv::install()
  # Skip when the library already holds every resolved package: repeat runs of
  # an unchanged script then cost nothing beyond resolution.
  dir.create(library_path, recursive = TRUE, showWarnings = FALSE)
  incomplete <- file.path(library_path, ".ir-incomplete")
  have <- list.files(library_path)
  if (length(pkgs) &&
      (has_source_ref || file.exists(incomplete) || !all(pkgs %in% have))) {
    # Cache and complete-library reuse do not run package installation code.
    # Check sources only when this invocation will ask renv to materialise them.
    if (no_local_sources)
      ir_assert_remote_install_sources(res)

    # Supply the full plan, including the downloaded artifacts, and disable
    # dependency discovery. renv remains responsible for installation and its
    # shared package cache, but cannot choose replacement repository artifacts.
    effective_repositories <- ir_effective_repositories()
    writeLines("Installation in progress", incomplete)
    options(renv.cache.linkable = TRUE, renv.config.install.remotes = FALSE)
    renv::install(packages = ir_install_records(res), library = library_path,
                  repos = effective_repositories, dependencies = FALSE,
                  prompt = FALSE, transactional = TRUE)
    installed <- installed.packages(lib.loc = library_path)
    stopifnot(all(pkgs %in% rownames(installed)),
              all(installed[pkgs, "Version"] == res$version))
    stopifnot(unlink(incomplete) == 0L)
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
