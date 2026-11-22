mod arguments;

use crate::{account, download, game_directory::GameDirectory, mojang};
use futures::SinkExt;
use std::{io, path::Path, process::ExitStatus};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
};

const MAX_STREAM_BYTES: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug)]
pub enum Event {
    Started { log: String },
    Finished(Result<Exit, Error>),
}

#[derive(Clone, Debug)]
pub struct Exit {
    pub status: ExitStatus,
    pub log: String,
    pub output_error: bool,
}

#[derive(Clone, Debug)]
pub enum Error {
    Installation { message: String, repairable: bool },
    Account(String),
    Io(String),
}

impl From<mojang::Error> for Error {
    fn from(error: mojang::Error) -> Self {
        Self::Installation {
            message: error.to_string(),
            repairable: error.repairable(),
        }
    }
}

pub(crate) fn start(version: &'static str) -> impl futures::Stream<Item = Event> {
    iced::stream::channel(2, async move |mut events| {
        let result = run(version, &mut events).await;
        let _ = events.send(Event::Finished(result)).await;
    })
}

async fn run(
    version: &str,
    events: &mut futures::channel::mpsc::Sender<Event>,
) -> Result<Exit, Error> {
    let (_sender, cancel) = download::cancellation();
    let validation = cancel.clone();
    let version = version.to_owned();
    let (directory, _lock, game, session) = tokio::task::spawn_blocking(move || {
        let directory = GameDirectory::discover().map_err(io_error)?;
        let lock = directory.lock_operation().map_err(io_error)?;
        let account = account::load(&directory)
            .map_err(|error| {
                Error::Account(format!(
                    "Could not load the offline account ({})",
                    error.kind()
                ))
            })?
            .ok_or_else(|| {
                Error::Account("Save an offline username in Accounts before playing.".into())
            })?;
        directory.prepare().map_err(io_error)?;
        let game = mojang::prepare(&directory, &version)?;
        mojang::verify(&game, &validation)?;
        Ok::<_, Error>((directory, lock, game, account.session()))
    })
    .await
    .map_err(|_| Error::Io("Installation validation did not finish.".into()))??;

    let major = game.java.major;
    let runtime = match mojang::probe_java(game.java.executable.clone(), major, &cancel)
        .await
        .map_err(mojang::Error::from)?
    {
        Some(runtime) => runtime,
        None => mojang::probe_system_java(major, &directory.java(), &cancel)
            .await
            .map_err(mojang::Error::from)?
            .ok_or(mojang::Error::Java(
                "compatible Java is missing or no longer works",
            ))?,
    };
    let output = tempfile::Builder::new()
        .prefix("minecraft-")
        .tempdir_in(directory.logs())
        .map_err(io_error)?;
    let mut command = Command::new(runtime.executable);
    command
        .args(arguments::resolve(
            &game,
            &session,
            mojang::Platform::current()?,
            &output.path().join("hs_err_pid%p.log"),
        )?)
        .current_dir(&game.game_directory);
    mojang::configure_java(&mut command);
    let (stdout, stderr) = open_logs(output.path()).await.map_err(io_error)?;
    let mut child = command
        .spawn()
        .map_err(|error| Error::Io(format!("Could not start Minecraft ({})", error.kind())))?;
    let log_path = output.keep();
    let log = format!("logs/{}", log_path.file_name().unwrap().to_string_lossy());
    let _ = events.send(Event::Started { log: log.clone() }).await;

    monitor(&mut child, log, stdout, stderr).await
}

async fn monitor(
    child: &mut Child,
    log: String,
    stdout: tokio::fs::File,
    stderr: tokio::fs::File,
) -> Result<Exit, Error> {
    let out_pipe = child.stdout.take().expect("stdout is piped");
    let err_pipe = child.stderr.take().expect("stderr is piped");
    let (status, out, err) = futures::join!(
        child.wait(),
        capture(out_pipe, stdout),
        capture(err_pipe, stderr)
    );
    Ok(Exit {
        status: status.map_err(io_error)?,
        log,
        output_error: out.is_err() || err.is_err(),
    })
}

async fn open_logs(path: &Path) -> io::Result<(tokio::fs::File, tokio::fs::File)> {
    futures::try_join!(
        tokio::fs::File::create(path.join("stdout.log")),
        tokio::fs::File::create(path.join("stderr.log")),
    )
}

async fn capture(pipe: impl AsyncRead + Unpin, mut output: tokio::fs::File) -> io::Result<()> {
    let mut limited = pipe.take(MAX_STREAM_BYTES);
    let written = tokio::io::copy(&mut limited, &mut output).await;
    // Keep draining even after reaching the limit or a disk write fails.
    let discarded = tokio::io::copy(&mut limited.into_inner(), &mut tokio::io::sink()).await?;
    written?;
    if discarded > 0 {
        output
            .write_all(b"\n[Averion truncated this stream after 16 MiB]\n")
            .await?;
    }
    output.flush().await
}

fn io_error(error: io::Error) -> Error {
    let message = if error.kind() == io::ErrorKind::WouldBlock {
        error.to_string()
    } else {
        format!(
            "Could not access launch files or monitor Minecraft ({})",
            error.kind()
        )
    };
    Error::Io(message)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn captures_both_pipes_and_reports_a_nonzero_exit() {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                let directory = tempfile::tempdir().unwrap();
                let mut command = Command::new("/bin/sh");
                command.args([
                    "-c",
                    "printf 'game output'; printf 'crash detail' >&2; exit 17",
                ]);
                let argument_count = command.as_std().get_args().len();
                mojang::configure_java(&mut command);
                assert_eq!(command.as_std().get_args().len(), argument_count);
                let (stdout, stderr) = open_logs(directory.path()).await.unwrap();
                let mut child = command.spawn().unwrap();
                let exit = monitor(&mut child, "logs/fixture".into(), stdout, stderr)
                    .await
                    .unwrap();
                assert_eq!(exit.status.code(), Some(17));
                assert!(!exit.output_error);
                assert_eq!(
                    std::fs::read(directory.path().join("stdout.log")).unwrap(),
                    b"game output"
                );
                assert_eq!(
                    std::fs::read(directory.path().join("stderr.log")).unwrap(),
                    b"crash detail"
                );
            });
    }
}
