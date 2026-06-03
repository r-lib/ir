# `ir run` for Quarto Documents — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make `ir run doc.qmd` resolve a Quarto document's `ir:` frontmatter dependencies into the existing cached library and render it with `quarto render`, using the same selected R.

**Architecture:** Reuse the unchanged two-phase pipeline. Phase 1 (resolve + materialise) is identical; only the frontmatter *source* differs (a leading `---` block, descending into the `ir:` key). Phase 2 dispatches by extension: `.qmd`/`.Rmd` → a new `run_quarto` that sets `QUARTO_R` + `R_LIBS` + `QUARTO_KNITR_RSCRIPT_ARGS` and execs `quarto render`; everything else keeps `run_script`. All changes are in `src/main.rs`; `driver/resolve.R` is untouched.

**Tech Stack:** Rust (binary crate), `saphyr` YAML (already a dependency), Quarto CLI, R/Rscript. Tests: `cargo test` with `#[cfg(test)]` unit modules in `src/main.rs` and integration tests in `tests/cli.rs` using fake executables.

**Spec:** `docs/superpowers/specs/2026-06-03-ir-run-quarto-design.md`

**Base:** `origin/main` at `4f23532` (#13, #14, #17 merged). Branch: `ir-run-quarto`.

---

## File Structure

- **Modify `src/main.rs`** — all production code:
  - `parse_frontmatter` gains a `nested` parameter and an `ir:` descent.
  - New `extract_yaml_block` (pure) + `read_yaml_block_to_string` (file reader).
  - New `is_quarto`, `quarto_r_value`, `quarto_spawn_error`, `run_quarto`.
  - `read_script_spec`, `resolve_library`, `cmd_run` thread a `quarto: bool`.
  - A `#[cfg(test)] mod tests` for the pure functions.
- **Modify `tests/cli.rs`** — integration tests for quarto dispatch (fake `quarto` on `PATH`, fake Rscript via `IR_RSCRIPT`).
- **Modify `tests/snapshots/help.stdout`, `tests/snapshots/run-help.stdout`** — updated help text.
- **Modify `README.md`** — document the qmd frontmatter form.
- **Create `examples/hello.qmd`** — a runnable example.

---

## Task 1: Nested-aware frontmatter parsing

Add the `ir:` descent to `parse_frontmatter` so a Quarto document's frontmatter (a mapping containing `ir:` plus quarto keys) resolves to the same `ScriptSpec`. R scripts keep using the top-level mapping.

**Files:**
- Modify: `src/main.rs:328-352` (`parse_frontmatter`), `src/main.rs:324-326` (`read_script_spec` call site)
- Test: `src/main.rs` (new `#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing unit tests**

Append to the end of `src/main.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_frontmatter_reads_top_level_for_scripts() {
        let spec = parse_frontmatter("dependencies:\n  - dplyr>=1.0\n  - tidyr\nR: \">= 4.0\"\n", false)
            .unwrap();
        assert_eq!(spec.dependencies, vec!["dplyr>=1.0", "tidyr"]);
        assert_eq!(spec.r_requirement.as_deref(), Some(">= 4.0"));
    }

    #[test]
    fn parse_frontmatter_descends_into_ir_for_quarto() {
        let yaml = "title: Demo\nir:\n  dependencies:\n    - gt@1.0\n  exclude after: \"2024-01-15\"\n";
        let spec = parse_frontmatter(yaml, true).unwrap();
        assert_eq!(spec.dependencies, vec!["gt@1.0"]);
        assert_eq!(spec.exclude_after.as_deref(), Some("2024-01-15"));
    }

    #[test]
    fn parse_frontmatter_quarto_without_ir_key_is_empty() {
        let spec = parse_frontmatter("title: Demo\nformat: html\n", true).unwrap();
        assert!(spec.dependencies.is_empty());
        assert!(spec.exclude_after.is_none());
        assert!(spec.r_requirement.is_none());
    }

    #[test]
    fn parse_frontmatter_quarto_null_ir_key_is_empty() {
        let spec = parse_frontmatter("ir:\n", true).unwrap();
        assert!(spec.dependencies.is_empty());
    }

    #[test]
    fn parse_frontmatter_quarto_non_mapping_ir_errors() {
        let err = parse_frontmatter("ir: nope\n", true).unwrap_err().to_string();
        assert!(err.contains("`ir`"), "{err}");
    }
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --bin ir parse_frontmatter`
Expected: FAIL — `parse_frontmatter` currently takes one argument, so the `, false` / `, true` calls do not compile.

- [ ] **Step 3: Add the `nested` parameter and `ir:` descent**

Replace `parse_frontmatter` (`src/main.rs:328-352`) with:

```rust
fn parse_frontmatter(frontmatter: &str, nested: bool) -> Result<ScriptSpec, Box<dyn Error>> {
    if frontmatter.trim().is_empty() {
        return Ok(ScriptSpec::default());
    }

    let docs = Yaml::load_from_str(frontmatter)
        .map_err(|e| format!("could not parse script frontmatter as YAML: {e}"))?;
    if docs.len() != 1 {
        return Err("script frontmatter must contain exactly one YAML document".into());
    }
    if docs[0].is_null() {
        return Ok(ScriptSpec::default());
    }

    let doc = &docs[0];
    if !doc.is_mapping() {
        return Err("script frontmatter must be a YAML mapping".into());
    }

    // For Quarto documents the dependency spec lives under the `ir:` key,
    // alongside ordinary quarto metadata; for scripts it is the document itself.
    let spec_node = if nested {
        match doc.as_mapping_get("ir") {
            None => return Ok(ScriptSpec::default()),
            Some(node) if node.is_null() => return Ok(ScriptSpec::default()),
            Some(node) => node,
        }
    } else {
        doc
    };

    if nested && !spec_node.is_mapping() {
        return Err("frontmatter `ir` must be a YAML mapping".into());
    }

    Ok(ScriptSpec {
        dependencies: frontmatter_dependencies(spec_node)?,
        exclude_after: frontmatter_optional_string(spec_node, "exclude after")?,
        r_requirement: frontmatter_optional_string(spec_node, "R")?,
    })
}
```

- [ ] **Step 4: Keep the existing caller compiling**

Update `read_script_spec` (`src/main.rs:324-326`) to pass `false` for now (Task 4 makes it conditional):

```rust
fn read_script_spec(script: &Path) -> Result<ScriptSpec, Box<dyn Error>> {
    parse_frontmatter(&read_op_frontmatter_to_string(script)?, false)
}
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --bin ir parse_frontmatter`
Expected: PASS (5 tests).

- [ ] **Step 6: Verify the whole build and existing tests still pass**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 7: Commit**

```bash
git add src/main.rs
git commit -m "Parse frontmatter from the ir: key for nested documents"
```

---

## Task 2: Quarto YAML block extractor

Extract the leading `---` … `---` metadata block from a `.qmd`/`.Rmd` file, tolerating CRLF, an optional BOM, and a `...` terminator. No opening fence, or a missing closing fence, yields an empty block (resolves to no dependencies, never an error).

**Files:**
- Modify: `src/main.rs` (add `extract_yaml_block` + `read_yaml_block_to_string` near `read_op_frontmatter_to_string` at `:448`)
- Test: `src/main.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing unit tests**

Add inside `mod tests`:

```rust
    #[test]
    fn extract_yaml_block_reads_fenced_block() {
        let doc = "---\ntitle: Demo\nir:\n  dependencies:\n    - gt\n---\n\nbody\n";
        assert_eq!(
            extract_yaml_block(doc),
            "title: Demo\nir:\n  dependencies:\n    - gt\n"
        );
    }

    #[test]
    fn extract_yaml_block_handles_crlf_and_dot_terminator() {
        let doc = "---\r\ntitle: Demo\r\n...\r\nbody\r\n";
        assert_eq!(extract_yaml_block(doc), "title: Demo\n");
    }

    #[test]
    fn extract_yaml_block_strips_optional_bom() {
        let doc = "\u{feff}---\ntitle: Demo\n---\n";
        assert_eq!(extract_yaml_block(doc), "title: Demo\n");
    }

    #[test]
    fn extract_yaml_block_without_opening_fence_is_empty() {
        assert_eq!(extract_yaml_block("title: Demo\nbody\n"), "");
        assert_eq!(extract_yaml_block(""), "");
    }

    #[test]
    fn extract_yaml_block_without_closing_fence_is_empty() {
        assert_eq!(extract_yaml_block("---\ntitle: Demo\nbody\n"), "");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --bin ir extract_yaml_block`
Expected: FAIL — `extract_yaml_block` not defined.

- [ ] **Step 3: Implement the extractor and file reader**

Add after `read_op_frontmatter_to_string` (`src/main.rs:474`):

```rust
/// Read the leading YAML metadata block from a Quarto document.
fn read_yaml_block_to_string(script: &Path) -> Result<String, Box<dyn Error>> {
    let content = fs::read_to_string(script)?;
    Ok(extract_yaml_block(&content))
}

/// Extract the leading YAML metadata block delimited by `---` fences, returning
/// the inner text. `str::lines` strips a trailing `\r`, so CRLF input is handled.
/// Returns an empty string when there is no opening fence on the first line or
/// no closing `---`/`...` line — both mean "no frontmatter", never an error.
fn extract_yaml_block(content: &str) -> String {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let mut lines = content.lines();

    match lines.next() {
        Some(first) if first.trim_end() == "---" => {}
        _ => return String::new(),
    }

    let mut block = String::new();
    for line in lines {
        let trimmed = line.trim_end();
        if trimmed == "---" || trimmed == "..." {
            return block;
        }
        block.push_str(line);
        block.push('\n');
    }

    // No closing fence: treat as no frontmatter.
    String::new()
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bin ir extract_yaml_block`
Expected: PASS (5 tests).

- [ ] **Step 5: Verify build + clippy**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS. (Note: `read_yaml_block_to_string` is unused until Task 4 — if clippy flags `dead_code`, proceed to Task 4 in the same branch; the functions are wired there. If running tasks independently, add `#[allow(dead_code)]` on `read_yaml_block_to_string` and remove it in Task 4.)

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "Extract the leading YAML block from Quarto documents"
```

---

## Task 3: Quarto run helpers

Add `quarto_r_value` (the path-like `QUARTO_R` rule), `quarto_spawn_error`, and `run_quarto` (with comma rejection). Not wired into dispatch yet — Task 4 does that — so this task is verified by unit tests on the pure helper.

**Files:**
- Modify: `src/main.rs` (add the three functions; place `run_quarto` after `run_script` at `:446`, helpers near `spawn_error` at `:538`)
- Test: `src/main.rs` (`#[cfg(test)] mod tests`)

- [ ] **Step 1: Write the failing unit tests**

Add inside `mod tests` (`OsStr` is already in scope via `use super::*`, since `src/main.rs:32` imports it — do not re-import it):

```rust
    #[test]
    fn quarto_r_value_set_for_pathlike() {
        assert_eq!(
            quarto_r_value(OsStr::new("/usr/local/bin/Rscript")),
            Some("/usr/local/bin/Rscript".into())
        );
        assert_eq!(
            quarto_r_value(OsStr::new("some/dir/Rscript")),
            Some("some/dir/Rscript".into())
        );
    }

    #[test]
    fn quarto_r_value_unset_for_bare_command() {
        // A bare command name with no separator and no such file on disk: leave
        // quarto's own PATH lookup in charge.
        assert_eq!(quarto_r_value(OsStr::new("Rscript")), None);
    }

    #[test]
    fn quarto_spawn_error_explains_missing_quarto() {
        let err = quarto_spawn_error(io::Error::from(io::ErrorKind::NotFound));
        assert!(err.contains("could not find `quarto` on PATH"), "{err}");
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --bin ir quarto_`
Expected: FAIL — `quarto_r_value` / `quarto_spawn_error` not defined.

- [ ] **Step 3: Implement the helpers and `run_quarto`**

Add `quarto_r_value` and `quarto_spawn_error` after `spawn_error` (`src/main.rs:547`):

```rust
/// The value to pass as `QUARTO_R`, or `None` to leave quarto's own R lookup in
/// charge. `QUARTO_R` is pinned only when the selected Rscript is path-like — an
/// existing path, or a value containing a path separator. A bare `Rscript`
/// resolves identically on PATH for both `ir` and quarto, so leaving `QUARTO_R`
/// unset there avoids quarto's "Specified QUARTO_R … does not exist" warning
/// while preserving the same-R invariant.
fn quarto_r_value(rscript: &OsStr) -> Option<std::ffi::OsString> {
    let looks_like_path = rscript.to_string_lossy().contains(['/', '\\']);
    if looks_like_path || Path::new(rscript).exists() {
        Some(rscript.to_os_string())
    } else {
        None
    }
}

/// Turn a failure to launch quarto into an actionable message.
fn quarto_spawn_error(err: io::Error) -> String {
    if err.kind() == io::ErrorKind::NotFound {
        "could not find `quarto` on PATH. Install Quarto: https://quarto.org/docs/get-started/"
            .to_string()
    } else {
        format!("failed to launch `quarto`: {err}")
    }
}
```

Add `run_quarto` after `run_script` (`src/main.rs:446`):

```rust
/// Phase 2 (Quarto) — render `doc` with `quarto render`, pointed at the selected
/// R and the materialised library.
///
/// `QUARTO_R` pins quarto's knitr R to `ir`'s selected Rscript (see
/// `quarto_r_value`). `R_LIBS` injects the resolved library exactly as for a
/// script. `rscript_args` (leading Rscript options) are forwarded to quarto's
/// knitr Rscript via `QUARTO_KNITR_RSCRIPT_ARGS`, which quarto splits on commas
/// with no escaping — so an arg containing a comma is rejected up front.
/// `script_args` (trailing) become `quarto render <doc> <script_args>`.
///
/// As with `run_script`, on Unix we `exec` into quarto; on Windows it runs as a
/// child and we return its exit code.
fn run_quarto(
    rscript: &OsStr,
    library: Option<&Path>,
    doc: &Path,
    rscript_args: &[String],
    script_args: &[String],
) -> Result<i32, Box<dyn Error>> {
    if let Some(arg) = rscript_args.iter().find(|arg| arg.contains(',')) {
        return Err(format!(
            "Rscript option `{arg}` contains a comma, which cannot be forwarded to \
             quarto's knitr engine: QUARTO_KNITR_RSCRIPT_ARGS is comma-separated \
             with no escaping."
        )
        .into());
    }

    let quarto: std::ffi::OsString = "quarto".into();
    let mut cmd = Command::new(&quarto);
    cmd.arg("render").arg(doc).args(script_args);

    if let Some(value) = quarto_r_value(rscript) {
        cmd.env("QUARTO_R", value);
    }
    if let Some(lib) = library {
        cmd.env("R_LIBS", lib);
    }
    if !rscript_args.is_empty() {
        cmd.env("QUARTO_KNITR_RSCRIPT_ARGS", rscript_args.join(","));
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Replace ir with quarto; returns only if the exec fails.
        Err(quarto_spawn_error(cmd.exec()).into())
    }

    #[cfg(not(unix))]
    {
        let status = cmd.status().map_err(quarto_spawn_error)?;
        Ok(status.code().unwrap_or(1))
    }
}
```

- [ ] **Step 4: Run the tests to verify they pass**

Run: `cargo test --bin ir quarto_`
Expected: PASS (3 tests).

- [ ] **Step 5: Verify build + clippy**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS. (`run_quarto` is unused until Task 4. If clippy flags `dead_code`, add `#[allow(dead_code)]` on `run_quarto` and remove it in Task 4, or run Tasks 3 and 4 back-to-back on the same branch.)

- [ ] **Step 6: Commit**

```bash
git add src/main.rs
git commit -m "Add run_quarto and its QUARTO_R and launch helpers"
```

---

## Task 4: Extension dispatch wiring

Route `.qmd`/`.Rmd` to `run_quarto` and everything else (including extensionless scripts) to `run_script`, threading the `quarto` flag through `read_script_spec` and `resolve_library`. Integration-tested end to end with a fake `quarto` on `PATH`.

**Files:**
- Modify: `src/main.rs` — `cmd_run` (`:247-270`), `resolve_library` (`:274-322`), `read_script_spec` (`:324-326`); add `is_quarto`.
- Test: `tests/cli.rs`

- [ ] **Step 1: Write the failing integration tests**

Add to `tests/cli.rs`. The first fakes both a resolver (via `IR_RSCRIPT`) and a `quarto` on `PATH`; the helper below builds a `PATH` with the fake's directory prepended.

```rust
/// Build a `PATH` value with `dir` prepended to the current process `PATH`.
fn path_with_prefix(dir: &Path) -> std::ffi::OsString {
    let mut prefixed = std::ffi::OsString::from(dir);
    if let Some(existing) = std::env::var_os("PATH") {
        prefixed.push(if cfg!(windows) { ";" } else { ":" });
        prefixed.push(existing);
    }
    prefixed
}

#[cfg(unix)]
#[test]
fn run_qmd_renders_with_quarto_and_injects_env() {
    let dir = unique_path("ir-quarto-test", "d");
    fs::create_dir_all(&dir).unwrap();
    let fake_rscript = dir.join("fake-rscript.sh");
    let fake_quarto = dir.join("quarto");
    let doc = unique_path("ir-doc", "qmd");

    // Fake Rscript: phase 1 (resolver) writes a library path and exits.
    write_executable(
        &fake_rscript,
        r#"#!/bin/sh
set -eu
if [ "${IR_RESOLVE_RESULT_FILE:-}" != "" ]; then
  cat > /dev/null
  echo "/tmp/ir-test-library" > "$IR_RESOLVE_RESULT_FILE"
  exit 0
fi
echo "fake Rscript should not run the document" >&2
exit 5
"#,
    );

    // Fake quarto: assert argv and the injected environment, then succeed.
    write_executable(
        &fake_quarto,
        &format!(
            r#"#!/bin/sh
set -eu
test "$1" = "render"
test "$3" = "--to"
test "$4" = "pdf"
test "${{QUARTO_R:-}}" = "{rscript}"
test "${{R_LIBS:-}}" = "/tmp/ir-test-library"
test "${{QUARTO_KNITR_RSCRIPT_ARGS:-}}" = "--vanilla"
echo "fake quarto rendered $2"
"#,
            rscript = fake_rscript.display()
        ),
    );

    fs::write(
        &doc,
        "---\nir:\n  dependencies:\n    - dplyr>=1.0\n---\n\n```{r}\n1 + 1\n```\n",
    )
    .unwrap();

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .env("PATH", path_with_prefix(&dir))
        .args(["run", "--vanilla", doc.to_str().unwrap(), "--to", "pdf"])
        .output()
        .unwrap();

    let _ = fs::remove_file(&doc);
    let _ = fs::remove_dir_all(&dir);

    assert!(out.status.success(), "{:?}", out);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("fake quarto rendered"),
        "{:?}",
        out
    );
}

#[cfg(unix)]
#[test]
fn run_qmd_with_comma_in_rscript_arg_errors_before_quarto() {
    let doc = unique_path("ir-doc", "qmd");
    fs::write(&doc, "---\nir:\n  dependencies:\n    - dplyr\n---\n").unwrap();

    // No IR_RSCRIPT/quarto needed: the comma check fires before any launch. A
    // present-but-failing resolver would still never run if the check is correct,
    // but to be safe we point IR_RSCRIPT at a resolver that fails loudly.
    let fake_rscript = unique_path("ir-fake-rscript", "sh");
    write_executable(
        &fake_rscript,
        "#!/bin/sh\necho \"resolver should not run\" >&2\nexit 7\n",
    );

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .args(["run", "--max-connections=1,2", doc.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = fs::remove_file(&doc);
    let _ = fs::remove_file(&fake_rscript);

    assert_eq!(out.status.code(), Some(1));
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(stderr.contains("contains a comma"), "{stderr}");
}

#[cfg(unix)]
#[test]
fn run_extensionless_script_still_uses_rscript() {
    let fake_rscript = unique_path("ir-fake-rscript", "sh");
    // No extension: must take the R-script path, not quarto.
    let script = unique_path("ir-bare-script", "");

    write_executable(
        &fake_rscript,
        r#"#!/bin/sh
set -eu
if [ "${IR_RESOLVE_RESULT_FILE:-}" != "" ]; then
  cat > /dev/null
  : > "$IR_RESOLVE_RESULT_FILE"
  exit 0
fi
echo "ran as R script"
"#,
    );
    fs::write(&script, "cat('unused by fake Rscript\\n')\n").unwrap();

    let out = ir()
        .env("IR_RSCRIPT", &fake_rscript)
        .args(["run", script.to_str().unwrap()])
        .output()
        .unwrap();

    let _ = fs::remove_file(&fake_rscript);
    let _ = fs::remove_file(&script);

    assert!(out.status.success(), "{:?}", out);
    assert!(
        String::from_utf8_lossy(&out.stdout).contains("ran as R script"),
        "{:?}",
        out
    );
}
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --test cli run_qmd run_extensionless`
Expected: FAIL — `.qmd` currently takes the R-script path (no quarto dispatch), so `run_qmd_renders_with_quarto_and_injects_env` does not reach the fake quarto, and the comma check does not exist yet.

- [ ] **Step 3: Add `is_quarto` and thread the flag**

Add `is_quarto` near `read_script_spec` (`src/main.rs:324`):

```rust
/// True for Quarto documents dispatched to `quarto render`. Every other name —
/// `.R`, `.r`, and extensionless scripts run via shebang — keeps the R-script
/// flow, preserving backward compatibility.
fn is_quarto(script: &Path) -> bool {
    matches!(
        script
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("qmd") | Some("rmd")
    )
}
```

Replace `read_script_spec` (`src/main.rs:324-326`):

```rust
fn read_script_spec(script: &Path, quarto: bool) -> Result<ScriptSpec, Box<dyn Error>> {
    let frontmatter = if quarto {
        read_yaml_block_to_string(script)?
    } else {
        read_op_frontmatter_to_string(script)?
    };
    parse_frontmatter(&frontmatter, quarto)
}
```

Change `resolve_library`'s signature and the `read_script_spec` call (`src/main.rs:274` and `:278`):

```rust
fn resolve_library(
    rscript: &OsStr,
    script: &Path,
    quarto: bool,
) -> Result<Option<PathBuf>, Box<dyn Error>> {
    let tmp = env::temp_dir();
    let driver = unique_path(&tmp, "ir-resolve", "R");
    let result_file = unique_path(&tmp, "ir-libpath", "txt");
    let spec = read_script_spec(script, quarto)?;
    fs::write(&driver, RESOLVE_DRIVER)?;
    // ... unchanged below ...
```

Replace the body of `cmd_run` (`src/main.rs:252-269`):

```rust
    let script_path =
        fs::canonicalize(script).map_err(|e| format!("cannot read script `{script}`: {e}"))?;

    let rscript = rscript_command();
    let quarto = is_quarto(&script_path);

    // Phase 1: private R session resolves deps and materialises the library.
    let library = resolve_library(&rscript, &script_path, quarto)?;

    // Phase 2: render the document, or run the script, in an isolated R session.
    let code = if quarto {
        run_quarto(
            &rscript,
            library.as_deref(),
            &script_path,
            rscript_args,
            script_args,
        )?
    } else {
        run_script(
            &rscript,
            library.as_deref(),
            &script_path,
            rscript_args,
            script_args,
        )?
    };
    std::process::exit(code);
```

If you added `#[allow(dead_code)]` on `read_yaml_block_to_string` or `run_quarto` in Tasks 2–3, remove it now.

- [ ] **Step 4: Run the new tests to verify they pass**

Run: `cargo test --test cli run_qmd run_extensionless`
Expected: PASS (3 tests).

- [ ] **Step 5: Verify the whole suite + clippy**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 6: Commit**

```bash
git add src/main.rs tests/cli.rs
git commit -m "Dispatch .qmd and .Rmd targets to quarto render"
```

---

## Task 5: Documentation, help text, and an example

Document Quarto support in help output (updating #17's snapshots), the README, and add a runnable example.

**Files:**
- Modify: `src/main.rs` — `print_help` (`:166-188`) and `print_run_help` (`:190-206`)
- Modify: `tests/snapshots/help.stdout`, `tests/snapshots/run-help.stdout`
- Modify: `tests/cli.rs` — the `contains(...)` assertions in `help_is_shown_for_help_flag_and_no_args` and `run_help_flag_shows_help`
- Modify: `README.md`
- Create: `examples/hello.qmd`

- [ ] **Step 1: Update the help text**

In `print_help` (`src/main.rs:175-180`) and `print_run_help` (`src/main.rs:197-200`), the body currently reads "reads the YAML frontmatter from <script.R>". Add a sentence after each existing description paragraph:

```rust
            "Quarto documents (.qmd, .Rmd) are also supported: declare\n",
            "dependencies under an `ir:` key in the document's YAML frontmatter\n",
            "and ir renders them with `quarto render`.\n",
```

Place this line inside both `concat!(...)` blocks, immediately before the `"\n", "ENVIRONMENT:\n"` lines. Keep the existing `<script.R>` USAGE lines unchanged (they remain accurate for scripts).

- [ ] **Step 2: Run the help tests to verify they fail**

Run: `cargo test --test cli help_outputs_match_snapshots help_is_shown run_help`
Expected: FAIL — `assert_help_snapshot` compares against the old `tests/snapshots/help.stdout` and `run-help.stdout`, which do not yet contain the new sentence.

- [ ] **Step 3: Regenerate the snapshots**

The test (`assert_help_snapshot`) compares stdout to the file **byte-for-byte**.
`ir` writes `\n` line endings and no BOM. **Do not redirect through PowerShell**
(`>` there writes UTF-16/CRLF + BOM and corrupts the snapshot); use a capture
that preserves raw LF bytes. In Git Bash, from the repo root:

```bash
cargo build
./target/debug/ir --help > tests/snapshots/help.stdout
./target/debug/ir run --help > tests/snapshots/run-help.stdout
```

(If `./target/debug/ir` does not resolve in your shell, use `target/debug/ir.exe`
on Windows.) Then confirm the files are LF-only with no BOM:

```bash
git diff --stat tests/snapshots/
file tests/snapshots/help.stdout   # should not say "with CRLF" or "BOM"
```

If your shell cannot produce clean LF output, instead run `cargo build`, read the
help text from the two new `concat!` blocks, and write the snapshot files
directly with an editor set to LF — the content is static.

- [ ] **Step 4: Run the help tests to verify they pass**

Run: `cargo test --test cli help_outputs_match_snapshots help_is_shown run_help`
Expected: PASS.

- [ ] **Step 5: Update the README**

In `README.md`, after the "Frontmatter format" section for scripts, add a short subsection:

````markdown
## Quarto documents

`ir run` also renders Quarto documents (`.qmd`, `.Rmd`). Declare dependencies
under an `ir:` key in the document's YAML frontmatter:

```yaml
---
title: "My report"
ir:
  dependencies:
    - dplyr>=1.0
    - gt@1.0
  R: ">= 4.0"
  exclude after: "2024-01-15"
---
```

`ir run report.qmd` resolves those dependencies into the same cached, isolated
library used for scripts, then runs `quarto render report.qmd` with that library
and the selected R. Trailing arguments are passed to `quarto render`
(`ir run report.qmd --to pdf`); leading Rscript options are forwarded to the
knitr engine (`ir run --vanilla report.qmd`).
````

- [ ] **Step 6: Add the example document**

Create `examples/hello.qmd`:

```markdown
---
title: "ir + Quarto"
ir:
  dependencies:
    - dplyr>=1.0
    - glue
---

```{r}
library(dplyr)
library(glue)

glue("rows: {nrow(mtcars)}")
```
```

- [ ] **Step 7: Verify the full suite + clippy**

Run: `cargo test && cargo clippy --all-targets -- -D warnings`
Expected: PASS, no warnings.

- [ ] **Step 8: Commit**

```bash
git add src/main.rs tests/snapshots/help.stdout tests/snapshots/run-help.stdout tests/cli.rs README.md examples/hello.qmd
git commit -m "Document Quarto document support"
```

---

## Manual verification (real toolchain)

Run once on a machine with R, the `pak`/`renv`/`secretbase` packages, and Quarto installed:

- [ ] `cargo build --release`
- [ ] `./target/release/ir run examples/hello.qmd` — confirm it resolves the library and produces `examples/hello.html`, and that `dplyr`/`glue` load from the materialised library (not a pre-existing user library). Optionally set `IR_CACHE_DIR` to a fresh temp dir to force a real resolve.
- [ ] `./target/release/ir run examples/hello.qmd --to pdf` (if LaTeX available) — confirm the trailing arg reaches `quarto render`.
- [ ] Temporarily rename `quarto` off `PATH` and confirm `ir run examples/hello.qmd` prints the "could not find `quarto` on PATH" error.

## Dev workflow / handoff

- Work on branch `ir-run-quarto` (already created off `origin/main` @ `4f23532`).
- Commit per task as above. Run `cargo test && cargo clippy --all-targets -- -D warnings` before each commit.
- `origin` is `t-kalinowski/ir` (no fork). After the tasks, `git push -u origin ir-run-quarto`, then STOP for Chris to review the diff locally before opening a PR against `main`.
- Link the PR to the tracking issue if one exists; do not reference private trackers in the PR body.

## Notes / conscious tradeoffs

- **No quarto preflight.** A missing `quarto` is reported at the render step, after phase 1. On a resolution cache-miss the resolver runs before the failure; this is accepted to keep the happy path free of an extra `quarto --version` spawn. (Decision confirmed with Chris.)
- **`QUARTO_R` only when path-like.** Avoids quarto's "does not exist" warning for the bare `Rscript` default while preserving the same-R invariant. (Decision confirmed with Chris.)
- **Comma in `rscript_args` is rejected**, not escaped — quarto's `QUARTO_KNITR_RSCRIPT_ARGS` split has no escape mechanism.
- **`resolve.R` is untouched** — the qmd flow produces the same stdin + env inputs a script does.
