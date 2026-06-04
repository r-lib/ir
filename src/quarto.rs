use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs;
use std::io;
use std::path::Path;
use std::process::Command;

/// Phase 2 — render `doc` with `quarto render`, pointed at the selected R and
/// the materialised library.
///
/// `QUARTO_R` pins quarto's knitr R to `ir`'s selected Rscript. `R_LIBS`
/// injects the resolved library exactly as for a script. `rscript_args`
/// (leading Rscript options) are forwarded to quarto's knitr Rscript via
/// `QUARTO_KNITR_RSCRIPT_ARGS`, which quarto splits on commas with no escaping.
/// `script_args` (trailing) become `quarto render <doc> <script_args>`.
pub(crate) fn run(
    rscript: &OsStr,
    library: Option<&Path>,
    doc: &Path,
    rscript_args: &[String],
    script_args: &[String],
    isolated: bool,
) -> Result<i32, Box<dyn Error>> {
    let mut cmd = Command::new(command());
    cmd.arg("render").arg(doc).args(script_args);

    if let Some(value) = r_value(rscript) {
        cmd.env("QUARTO_R", value);
    }
    if let Some(lib) = library {
        cmd.env("R_LIBS", lib);
    }
    if !rscript_args.is_empty() {
        cmd.env("QUARTO_KNITR_RSCRIPT_ARGS", rscript_args.join(","));
    }
    if isolated {
        cmd.env("R_LIBS_USER", "NULL");
    }

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Replace ir with quarto; returns only if the exec fails.
        Err(spawn_error(cmd.exec()).into())
    }

    #[cfg(not(unix))]
    {
        let status = cmd.status().map_err(spawn_error)?;
        Ok(status.code().unwrap_or(1))
    }
}

/// Read the leading YAML metadata block from a Quarto document.
pub(crate) fn read_yaml_block_to_string(script: &Path) -> Result<String, Box<dyn Error>> {
    let content = fs::read_to_string(script)?;
    Ok(extract_yaml_block(&content))
}

/// True for Quarto documents dispatched to `quarto render`. Every other name,
/// including `.R`, `.r`, and extensionless scripts, keeps the R-script flow.
pub(crate) fn is_quarto(script: &Path) -> bool {
    matches!(
        script
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("qmd") | Some("rmd")
    )
}

/// `QUARTO_KNITR_RSCRIPT_ARGS` is comma-separated with no escaping, so an
/// Rscript option containing a comma cannot be forwarded faithfully.
pub(crate) fn reject_comma_rscript_args(rscript_args: &[String]) -> Result<(), Box<dyn Error>> {
    if let Some(arg) = rscript_args.iter().find(|arg| arg.contains(',')) {
        return Err(format!(
            "Rscript option `{arg}` contains a comma, which cannot be forwarded to \
             quarto's knitr engine: QUARTO_KNITR_RSCRIPT_ARGS is comma-separated \
             with no escaping."
        )
        .into());
    }
    Ok(())
}

/// Extract the leading YAML metadata block delimited by `---` fences, returning
/// the inner text. `str::lines` strips a trailing `\r`, so CRLF input is handled.
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

    String::new()
}

/// The value to pass as `QUARTO_R`, or `None` to leave quarto's own R lookup in
/// charge. `QUARTO_R` is pinned only when the selected Rscript is path-like.
fn r_value(rscript: &OsStr) -> Option<OsString> {
    let looks_like_path = rscript.to_string_lossy().contains(['/', '\\']);
    if looks_like_path || Path::new(rscript).exists() {
        Some(rscript.to_os_string())
    } else {
        None
    }
}

/// The quarto executable to launch: `IR_QUARTO` if set, else bare `quarto`.
fn command() -> OsString {
    env::var_os("IR_QUARTO").unwrap_or_else(|| "quarto".into())
}

/// Turn a failure to launch quarto into an actionable message.
fn spawn_error(err: io::Error) -> String {
    if err.kind() == io::ErrorKind::NotFound {
        "could not find `quarto`. Install Quarto (https://quarto.org/docs/get-started/) \
         or set IR_QUARTO to a quarto executable."
            .to_string()
    } else {
        format!("failed to launch `quarto`: {err}")
    }
}
