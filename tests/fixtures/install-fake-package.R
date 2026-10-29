args <- commandArgs(TRUE)
stopifnot(length(args) == 2L)
lib <- args[[1L]]
package <- args[[2L]]
dir.create(lib, recursive = TRUE, showWarnings = FALSE)

# Each simulated tooling library starts with pak and also needs real file locks.
if (basename(package) == "pak")
  stopifnot(file.copy(find.package("filelock"), lib, recursive = TRUE))

install.packages(package, lib = lib, repos = NULL, type = "source",
                 INSTALL_opts = c("--no-byte-compile", "--no-help", "--no-docs"))
