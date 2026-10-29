r_package_dir <- R_PACKAGE_DIR
stopifnot(nzchar(r_package_dir))

arch <- sub("^/", "", R_ARCH)
if (!nzchar(arch)) {
  arch <- R.version$arch
}
if (!nzchar(arch)) {
  arch <- "native"
}

write_tool <- function(path, location) {
  if (.Platform$OS.type == "windows") {
    path <- paste0(path, ".cmd")
    writeLines(c(
      "@echo off",
      paste0("echo tool.location=", location),
      "echo tool.args=%*"
    ), path, useBytes = TRUE)
  } else {
    writeLines(c(
      "#!/bin/sh",
      paste0("printf 'tool.location=", location, "\\n'"),
      "printf 'tool.args=%s\\n' \"$*\""
    ), path, useBytes = TRUE)
    Sys.chmod(path, "0755")
  }
}

bin_dir <- file.path(r_package_dir, "bin")
arch_bin_dir <- file.path(bin_dir, arch)
dir.create(bin_dir, recursive = TRUE, showWarnings = FALSE)
dir.create(arch_bin_dir, recursive = TRUE, showWarnings = FALSE)

write_tool(file.path(bin_dir, "irrustbin"), "bin")
write_tool(file.path(arch_bin_dir, "irrustbin-arch"), paste0("bin/", arch))
