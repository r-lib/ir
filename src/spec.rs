use std::error::Error;

use saphyr::{Yaml, YamlLoader};
use saphyr_parser::Parser;

#[derive(Debug, Default)]
pub(crate) struct RuntimeSpec {
    pub(crate) dependencies: Vec<String>,
    pub(crate) exclude_newer: Option<String>,
    pub(crate) isolated: bool,
    pub(crate) r_requirement: Option<String>,
    pub(crate) python: Option<PythonSpec>,
    // A Quarto render needs rmarkdown injected for the knitr engine.
    pub(crate) quarto_render: bool,
    // A knitr-backed Quarto render with Python chunks can use reticulate.
    pub(crate) quarto_reticulate: bool,
}

#[derive(Debug, Default)]
pub(crate) struct PythonSpec {
    pub(crate) packages: Vec<String>,
    pub(crate) python_version: Option<String>,
    pub(crate) exclude_newer: Option<String>,
    pub(crate) exclude_newer_explicit: bool,
}

pub(crate) fn parse_r_frontmatter(frontmatter: &str) -> Result<RuntimeSpec, Box<dyn Error>> {
    if frontmatter.trim().is_empty() {
        return Ok(RuntimeSpec::default());
    }

    let Some(doc) = load_first_yaml_document(frontmatter, "script frontmatter")? else {
        return Ok(RuntimeSpec::default());
    };

    runtime_spec_from_yaml_mapping(&doc, "")
}

pub(crate) fn parse_quarto_frontmatter(document: &str) -> Result<RuntimeSpec, Box<dyn Error>> {
    if document.trim().is_empty() {
        return Ok(RuntimeSpec::default());
    }

    let Some(doc) = load_first_yaml_document(document, "script frontmatter")? else {
        return Ok(RuntimeSpec::default());
    };
    if doc.is_null() {
        return Ok(RuntimeSpec::default());
    }
    if !doc.is_mapping() {
        return Err("script frontmatter must be a YAML mapping".into());
    }

    let Some(spec_node) = doc.as_mapping_get("ir") else {
        return Ok(RuntimeSpec::default());
    };
    if spec_node.is_null() {
        return Ok(RuntimeSpec::default());
    }
    if !spec_node.is_mapping() {
        return Err("frontmatter `ir` must be a YAML mapping".into());
    }

    runtime_spec_from_yaml_mapping(spec_node, "ir.")
}

fn runtime_spec_from_yaml_mapping(
    doc: &Yaml<'_>,
    key_prefix: &str,
) -> Result<RuntimeSpec, Box<dyn Error>> {
    if doc.is_null() {
        return Ok(RuntimeSpec::default());
    }
    if !doc.is_mapping() {
        return Err("script frontmatter must be a YAML mapping".into());
    }
    reject_frontmatter_rscript(doc, key_prefix)?;

    Ok(RuntimeSpec {
        dependencies: frontmatter_dependencies(doc)?,
        exclude_newer: frontmatter_optional_string(doc, "exclude-newer")?,
        isolated: frontmatter_optional_bool(doc, "isolated")?.unwrap_or(false),
        r_requirement: frontmatter_optional_string(doc, "r-version")?,
        python: frontmatter_python_spec(doc, key_prefix)?,
        // Quarto rendering is a property of the command, not the frontmatter.
        // cmd_render sets it after parsing.
        ..RuntimeSpec::default()
    })
}

fn reject_frontmatter_rscript(doc: &Yaml<'_>, key_prefix: &str) -> Result<(), Box<dyn Error>> {
    if doc.as_mapping_get("rscript").is_some() {
        return Err(format!(
            "frontmatter `{}` is no longer supported. Use `--rscript` or `IR_RSCRIPT` instead.",
            frontmatter_key(key_prefix, "rscript")
        )
        .into());
    }
    Ok(())
}

pub(crate) fn load_first_yaml_document<'a>(
    source: &'a str,
    context: &str,
) -> Result<Option<Yaml<'a>>, Box<dyn Error>> {
    let mut parser = Parser::new_from_str(source);
    let mut loader = YamlLoader::default();
    parser
        .load(&mut loader, false)
        .map_err(|e| format!("could not parse {context} as YAML: {e}"))?;
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

fn frontmatter_python_spec(
    doc: &Yaml<'_>,
    python_key_prefix: &str,
) -> Result<Option<PythonSpec>, Box<dyn Error>> {
    let packages = frontmatter_python_packages(doc, python_key_prefix)?;
    let python_version = frontmatter_optional_string(doc, "python-version")?;
    let python_exclude_newer =
        frontmatter_optional_raw_string_with_presence(doc, "python-exclude-newer")?;

    if packages.is_none() && python_version.is_none() {
        return Ok(None);
    }

    Ok(Some(PythonSpec {
        packages: packages.unwrap_or_default(),
        python_version,
        exclude_newer: python_exclude_newer.0,
        exclude_newer_explicit: python_exclude_newer.1,
    }))
}

fn frontmatter_python_packages(
    doc: &Yaml<'_>,
    python_key_prefix: &str,
) -> Result<Option<Vec<String>>, Box<dyn Error>> {
    let Some(value) = doc.as_mapping_get("python-packages") else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(Some(Vec::new()));
    }
    let mut packages = Vec::new();
    let Some(seq) = value.as_vec() else {
        return Err(format!(
            "frontmatter `{}` must be a YAML sequence",
            frontmatter_key(python_key_prefix, "python-packages")
        )
        .into());
    };
    for item in seq {
        let Some(value) = item.as_str() else {
            return Err(format!(
                "frontmatter `{}` entries must be strings",
                frontmatter_key(python_key_prefix, "python-packages")
            )
            .into());
        };
        packages.push(value.to_owned());
    }
    Ok(Some(packages))
}

fn frontmatter_key(prefix: &str, key: &str) -> String {
    format!("{prefix}{key}")
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

fn frontmatter_optional_raw_string_with_presence(
    doc: &Yaml<'_>,
    key: &str,
) -> Result<(Option<String>, bool), Box<dyn Error>> {
    let Some(value) = doc.as_mapping_get(key) else {
        return Ok((None, false));
    };
    if value.is_null() {
        return Ok((None, true));
    }

    value
        .as_str()
        .map(|value| (Some(value.to_owned()), true))
        .ok_or_else(|| format!("frontmatter `{key}` must be a string").into())
}
