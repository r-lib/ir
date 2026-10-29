use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

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
        std::process::exit(status.code().unwrap_or(2));
    }
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

fn run() -> io::Result<ExitCode> {
    let user_args = std::env::args_os().skip(1).collect::<Vec<_>>();
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
