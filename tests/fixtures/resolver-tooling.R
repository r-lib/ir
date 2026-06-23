ir_test_write_pkg <- function(lib, pkg, namespace, code,
                              built = as.character(getRversion())) {
  path <- file.path(lib, pkg)
  dir.create(file.path(path, "R"), recursive = TRUE, showWarnings = FALSE)

  description <- c(
    Package = pkg,
    Version = "0.0.1",
    Title = pkg,
    Description = paste0(pkg, "."),
    License = "MIT"
  )

  if (!is.null(built)) {
    built_field <- paste0(
      "R ", built, "; ; 2026-01-01 00:00:00 UTC; ", .Platform$OS.type
    )
    description <- c(description, Built = built_field)
  }

  writeLines(paste(names(description), description, sep = ": "),
             file.path(path, "DESCRIPTION"))
  writeLines(namespace, file.path(path, "NAMESPACE"))
  writeLines(code, file.path(path, "R", pkg))

  if (!is.null(built)) {
    dir.create(file.path(path, "Meta"), recursive = TRUE, showWarnings = FALSE)
    saveRDS(
      list(
        DESCRIPTION = description,
        Built = list(
          R = package_version(built),
          Platform = "",
          Date = "2026-01-01 00:00:00 UTC",
          OStype = .Platform$OS.type
        ),
        Depends = NULL,
        Imports = NULL,
        LinkingTo = NULL,
        Suggests = NULL
      ),
      file.path(path, "Meta", "package.rds")
    )
  }
}

ir_test_renv_code <- function() {
  paste(
    "use <- function(..., library, repos, attach, sandbox, isolate, verbose) {",
    "  specs <- unlist(list(...), use.names = FALSE)",
    "  for (spec in specs) {",
    "    pkg <- sub('@.*$', '', spec)",
    "    dir.create(file.path(library, pkg), recursive = TRUE, showWarnings = FALSE)",
    "  }",
    "  invisible(TRUE)",
    "}",
    sep = "\n"
  )
}

ir_test_write_renv <- function(lib, code = ir_test_renv_code(),
                               built = as.character(getRversion())) {
  ir_test_write_pkg(lib, "renv", "export(use)", code, built = built)
}

ir_test_write_secretbase <- function(lib, marker = NULL, hash = "privatehash",
                                     built = as.character(getRversion())) {
  on_load <- character()
  if (!is.null(marker))
    on_load <- paste0(".onLoad <- function(...) writeLines('loaded', ",
                      deparse(marker), ")")
  ir_test_write_pkg(
    lib,
    "secretbase",
    "export(sha256)",
    paste(c(on_load, paste0("sha256 <- function(x) '", hash, "'")),
          collapse = "\n"),
    built = built
  )
}

ir_test_wrong_minor_version <- function() {
  r_parts <- strsplit(as.character(getRversion()), ".", fixed = TRUE)[[1]]
  wrong_minor <- if (identical(r_parts[[2]], "0")) "1" else "0"
  paste(r_parts[[1]], wrong_minor, "0", sep = ".")
}
