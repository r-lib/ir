# Model an installer whose library timestamp does not invalidate R's inventory.
# The public CLI must validate the packages on disk after restore completes.
args <- commandArgs(TRUE)
source(args[[1L]])
lib <- args[[2L]]
unlink(file.path(lib, "renv"), recursive = TRUE)
ir_test_write_renv(lib, code = '
restore <- function(lockfile, library, repos, ...) {
  records <- lockfile$Packages
  stopifnot(length(records) > 1L)
  timestamp <- as.POSIXct("2000-01-01", tz = "UTC")
  for (i in seq_along(records)) {
    record <- records[[i]]
    path <- file.path(library, record$Package)
    dir.create(file.path(path, "Meta"), recursive = TRUE, showWarnings = FALSE)
    desc <- c(Package = record$Package, Version = record$Version)
    writeLines(paste(names(desc), desc, sep = ": "), file.path(path, "DESCRIPTION"))
    saveRDS(list(DESCRIPTION = desc, Built = list(R = getRversion(),
      Platform = R.version$platform)), file.path(path, "Meta/package.rds"))
    if (i == 1L) {
      stopifnot(Sys.setFileTime(library, timestamp))
      stopifnot(nrow(utils::installed.packages(lib.loc = library)) == 1L)
    }
  }
  # Make the stale and fresh inventories observably different without sleeps.
  stopifnot(Sys.setFileTime(library, timestamp))
  stopifnot(nrow(utils::installed.packages(lib.loc = library)) == 1L,
            nrow(utils::installed.packages(lib.loc = library, noCache = TRUE)) == length(records))
  invisible(TRUE)
}
')
