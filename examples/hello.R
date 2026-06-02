#!/usr/bin/env -S ir run
# dependencies:
#   - dplyr>=1.0
#   - tidyr
#   - secretbase==1.2
# R: ">= 4.0"

library(dplyr)
library(tidyr)

# Prove we are running against the isolated library.
cat("dplyr:", as.character(packageVersion("dplyr")), "\n")
cat("tidyr:", as.character(packageVersion("tidyr")), "\n")
cat("secretbase:", as.character(packageVersion("secretbase")), "\n")
cat(".libPaths()[1]:", .libPaths()[1], "\n")

tibble(x = 1:3, y = c("a", "b", "a")) |>
  group_by(y) |>
  summarise(total = sum(x)) |>
  print()

cat("1 + 1 =", 1 + 1, "\n")
