# Use renv's public copy hook to hold a cache replacement after its backup.
args <- commandArgs(TRUE)
port <- as.integer(args[[1L]])
entry <- args[[2L]]
stopifnot(file.exists(file.path(entry, "Meta/package.rds")))
record <- as.list(read.dcf(file.path(entry, "DESCRIPTION"))[1L, ])
record$Source <- "URL"
record$Hash <- basename(dirname(entry))

options(renv.cache.linkable = FALSE,
        renv.config.copy.method = function(source, target) {
  if (identical(dirname(target), dirname(entry))) {
    stopifnot(!dir.exists(entry))
    gate <- socketConnection("127.0.0.1", port = port, open = "r+b",
                             blocking = TRUE, timeout = 60)
    on.exit(close(gate), add = TRUE)
    writeLines("cache-removed", gate)
    flush(gate)
    stopifnot(identical(readLines(gate, n = 1L), "continue"))
  }
  dir.create(target, recursive = TRUE, showWarnings = FALSE)
  files <- list.files(source, all.files = TRUE, no.. = TRUE, full.names = TRUE)
  stopifnot(all(file.copy(files, target, recursive = TRUE,
                         copy.mode = TRUE, copy.date = TRUE)))
  TRUE
})
library <- tempfile("cache-replacement-library-")
renv::restore(lockfile = list(R = list(Version = as.character(getRversion())),
                              Packages = list(iridentity = record)),
              library = library, prompt = FALSE, rebuild = TRUE,
              transactional = TRUE)
