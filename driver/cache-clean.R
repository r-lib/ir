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
    unlink(path.expand(path), recursive = TRUE, force = TRUE, expand = FALSE)
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

ir_uv_cache_args <- function(command) {
  # Tool runs ignore project configuration but retain user/system configuration.
  # A standalone workspace gives cache commands the same scope without changing
  # the working directory used to resolve relative environment/config paths.
  project <- tempfile("ir-uv-cache-")
  stopifnot(dir.create(project))
  writeLines("[tool.uv.workspace]", file.path(project, "pyproject.toml"))
  c("cache", command, "--project", shQuote(project))
}

ir_uv_output <- function(uv, args) {
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
      "`", uv, " ", paste(args, collapse = " "), "` must print exactly one line",
      call. = FALSE
    )
  }

  out[[1L]]
}

ir_clean_uv_cache <- function(uv, path) {
  if (!nzchar(uv) || !nzchar(path)) {
    return(invisible(FALSE))
  }

  version <- numeric_version(sub(
    "^uv ([0-9.]+).*", "\\1", ir_uv_output(uv, "--version")
  ))
  # Cache locking predates UV_LOCK_TIMEOUT, so these versions can deadlock
  # when the uv process running IR holds the shared cache lock.
  if (version >= numeric_version("0.8.19") &&
      version < numeric_version("0.9.16")) {
    stop(
      "uv ", version, " cannot limit cache lock waits; ",
      "upgrade uv to 0.9.16 or newer before running `ir cache clean --all`.",
      call. = FALSE
    )
  }

  lock_timeout <- Sys.getenv("UV_LOCK_TIMEOUT", unset = NA_character_)
  on.exit({
    if (is.na(lock_timeout)) {
      Sys.unsetenv("UV_LOCK_TIMEOUT")
    } else {
      Sys.setenv(UV_LOCK_TIMEOUT = lock_timeout)
    }
  })
  Sys.setenv(UV_LOCK_TIMEOUT = "0")

  cat("Clearing uv cache at: ", path, "\n", sep = "")
  status <- suppressWarnings(system2(uv, ir_uv_cache_args("clean")))
  if (!is.null(status) && status != 0L) {
    stop(
      "failed to run `", uv, " cache clean`. ",
      "If the cache is in use, stop other uv commands and rerun with a ",
      "standalone installation of ir, outside `uvx` or `uv tool run`.",
      call. = FALSE
    )
  }

  invisible(TRUE)
}


# This process starts without startup files or default packages. The process
# that loaded packages to discover these paths has exited and released its DLLs.
plan <- readRDS(Sys.getenv("IR_CACHE_CLEAN_PLAN_FILE"))
setwd(plan$working_directory)
Sys.unsetenv(setdiff(names(Sys.getenv()), names(plan$environment)))
do.call(Sys.setenv, as.list(plan$environment))

uv_cache <- ir_uv_output(plan$uv, ir_uv_cache_args("dir"))
ir_clean_uv_cache(plan$uv, uv_cache)
for (label in names(plan$caches)) {
  ir_clear_cache(label, plan$caches[[label]])
}
