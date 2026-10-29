use std::error::Error;
use std::io::{self, Write as _};

pub(crate) const QUICKSTART: &str = include_str!("quickstart.txt");

pub(crate) fn cmd_quickstart() -> Result<(), Box<dyn Error>> {
    io::stdout().write_all(QUICKSTART.as_bytes())?;
    Ok(())
}
