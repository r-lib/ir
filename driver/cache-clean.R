ir_count_files <- function(path) {
  if (!file.exists(path)) {
    return(0L)
  }

  info <- file.info(path)
  if (!isTRUE(info$isdir)) {
    return(1L)
  }

  length(list.files(
    path,
    all.files = TRUE,
    no.. = TRUE,
    recursive = TRUE,
    full.names = TRUE,
    include.dirs = FALSE
  ))
}

ir_clear_cache <- function(label, paths) {
  paths <- unique(paths[nzchar(paths)])

  for (path in paths) {
    if (!file.exists(path)) {
      cat("No ", label, " found at: ", path, "\n", sep = "")
      next
    }

    files <- ir_count_files(path)
    cat("Clearing ", label, " at: ", path, "\n", sep = "")
    unlink(path, recursive = TRUE, force = TRUE)
    if (file.exists(path)) {
      stop("failed to remove ", label, " `", path, "`", call. = FALSE)
    }
    cat(
      "Removed ", files, " ",
      if (identical(files, 1L)) "file" else "files",
      " from ", label, "\n",
      sep = ""
    )
  }
}

ir_split_paths <- function(paths) {
  if (!nzchar(paths)) {
    return(character())
  }

  pattern <- if (.Platform$OS.type == "windows") ";" else "[;:]"
  parts <- strsplit(paths, pattern)[[1L]]
  parts[nzchar(parts)]
}

ir_env <- function(name, default = "") {
  value <- Sys.getenv(name, "")
  if (nzchar(value)) value else default
}

ir_pkg_config_path <- function(option, envvar) {
  value <- getOption(option, NULL)
  if (!is.null(value)) {
    return(as.character(value[[1L]]))
  }

  Sys.getenv(envvar, "")
}

ir_windows_local_app_data <- function() {
  local_app_data <- Sys.getenv("LOCALAPPDATA", "")
  if (nzchar(local_app_data)) {
    return(local_app_data)
  }

  user_profile <- Sys.getenv("USERPROFILE", "")
  if (nzchar(user_profile)) {
    return(file.path(user_profile, "AppData", "Local"))
  }

  file.path(tempdir(), "r-pkg-cache")
}

ir_r_user_cache_dir <- function(package) {
  tools <- asNamespace("tools")
  r_user_dir <- tools$R_user_dir
  if (is.function(r_user_dir)) {
    return(r_user_dir(package, "cache"))
  }

  r_user_cache_dir <- Sys.getenv("R_USER_CACHE_DIR", "")
  if (nzchar(r_user_cache_dir)) {
    return(file.path(r_user_cache_dir, "R", package))
  }

  xdg_cache_home <- Sys.getenv("XDG_CACHE_HOME", "")
  if (nzchar(xdg_cache_home)) {
    return(file.path(xdg_cache_home, "R", package))
  }

  if (.Platform$OS.type == "windows") {
    return(file.path(ir_windows_local_app_data(), "R", "cache", "R", package))
  }

  if (identical(Sys.info()[["sysname"]], "Darwin")) {
    return(path.expand(file.path(
      "~/Library/Caches/org.R-project.R/R",
      package
    )))
  }

  path.expand(file.path("~/.cache/R", package))
}

ir_pkgcache_cache_dirs <- function() {
  c(
    ir_pkg_config_path("pkg.package_cache_dir", "PKG_PACKAGE_CACHE_DIR"),
    ir_pkgcache_default_cache_dir()
  )
}

ir_pkg_metadata_cache_dirs <- function() {
  c(
    ir_pkg_config_path("pkg.metadata_cache_dir", "PKG_METADATA_CACHE_DIR"),
    ir_pkgcache_default_metadata_cache_dirs()
  )
}

ir_pkgcache_default_cache_dir <- function() {
  if (nzchar(Sys.getenv("R_PKG_CACHE_DIR", ""))) {
    dirs <- ir_resolve_pkgcache_user_dirs()
    if (is.null(dirs)) {
      dirs <- ir_pkgcache_fallback_user_dirs()
    }
    if (!is.null(dirs)) {
      return(dirs[["pkg"]])
    }

    return(character())
  }

  ir_r_user_cache_dir("pkgcache")
}

ir_pkgcache_default_metadata_cache_dirs <- function() {
  if (!nzchar(Sys.getenv("R_PKG_CACHE_DIR", ""))) {
    return(character())
  }

  dirs <- ir_resolve_pkgcache_user_dirs()
  if (is.null(dirs)) {
    dirs <- ir_pkgcache_fallback_user_dirs()
  }
  if (is.null(dirs)) {
    return(character())
  }

  c(dirs[["meta"]], dirs[["lock"]])
}

ir_resolve_pkgcache_user_dirs <- function() {
  if (requireNamespace("pkgcache", quietly = TRUE)) {
    dirs <- tryCatch(
      ir_pkgcache_user_dirs_from_namespace(asNamespace("pkgcache")),
      error = function(err) NULL
    )
    if (!is.null(dirs)) {
      return(dirs)
    }
  }

  if (requireNamespace("pak", quietly = TRUE)) {
    dirs <- tryCatch({
      ns <- asNamespace("pak")
      load_private_package <- get(
        "load_private_package",
        envir = ns,
        inherits = FALSE
      )
      load_private_package("pkgcache")
      pkg_data <- get("pkg_data", envir = ns, inherits = FALSE)
      ir_pkgcache_user_dirs_from_namespace(pkg_data$ns$pkgcache)
    }, error = function(err) NULL)
    if (!is.null(dirs)) {
      return(dirs)
    }
  }

  NULL
}

ir_pkgcache_fallback_user_dirs <- function() {
  root <- Sys.getenv("R_PKG_CACHE_DIR", "")
  if (!nzchar(root)) {
    return(NULL)
  }

  pkgcache <- normalizePath(file.path(root, "R", "pkgcache"), mustWork = FALSE)
  c(
    pkg = file.path(pkgcache, "pkg"),
    meta = file.path(pkgcache, "_metadata"),
    lock = file.path(pkgcache, "_metadata.lock")
  )
}

ir_pkgcache_user_dirs_from_namespace <- function(ns) {
  get_user_cache_dir <- get("get_user_cache_dir", envir = ns, inherits = FALSE)
  dirs <- get_user_cache_dir()
  c(
    pkg = as.character(dirs$pkg[[1L]]),
    meta = as.character(dirs$meta[[1L]]),
    lock = as.character(dirs$lock[[1L]])
  )
}

ir_pak_cache_dirs <- function() {
  ir_pkg_config_path("pkg.cache_dir", "PKG_CACHE_DIR")
}

ir_renv_cache_dirs <- function() {
  cache <- Sys.getenv("RENV_PATHS_CACHE", "")
  if (nzchar(cache)) {
    return(ir_split_paths(cache))
  }

  if (requireNamespace("renv", quietly = TRUE)) {
    return(renv::paths$root("cache"))
  }

  root <- Sys.getenv("RENV_PATHS_ROOT", "")
  if (nzchar(root)) {
    return(file.path(root, "cache"))
  }

  file.path(ir_renv_default_root_dir(), "cache")
}

ir_renv_default_root_dir <- function() {
  roots <- c(
    ir_r_user_cache_dir("renv"),
    ir_renv_legacy_root_dir()
  )

  for (root in roots) {
    if (file.exists(root)) {
      return(root)
    }
  }

  roots[[1L]]
}

ir_renv_legacy_root_dir <- function() {
  base <- switch(
    Sys.info()[["sysname"]],
    Darwin = ir_env("XDG_DATA_HOME", "~/Library/Application Support"),
    Windows = ir_env(
      "LOCALAPPDATA",
      ir_env("APPDATA", ir_windows_local_app_data())
    ),
    ir_env("XDG_DATA_HOME", "~/.local/share")
  )

  path.expand(file.path(base, "renv"))
}

ir_reticulate_legacy_cache_dir <- function() {
  if (requireNamespace("rappdirs", quietly = TRUE)) {
    return(rappdirs::user_cache_dir("r-reticulate", NULL))
  }

  r_user_cache_dir <- Sys.getenv("R_USER_CACHE_DIR", "")
  if (nzchar(r_user_cache_dir)) {
    return(file.path(r_user_cache_dir, "r-reticulate"))
  }

  if (.Platform$OS.type == "windows") {
    return(file.path(ir_windows_local_app_data(), "r-reticulate", "Cache"))
  }

  if (identical(Sys.info()[["sysname"]], "Darwin")) {
    return(path.expand("~/Library/Caches/r-reticulate"))
  }

  file.path(ir_env("XDG_CACHE_HOME", path.expand("~/.cache")), "r-reticulate")
}

ir_is_usable_uv <- function(uv) {
  if (is.null(uv) || is.na(uv) || identical(uv, "") || !file.exists(uv)) {
    return(FALSE)
  }

  out <- suppressWarnings(system2(uv, "--version", stdout = TRUE, stderr = TRUE))
  status <- attr(out, "status", exact = TRUE)
  if (!is.null(status) && !identical(status, 0L)) {
    return(FALSE)
  }

  version <- numeric_version(sub("uv ([0-9.]+).*", "\\1", out), strict = FALSE)
  !is.na(version) && version >= numeric_version("0.6.3")
}

ir_external_uv_binary <- function() {
  uv <- Sys.getenv("RETICULATE_UV", unset = NA_character_)
  if (!is.na(uv)) {
    if (identical(uv, "managed")) {
      return("")
    }
    return(uv)
  }

  uv <- getOption("reticulate.uv_binary", NULL)
  if (!is.null(uv)) {
    uv <- as.character(uv[[1L]])
    if (identical(uv, "managed")) {
      return("")
    }
    return(uv)
  }

  uv <- unname(Sys.which("uv"))
  if (ir_is_usable_uv(uv)) {
    return(uv)
  }

  uv <- path.expand("~/.local/bin/uv")
  if (ir_is_usable_uv(uv)) uv else ""
}

ir_reticulate_cache_dir <- function() {
  ir_r_user_cache_dir("reticulate")
}

ir_reticulate_managed_uv_binary <- function() {
  file.path(
    ir_reticulate_cache_dir(),
    "uv",
    "bin",
    if (.Platform$OS.type == "windows") "uv.exe" else "uv"
  )
}

ir_uv_dir <- function(uv, args) {
  if (!nzchar(uv)) {
    return("")
  }

  out <- suppressWarnings(system2(uv, args, stdout = TRUE, stderr = FALSE))
  status <- attr(out, "status", exact = TRUE)
  if (!is.null(status) && !identical(status, 0L)) {
    stop(
      "failed to run `", uv, " ", paste(args, collapse = " "), "`",
      call. = FALSE
    )
  }

  out <- out[nzchar(out)]
  if (length(out) != 1L) {
    stop(
      "`", uv, " ", paste(args, collapse = " "), "` must print exactly one path",
      call. = FALSE
    )
  }

  out[[1L]]
}

ir_clean_uv_cache <- function(uv, path) {
  if (!nzchar(uv) || !nzchar(path)) {
    return(invisible(FALSE))
  }

  cat("Clearing uv cache at: ", path, "\n", sep = "")
  status <- suppressWarnings(system2(uv, c("cache", "clean")))
  if (!is.null(status) && status != 0L) {
    stop("failed to run `", uv, " cache clean`", call. = FALSE)
  }

  invisible(TRUE)
}

uv <- ir_external_uv_binary()
uv_cache <- ir_uv_dir(uv, c("cache", "dir"))

ir_clear_cache("pak package cache", ir_pkgcache_cache_dirs())
ir_clear_cache("pak metadata cache", ir_pkg_metadata_cache_dirs())
ir_clear_cache("pak cache", ir_pak_cache_dirs())
ir_clear_cache("renv cache", ir_renv_cache_dirs())
ir_clean_uv_cache(uv, uv_cache)
ir_clear_cache("reticulate cache", ir_reticulate_cache_dir())
ir_clear_cache("reticulate legacy cache", ir_reticulate_legacy_cache_dir())
