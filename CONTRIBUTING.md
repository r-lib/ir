# Contributing

## Local development

Install Rust, R, rig, and Quarto. Install rig and Quarto manually; the test
dependency script only installs R packages.

After installing or upgrading R, run:

```console
$ Rscript scripts/install-test-deps.R
```

Then run:

```console
$ cargo test
```
