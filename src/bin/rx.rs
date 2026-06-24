use std::ffi::{OsStr, OsString};
use std::io::{self, Write as _};
use std::path::{Path, PathBuf};
#[cfg(not(unix))]
use std::process::ExitStatus;
use std::process::{Command, ExitCode};

const QUICKSTART: &str = include_str!("../rx_quickstart.txt");
const QUICKSTART_HELP: &str = "Show a concise usage guide for AI agents\n\nUsage: rx quickstart\n\nOptions:\n  -h, --help  Print help\n";

fn exec_or_status(cmd: &mut Command) -> io::Result<ExitCode> {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        let err = cmd.exec();
        Err(err)
    }

    #[cfg(not(unix))]
    {
        let status = cmd.status()?;
        Ok(exit_code(status))
    }
}

#[cfg(not(unix))]
fn exit_code(status: ExitStatus) -> ExitCode {
    u8::try_from(status.code().unwrap_or(2)).unwrap_or(2).into()
}

fn ir_path(rx: &Path) -> std::io::Result<PathBuf> {
    let Some(bin) = rx.parent() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "could not determine the location of the `rx` binary",
        ));
    };

    let ir = bin.join(format!("ir{}", std::env::consts::EXE_SUFFIX));
    if matches!(ir.try_exists(), Ok(false)) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("could not find the `ir` binary at: {}", ir.display()),
        ));
    }

    Ok(ir)
}

fn is_arg(arg: &OsString, expected: &str) -> bool {
    arg == OsStr::new(expected)
}

fn cmd_quickstart(args: &[OsString]) -> io::Result<ExitCode> {
    match args {
        [] => {
            io::stdout().write_all(QUICKSTART.as_bytes())?;
            Ok(ExitCode::SUCCESS)
        }
        [arg] if is_arg(arg, "--help") || is_arg(arg, "-h") => {
            io::stdout().write_all(QUICKSTART_HELP.as_bytes())?;
            Ok(ExitCode::SUCCESS)
        }
        [arg, ..] => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "unexpected argument '{}' for `rx quickstart`",
                arg.to_string_lossy()
            ),
        )),
    }
}

fn run() -> io::Result<ExitCode> {
    let user_args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if user_args
        .first()
        .is_some_and(|arg| is_arg(arg, "quickstart"))
    {
        return cmd_quickstart(&user_args[1..]);
    }

    let current_exe = std::env::current_exe()?;
    let ir = ir_path(&current_exe)?;
    let args = ["tool", "rx"]
        .iter()
        .map(OsString::from)
        .chain(user_args)
        .collect::<Vec<_>>();

    let mut cmd = Command::new(ir);
    cmd.args(&args);
    exec_or_status(&mut cmd)
}

fn main() -> ExitCode {
    match run() {
        Ok(status) => status,
        Err(err) => {
            eprintln!("rx: {err}");
            ExitCode::from(2)
        }
    }
}
