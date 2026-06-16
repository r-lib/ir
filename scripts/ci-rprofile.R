rspm <- Sys.getenv("RSPM", unset = "")
if (!nzchar(rspm))
  rspm <- "https://packagemanager.posit.co/cran/latest"

options(repos = c(CRAN = rspm))
