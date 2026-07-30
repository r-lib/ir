//! Preview integration tests for the public `ir` CLI.

mod support;

use support::*;

use std::fs;

#[cfg(unix)]
#[test]
fn preview_invokes_quarto_preview_with_source_and_trailing_args() {
    let cache_dir = temp_dir("ir-preview-cache");
    let bin_dir = temp_dir("ir-preview-bin");
    let doc = temp_path("ir-preview", "qmd");
    let observed = temp_path("ir-preview-observed", "txt");
    let rscript = bin_dir.join("Rscript");
    let quarto = bin_dir.join("quarto");

    fs::write(&doc, "---\ntitle: Preview\n---\n").unwrap();
    write_executable(
        &rscript,
        "#!/bin/sh\n\
if [ -n \"${IR_RESOLVE_RESULT_FILE:-}\" ]; then\n\
  if [ -z \"${IR_QUARTO_RENDER:-}\" ]; then\n\
    echo expected IR_QUARTO_RENDER >&2\n\
    exit 1\n\
  fi\n\
  if [ \"${IR_REFRESH:-}\" != \"1\" ]; then\n\
    echo expected IR_REFRESH >&2\n\
    exit 1\n\
  fi\n\
  mkdir -p \"$IR_CACHE_DIR/fake-library\"\n\
  printf '%s\\n' \"$IR_CACHE_DIR/fake-library\" > \"$IR_RESOLVE_RESULT_FILE\"\n\
  exit 0\n\
fi\n\
echo unexpected Rscript invocation >&2\n\
exit 1\n",
    );
    write_executable(
        &quarto,
        &format!(
            "#!/bin/sh\n\
printf '%s\\n' \"$@\" > {}\n",
            observed.display()
        ),
    );

    let out = ir()
        .env("IR_CACHE_DIR", &cache_dir)
        .env("IR_QUARTO", &quarto)
        .args(["preview", "--refresh", "--rscript"])
        .arg(&rscript)
        .arg(&doc)
        .args(["--port", "4321"])
        .output()
        .unwrap();

    assert_success(&out);
    assert_eq!(
        fs::read_to_string(&observed).unwrap(),
        format!("preview\n{}\n--port\n4321\n", doc.display())
    );
}
