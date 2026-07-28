//! End-to-end tests for executable examples published in user-facing docs.

mod support;

use support::*;

use std::fs;
use std::path::Path;

fn read_source(path: &str) -> String {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(path);
    fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("failed to read {}: {error}", path.display()))
        .replace("\r\n", "\n")
}

fn fenced_block(path: &str, language: &str, needle: &str) -> String {
    let source = read_source(path);
    let opener = format!("```{language}\n");
    let matches = source
        .split(&opener)
        .skip(1)
        .filter_map(|rest| rest.split_once("\n```").map(|(block, _)| block))
        .filter(|block| block.contains(needle))
        .collect::<Vec<_>>();

    assert_eq!(
        matches.len(),
        1,
        "expected one `{language}` block containing {needle:?} in {path}"
    );
    matches[0].to_string()
}

fn rustdoc_r_block(path: &str, needle: &str) -> String {
    let source = read_source(path);
    let mut in_block = false;
    let mut blocks = Vec::new();
    let mut block = String::new();

    for line in source.lines() {
        if line == "//! ```r" {
            in_block = true;
            block.clear();
        } else if in_block && line == "//! ```" {
            if block.contains(needle) {
                blocks.push(block.trim_end().to_string());
            }
            in_block = false;
        } else if in_block {
            let line = line
                .strip_prefix("//! ")
                .or_else(|| line.strip_prefix("//!"))
                .unwrap_or_else(|| panic!("unexpected Rustdoc line in {path}: {line}"));
            block.push_str(line);
            block.push('\n');
        }
    }

    assert_eq!(
        blocks.len(),
        1,
        "expected one Rustdoc R block containing {needle:?} in {path}"
    );
    blocks.remove(0)
}

fn indented_section(path: &str, heading: &str) -> String {
    let source = read_source(path);
    let start = source
        .find(heading)
        .unwrap_or_else(|| panic!("missing {heading:?} in {path}"));
    let rest = &source[start + heading.len()..];
    let block = rest
        .strip_prefix('\n')
        .unwrap_or_else(|| panic!("{heading:?} in {path} must end at a line boundary"))
        .lines()
        .take_while(|line| !line.is_empty())
        .map(|line| {
            line.strip_prefix("  ")
                .unwrap_or_else(|| panic!("expected an indented line after {heading:?} in {path}"))
        })
        .collect::<Vec<_>>()
        .join("\n");

    assert!(
        !block.is_empty(),
        "empty section after {heading:?} in {path}"
    );
    block
}

const SCRIPT_PROBE: &str = r#"
stopifnot(
  getRversion() >= "4.3",
  getRversion() < "4.4",
  as.character(packageVersion("dplyr")) == "1.1.4",
  as.character(packageVersion("tidyr")) == "1.3.1",
  Sys.getenv("R_LIBS_USER") == "NULL"
)
cat("ir.docs.script=true\n")
"#;

const QUARTO_PROBE: &str = r#"
```{r}
stopifnot(
  getRversion() >= "4.3",
  getRversion() < "4.4",
  as.character(packageVersion("dplyr")) == "1.1.4",
  as.character(packageVersion("gt")) == "0.10.1",
  Sys.getenv("R_LIBS_USER") == "NULL"
)
cat("ir.docs.quarto=true")
```
"#;

const QUARTO_SCRIPT_PROBE: &str = r#"

#' ## Documentation example test

suppressPackageStartupMessages(library(dplyr))
stopifnot(nrow(count(mtcars, cyl)) == 3)
cat("ir.docs.quarto-script=true")
"#;

#[test]
fn published_r_and_quarto_examples_execute() {
    let cache_dir = temp_cache("ir-docs-examples-cache");
    let work_dir = temp_dir("ir-docs-examples");

    let script_examples = [
        ("readme", fenced_block("README.md", "r", "tidyr==1.3.1")),
        ("index", fenced_block("docs/index.qmd", "r", "tidyr==1.3.1")),
        ("run", fenced_block("docs/run.qmd", "r", "tidyr==1.3.1")),
        (
            "rustdoc",
            rustdoc_r_block("src/main.rs", "#!/usr/bin/env -S ir run"),
        ),
        (
            "quickstart",
            format!(
                "{}\n\nairquality |> tidyr::drop_na(Ozone) |> dplyr::count(Month)",
                indented_section("src/quickstart.txt", "R script frontmatter:")
            ),
        ),
    ];

    for (name, example) in script_examples {
        let script = work_dir.join(format!("{name}.R"));
        fs::write(&script, format!("{example}\n{SCRIPT_PROBE}")).unwrap();
        let output = ir()
            .env("IR_CACHE_DIR", &cache_dir)
            .env_remove("IR_EXCLUDE_NEWER")
            .env_remove("IR_RSCRIPT")
            .env_remove("IR_R_VERSION")
            .args(["run", "--vanilla"])
            .arg(&script)
            .output()
            .unwrap();

        assert_success(&output);
        assert_stdout_contains(&output, "ir.docs.script=true");
    }

    let quarto_examples = [
        (
            "quarto-docs",
            fenced_block("docs/quarto.qmd", "yaml", "title: My report"),
        ),
        (
            "quarto-quickstart",
            indented_section("src/quickstart.txt", "Quarto frontmatter:"),
        ),
    ];

    for (name, example) in quarto_examples {
        let document = work_dir.join(format!("{name}.qmd"));
        fs::write(&document, format!("{example}\n{QUARTO_PROBE}")).unwrap();
        let output = ir()
            .current_dir(&work_dir)
            .env("IR_CACHE_DIR", &cache_dir)
            .env_remove("IR_EXCLUDE_NEWER")
            .env_remove("IR_RSCRIPT")
            .env_remove("IR_R_VERSION")
            .args(["render", "--vanilla"])
            .arg(&document)
            .args(["--to", "html"])
            .output()
            .unwrap();

        assert_success(&output);

        let html =
            fs::read_to_string(work_dir.join(format!("{name}.html"))).unwrap_or_else(|error| {
                panic!(
                    "failed to read rendered {name}: {error}\n{}",
                    output_text(&output)
                )
            });
        assert!(html.contains("ir.docs.quarto=true"), "{html}");
    }

    let script = work_dir.join("quarto-script.R");
    let example = fenced_block("docs/quarto.qmd", "r", "#' title: \"My report\"");
    fs::write(&script, format!("{example}{QUARTO_SCRIPT_PROBE}")).unwrap();
    let output = ir()
        .current_dir(&work_dir)
        .env("IR_CACHE_DIR", &cache_dir)
        .env_remove("IR_EXCLUDE_NEWER")
        .env_remove("IR_R_VERSION")
        .args(["render"])
        .arg(&script)
        .args(["--to", "html"])
        .output()
        .unwrap();

    assert_success(&output);

    let html = fs::read_to_string(work_dir.join("quarto-script.html")).unwrap_or_else(|error| {
        panic!(
            "failed to read rendered Quarto script: {error}\n{}",
            output_text(&output)
        )
    });
    assert!(html.contains("ir.docs.quarto-script=true"), "{html}");
}
