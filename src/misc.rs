use crate::errors::FileError;
use async_std::fs::File;
use futures::prelude::*;
use std::{
    ffi::OsStr,
    io,
    os::unix::process::ExitStatusExt,
    path::Path,
    process::{ExitStatus, Stdio},
};

use tokio::net::process::Command;

/// Asynchronously execute a command and wait for its exit status.
pub async fn check_call<'a, S: AsRef<OsStr>>(
    cmd: &'a str,
    args: &'a [S],
    cwd: Option<&'a Path>,
) -> io::Result<()> {
    let mut command = Command::new(cmd);

    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let status = dbg!(command.args(args)).status().await?;

    eval_status(cmd, status)
}

/// Asynchronously fetch the UTF-8 stdout output of a command.
pub async fn check_output<'a>(
    cmd: &'a str,
    args: &'a [&'a str],
    cwd: Option<&'a Path>,
) -> io::Result<String> {
    let mut command = Command::new(cmd);

    if let Some(cwd) = cwd {
        command.current_dir(cwd);
    }

    let output = command.args(args).output().await?;

    eval_status(cmd, output.status).and_then(|_| {
        String::from_utf8(output.stdout).map_err(|_| {
            io::Error::new(
                io::ErrorKind::Other,
                format!("{} output was not UTF-8", cmd),
            )
        })
    })
}

/// Asynchronously create a file and write to it, with high level errors.
pub async fn create_and_write(file: &Path, bytes: &[u8]) -> Result<(), FileError> {
    File::create(file)
        .await
        .map_err(|source| FileError::CreateFile {
            file: file.into(),
            source,
        })?
        .write_all(bytes)
        .await
        .map_err(|source| FileError::WriteFile {
            file: file.into(),
            source,
        })?;

    Ok(())
}

fn eval_status(cmd: &str, status: ExitStatus) -> io::Result<()> {
    if status.success() {
        Ok(())
    } else {
        let source = match status.code() {
            Some(code) => io::Error::new(
                io::ErrorKind::Other,
                format!("{} exited with status of {}", cmd, code),
            ),
            None => match status.signal() {
                Some(signal) => io::Error::new(
                    io::ErrorKind::Other,
                    format!("{} terminated with signal {}", cmd, signal),
                ),
                None => io::Error::new(io::ErrorKind::Other, format!("{} status is unknown", cmd)),
            },
        };

        Err(source)
    }
}
