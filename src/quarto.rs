use std::env;
use std::error::Error;
use std::ffi::{OsStr, OsString};
use std::fs::{self, File};
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use std::process::Command;

use saphyr::Yaml;

use crate::spec::{load_first_yaml_document, parse_quarto_frontmatter, RuntimeSpec};

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
}

/// Phase 2 — render `doc` with `quarto render`, pointed at the selected R and
/// the materialised library.
///
/// `QUARTO_R` pins Quarto to `ir`'s selected Rscript. `R_LIBS`
/// injects the resolved library exactly as for a script. With `vanilla`, that
/// Rscript receives `--vanilla`. `render_args` become
/// `quarto render <doc> <render_args>`.
pub(crate) fn run(
    rscript: &OsStr,
    library: Option<&Path>,
    python: Option<&Path>,
    doc: &Path,
    render_args: &[String],
    isolated: bool,
    vanilla: bool,
) -> Result<i32, Box<dyn Error>> {
    let mut cmd = Command::new(command());
    cmd.arg("render").arg(doc).args(render_args);

    if let Some(value) = r_value(rscript) {
        cmd.env("QUARTO_R", value);
    }
    if let Some(lib) = library {
        cmd.env("R_LIBS", lib);
    }
    if let Some(python) = python {
        cmd.env("QUARTO_PYTHON", python);
        cmd.env("RETICULATE_PYTHON", python);
    }
    if isolated {
        cmd.env("R_LIBS_USER", "NULL");
    }
    if vanilla {
        cmd.env("QUARTO_KNITR_RSCRIPT_ARGS", "--vanilla");
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
    let document = read_to_string(script)?;
    let mut spec = parse_quarto_frontmatter(&document)?;
    spec.quarto_reticulate = quarto_reticulate_required(script, &document)?;
    Ok(spec)
}

fn read_quarto_script_spec(script: &Path) -> Result<RuntimeSpec, Box<dyn Error>> {
    parse_quarto_frontmatter(&read_quarto_script_frontmatter_to_string(script)?)
}

fn read_to_string(script: &Path) -> Result<String, Box<dyn Error>> {
    Ok(fs::read_to_string(script)?)
}

fn quarto_reticulate_required(script: &Path, document: &str) -> Result<bool, Box<dyn Error>> {
    let chunks = chunk_languages(document);
    if !chunks.has_python {
        return Ok(false);
    }

    let engine = frontmatter_engine(document)?;
    if engine.explicit_knitr {
        return Ok(true);
    }
    if engine.explicit_jupyter {
        return Ok(false);
    }

    Ok(is_r_markdown(script) || chunks.has_r)
}

#[derive(Default)]
struct ChunkLanguages {
    has_r: bool,
    has_python: bool,
}

fn chunk_languages(document: &str) -> ChunkLanguages {
    let mut chunks = ChunkLanguages::default();
    let mut in_yaml = false;
    let mut frontmatter_closed = false;
    let mut seen_content = false;
    let mut fence_ticks = None;

    for line in document.lines() {
        if !seen_content {
            if line.trim().is_empty() {
                continue;
            }
            seen_content = true;
            if line.trim_end() == "---" {
                in_yaml = true;
                continue;
            }
        }
        if in_yaml {
            if line.trim_end() == "---" {
                in_yaml = false;
                frontmatter_closed = true;
            }
            continue;
        }
        if !frontmatter_closed && line.trim().is_empty() {
            continue;
        }

        if let Some(ticks) = fence_ticks {
            if closing_fence(line, ticks) {
                fence_ticks = None;
            }
            continue;
        }

        if let Some((language, ticks)) = executable_chunk_start(line) {
            match language.as_str() {
                "r" => chunks.has_r = true,
                "python" => chunks.has_python = true,
                _ => {}
            }
            fence_ticks = Some(ticks);
            continue;
        }

        if let Some((ticks, _)) = backtick_fence_start(line) {
            fence_ticks = Some(ticks);
        }
    }

    chunks
}

fn executable_chunk_start(line: &str) -> Option<(String, usize)> {
    let (ticks, rest) = backtick_fence_start(line)?;
    let rest = rest.trim_start();
    let inside = rest.strip_prefix('{')?;
    let language_end = inside
        .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
        .unwrap_or(inside.len());
    if language_end == 0 {
        return None;
    }
    let language = &inside[..language_end];
    let suffix = &inside[language_end..];
    if !matches!(suffix.chars().next(), Some('}' | ' ' | ',')) {
        return None;
    }
    let close = suffix.rfind('}')?;
    if !suffix[(close + 1)..].trim().is_empty() {
        return None;
    }

    Some((language.to_ascii_lowercase(), ticks))
}

fn backtick_fence_start(line: &str) -> Option<(usize, &str)> {
    let line = line.trim_start_matches([' ', '\t', '>']);
    let ticks = line.chars().take_while(|ch| *ch == '`').count();
    (ticks >= 3).then(|| (ticks, &line[ticks..]))
}

fn closing_fence(line: &str, expected_ticks: usize) -> bool {
    let Some((ticks, rest)) = backtick_fence_start(line) else {
        return false;
    };
    ticks == expected_ticks && rest.trim().is_empty()
}

#[derive(Default)]
struct FrontmatterEngine {
    explicit_knitr: bool,
    explicit_jupyter: bool,
}

fn frontmatter_engine(document: &str) -> Result<FrontmatterEngine, Box<dyn Error>> {
    let Some(doc) = load_first_yaml_document(document, "script frontmatter")? else {
        return Ok(FrontmatterEngine::default());
    };
    if doc.is_null() || !doc.is_mapping() {
        return Ok(FrontmatterEngine::default());
    }

    let mut engine = FrontmatterEngine::default();
    if frontmatter_key_present(&doc, "knitr") {
        engine.explicit_knitr = true;
    }
    if frontmatter_key_present(&doc, "jupyter") {
        engine.explicit_jupyter = true;
    }
    apply_execute_engine(&doc, &mut engine);
    apply_format_execute_engines(&doc, &mut engine);

    Ok(engine)
}

fn frontmatter_key_present(doc: &Yaml<'_>, key: &str) -> bool {
    doc.as_mapping_get(key)
        .is_some_and(|value| !value.is_null())
}

fn apply_execute_engine(doc: &Yaml<'_>, engine: &mut FrontmatterEngine) {
    let Some(execute) = doc.as_mapping_get("execute") else {
        return;
    };
    let Some(value) = execute
        .as_mapping_get("engine")
        .and_then(|value| value.as_str())
    else {
        return;
    };
    apply_engine_name(value, engine);
}

fn apply_format_execute_engines(doc: &Yaml<'_>, engine: &mut FrontmatterEngine) {
    let Some(format) = doc.as_mapping_get("format") else {
        return;
    };
    let Some(formats) = format.as_mapping() else {
        return;
    };
    for format in formats.values() {
        apply_execute_engine(format, engine);
    }
}

fn apply_engine_name(value: &str, engine: &mut FrontmatterEngine) {
    match value.trim().to_ascii_lowercase().as_str() {
        "knitr" => engine.explicit_knitr = true,
        "jupyter" => engine.explicit_jupyter = true,
        _ => {}
    }
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

fn is_r_markdown(script: &Path) -> bool {
    matches!(
        script
            .extension()
            .and_then(|ext| ext.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("rmd")
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
