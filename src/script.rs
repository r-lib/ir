use std::error::Error;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use saphyr::{Yaml, YamlLoader};
use saphyr_parser::Parser;

use crate::quarto;

#[derive(Debug, Default)]
pub(crate) struct ScriptSpec {
    pub(crate) dependencies: Vec<String>,
    pub(crate) exclude_newer: Option<String>,
    pub(crate) isolated: bool,
    pub(crate) r_requirement: Option<String>,
    // A Quarto source: the resolver injects rmarkdown for the knitr engine.
    pub(crate) quarto: bool,
}

/// Where the user's program comes from.
pub(crate) enum RunSource {
    Script(PathBuf),
    Expressions(Vec<String>),
    Stdin,
}

pub(crate) struct RenderSource {
    path: PathBuf,
}

impl RunSource {
    pub(crate) fn from_script_arg(script: String) -> Result<Self, Box<dyn Error>> {
        if script == "-" {
            return Ok(Self::Stdin);
        }

        // The path is passed through untouched: R and quarto both inherit `ir`'s
        // working directory, so a relative path resolves exactly as the user
        // typed it. (`fs::canonicalize` was avoided because on Windows it returns
        // a `\\?\C:\...` verbatim path that quarto's Deno `expandGlobSync` cannot
        // stat — `os error 123`.) Verify existence here for a clear error.
        let path = PathBuf::from(&script);
        fs::metadata(&path).map_err(|e| format!("cannot read script `{script}`: {e}"))?;
        if quarto::is_quarto_document(&path) {
            return Err("`ir run` does not render Quarto sources; use `ir render <source>`".into());
        }
        Ok(Self::Script(path))
    }

    pub(crate) fn script_spec(&self) -> Result<ScriptSpec, Box<dyn Error>> {
        match self {
            Self::Script(script) => read_r_script_spec(script),
            Self::Expressions(_) | Self::Stdin => Ok(ScriptSpec::default()),
        }
    }
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

    pub(crate) fn script_spec(&self) -> Result<ScriptSpec, Box<dyn Error>> {
        if quarto::is_quarto_document(&self.path) {
            return read_quarto_document_spec(&self.path);
        }
        if quarto::is_r_script(&self.path) {
            return read_quarto_script_spec(&self.path);
        }
        Ok(ScriptSpec::default())
    }

    pub(crate) fn reject_unsupported_rscript_args(
        &self,
        rscript_args: &[String],
    ) -> Result<(), Box<dyn Error>> {
        quarto::reject_comma_rscript_args(rscript_args)
    }
}

fn read_r_script_spec(script: &Path) -> Result<ScriptSpec, Box<dyn Error>> {
    parse_r_script_frontmatter(&read_r_script_frontmatter_to_string(script)?)
}

fn read_quarto_document_spec(script: &Path) -> Result<ScriptSpec, Box<dyn Error>> {
    parse_quarto_frontmatter(&quarto::read_to_string(script)?)
}

fn read_quarto_script_spec(script: &Path) -> Result<ScriptSpec, Box<dyn Error>> {
    parse_quarto_frontmatter(&read_quarto_script_frontmatter_to_string(script)?)
}

fn read_r_script_frontmatter_to_string(script: &Path) -> Result<String, Box<dyn Error>> {
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

    while let Some(rest) = line.strip_prefix("#| ") {
        frontmatter.push_str(rest);

        if read_next_line(&mut line)? == 0 {
            break;
        }
    }

    Ok(frontmatter)
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

fn parse_r_script_frontmatter(frontmatter: &str) -> Result<ScriptSpec, Box<dyn Error>> {
    if frontmatter.trim().is_empty() {
        return Ok(ScriptSpec::default());
    }

    let Some(doc) = load_first_yaml_document(frontmatter)? else {
        return Ok(ScriptSpec::default());
    };

    script_spec_from_yaml_mapping(&doc)
}

fn parse_quarto_frontmatter(document: &str) -> Result<ScriptSpec, Box<dyn Error>> {
    if document.trim().is_empty() {
        return Ok(ScriptSpec::default());
    }

    let Some(doc) = load_first_yaml_document(document)? else {
        return Ok(ScriptSpec::default());
    };
    if doc.is_null() {
        return Ok(ScriptSpec::default());
    }
    if !doc.is_mapping() {
        return Err("script frontmatter must be a YAML mapping".into());
    }

    let Some(spec_node) = doc.as_mapping_get("ir") else {
        return Ok(ScriptSpec::default());
    };
    if spec_node.is_null() {
        return Ok(ScriptSpec::default());
    }
    if !spec_node.is_mapping() {
        return Err("frontmatter `ir` must be a YAML mapping".into());
    }

    script_spec_from_yaml_mapping(spec_node)
}

fn script_spec_from_yaml_mapping(doc: &Yaml<'_>) -> Result<ScriptSpec, Box<dyn Error>> {
    if doc.is_null() {
        return Ok(ScriptSpec::default());
    }
    if !doc.is_mapping() {
        return Err("script frontmatter must be a YAML mapping".into());
    }

    Ok(ScriptSpec {
        dependencies: frontmatter_dependencies(doc)?,
        exclude_newer: frontmatter_optional_string(doc, "exclude-newer")?,
        isolated: frontmatter_optional_bool(doc, "isolated")?.unwrap_or(false),
        r_requirement: frontmatter_optional_string(doc, "r-version")?,
        // Quarto rendering is a property of the command, not the frontmatter.
        // cmd_render sets it after parsing.
        ..ScriptSpec::default()
    })
}

fn load_first_yaml_document(source: &str) -> Result<Option<Yaml<'_>>, Box<dyn Error>> {
    let mut parser = Parser::new_from_str(source);
    let mut loader = YamlLoader::default();
    parser
        .load(&mut loader, false)
        .map_err(|e| format!("could not parse script frontmatter as YAML: {e}"))?;
    Ok(loader.into_documents().into_iter().next())
}

fn frontmatter_dependencies(doc: &Yaml<'_>) -> Result<Vec<String>, Box<dyn Error>> {
    let Some(value) = doc.as_mapping_get("packages") else {
        return Ok(Vec::new());
    };
    if value.is_null() {
        return Ok(Vec::new());
    }

    let mut dependencies = Vec::new();
    let Some(seq) = value.as_vec() else {
        return Err("frontmatter `packages` must be a YAML sequence".into());
    };
    for item in seq {
        push_dependency_entry(&mut dependencies, item)?;
    }
    Ok(dependencies)
}

fn push_dependency_entry(
    dependencies: &mut Vec<String>,
    value: &Yaml<'_>,
) -> Result<(), Box<dyn Error>> {
    let Some(value) = value.as_str() else {
        return Err("frontmatter `packages` entries must be strings".into());
    };
    dependencies.push(value.to_owned());
    Ok(())
}

fn frontmatter_optional_bool(doc: &Yaml<'_>, key: &str) -> Result<Option<bool>, Box<dyn Error>> {
    let Some(value) = doc.as_mapping_get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    value
        .as_bool()
        .map(Some)
        .ok_or_else(|| format!("frontmatter `{key}` must be a boolean").into())
}

fn frontmatter_optional_string(
    doc: &Yaml<'_>,
    key: &str,
) -> Result<Option<String>, Box<dyn Error>> {
    let Some(value) = doc.as_mapping_get(key) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }

    let Some(value) = value.as_str() else {
        return Err(format!("frontmatter `{key}` must be a string").into());
    };
    let value = value.trim();
    Ok(if value.is_empty() {
        None
    } else {
        Some(value.to_owned())
    })
}
