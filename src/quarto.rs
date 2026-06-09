use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::spec::{parse_quarto_frontmatter, RuntimeSpec};

pub(crate) struct RenderSource {
    path: PathBuf,
}

impl RenderSource {
    pub(crate) fn from_source_arg(source: String) -> Result<Self, Box<dyn Error>> {
        let path = PathBuf::from(&source);
        fs::metadata(&path).map_err(|e| format!("cannot read source `{source}`: {e}"))?;
        Ok(Self { path })
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn script_spec(&self) -> Result<RuntimeSpec, Box<dyn Error>> {
        if is_quarto_document(&self.path) {
            return read_quarto_document_spec(&self.path);
        }
        if is_r_script(&self.path) {
            return read_quarto_script_spec(&self.path);
        }
        Ok(RuntimeSpec::default())
    }

    pub(crate) fn reject_unsupported_rscript_args(
        &self,
        rscript_args: &[String],
    ) -> Result<(), Box<dyn Error>> {
        reject_comma_rscript_args(rscript_args)
    }
}

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

fn read_quarto_document_spec(script: &Path) -> Result<RuntimeSpec, Box<dyn Error>> {
    parse_quarto_frontmatter(&read_to_string(script)?)
}

fn read_quarto_script_spec(script: &Path) -> Result<RuntimeSpec, Box<dyn Error>> {
    parse_quarto_frontmatter(&read_quarto_script_frontmatter_to_string(script)?)
}

fn read_to_string(script: &Path) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(script)?)
}

/// True for Quarto markdown documents.
pub(crate) fn is_quarto_document(script: &Path) -> bool {
    matches!(
        script
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("qmd") | Some("rmd")
    )
}

/// True for R scripts that Quarto can render through the knitr script flow.
fn is_r_script(script: &Path) -> bool {
    matches!(
        script
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("r")
    )
}

/// `QUARTO_KNITR_RSCRIPT_ARGS` is comma-separated with no escaping, so an
/// Rscript option containing a comma cannot be forwarded faithfully.
fn reject_comma_rscript_args(rscript_args: &[String]) -> Result<(), Box<dyn Error>> {
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

fn read_quarto_script_frontmatter_to_string(script: &Path) -> Result<String, Box<dyn Error>> {
    let file = File::open(script)?;
    let mut reader = BufReader::new(file);
    let mut frontmatter = String::new();
    let mut line = String::new();

    let mut read_next_line = |line: &mut String| {
        line.clear();
        reader.read_line(line)
    };

    read_next_line(&mut line)?;
    if line.starts_with("#!") {
        read_next_line(&mut line)?;
    }

    let Some(first) = strip_quarto_script_comment(&line) else {
        return Ok(frontmatter);
    };
    if first.trim_end() != "---" {
        return Ok(frontmatter);
    }
    frontmatter.push_str(first);

    while read_next_line(&mut line)? != 0 {
        let Some(rest) = strip_quarto_script_comment(&line) else {
            break;
        };
        frontmatter.push_str(rest);
        if rest.trim_end() == "---" {
            break;
        }
    }

    Ok(frontmatter)
}

fn strip_quarto_script_comment(line: &str) -> Option<&str> {
    line.strip_prefix("#'")
        .map(|rest| rest.strip_prefix(' ').unwrap_or(rest))
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
