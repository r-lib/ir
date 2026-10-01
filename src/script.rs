use std::error::Error;
use std::fs::{self, File};
use std::io::{BufRead, BufReader};
use std::ops::Range;
use std::path::{Path, PathBuf};

use crate::quarto;
use crate::spec::{parse_r_frontmatter, RuntimeSpec};

pub(crate) struct RScriptHeader {
    pub(crate) shebang_end: Option<usize>,
    pub(crate) frontmatter: Option<Range<usize>>,
}

pub(crate) fn r_script_header(contents: &[u8]) -> RScriptHeader {
    let shebang_end = contents.starts_with(b"#!").then(|| line_end(contents, 0));
    let frontmatter_start = shebang_end.unwrap_or(0);
    let mut cursor = frontmatter_start;
    let mut found = false;
    while contents
        .get(cursor..)
        .is_some_and(|rest| rest.starts_with(b"#| "))
    {
        found = true;
        cursor = line_end(contents, cursor);
    }
    RScriptHeader {
        shebang_end,
        frontmatter: found.then_some(frontmatter_start..cursor),
    }
}

fn line_end(contents: &[u8], start: usize) -> usize {
    contents[start..]
        .iter()
        .position(|byte| *byte == b'\n')
        .map_or(contents.len(), |position| start + position + 1)
}

/// Where the user's program comes from.
pub(crate) enum RunSource {
    Script(PathBuf),
    Expressions(Vec<String>),
    Stdin,
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

    pub(crate) fn script_spec(&self) -> Result<RuntimeSpec, Box<dyn Error>> {
        match self {
            Self::Script(script) => read_r_script_spec(script),
            Self::Expressions(_) | Self::Stdin => Ok(RuntimeSpec::default()),
        }
    }
}

fn read_r_script_spec(script: &Path) -> Result<RuntimeSpec, Box<dyn Error>> {
    parse_r_frontmatter(&read_r_script_frontmatter_to_string(script)?)
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
