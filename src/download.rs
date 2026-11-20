//! Sequential artifact downloads. The caller supplies trusted metadata, destinations
//! under a launcher-owned directory, and a cancellable asynchronous transport.

mod batch;
mod integrity;

pub(crate) use batch::run_verified;

pub(crate) type Reporter = dyn Fn(Progress) -> BoxFuture<'static, ()> + Send + Sync;

pub use integrity::Checksum;

use futures::{
    channel::oneshot,
    future::{BoxFuture, Either, FutureExt, Shared, select},
    io::{AsyncRead, AsyncReadExt},
};
use std::{
    fmt, fs,
    future::Future,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    pin::pin,
    sync::Arc,
    time::{Duration, Instant},
};
use tempfile::{Builder, NamedTempFile};

pub struct Response {
    /// The transport must reject HTTP errors and incomplete response framing.
    pub body: Box<dyn AsyncRead + Send + Unpin>,
    /// Exact length of the delivered body, if known.
    pub total: Option<u64>,
}

pub struct Download {
    pub name: Arc<str>,
    pub destination: PathBuf,
    pub expected_size: Option<u64>,
    pub checksum: Option<Checksum>,
    pub existing: ExistingFile,
    /// Each call must open a fresh response. Dropping it must abandon the request.
    pub open: Box<dyn Fn() -> BoxFuture<'static, io::Result<Response>> + Send + Sync>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExistingFile {
    Preserve,
    ReplaceInvalid,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Stage {
    Resolving,
    Extracting,
    Checking,
    Receiving { received: u64, total: Option<u64> },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Progress {
    pub component: Arc<str>,
    pub step: usize,
    pub count: usize,
    pub stage: Stage,
    pub overall: Option<OverallProgress>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OverallProgress {
    pub bytes: u64,
    pub total_bytes: u64,
    pub files: usize,
    pub total_files: usize,
}

impl Progress {
    pub fn percent(&self) -> Option<f32> {
        if let Some(overall) = self.overall {
            return (overall.total_bytes != 0)
                .then(|| (overall.bytes as f64 * 100. / overall.total_bytes as f64) as f32);
        }
        match self.stage {
            Stage::Receiving {
                received,
                total: Some(total),
            } if total > 0 => Some((received as f64 * 100. / total as f64) as f32),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArtifactStatus {
    Reused,
    Verified,
    /// The transfer completed but no trusted checksum was supplied.
    Transferred,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Failure {
    Cancelled,
    Network(io::ErrorKind),
    Storage(io::ErrorKind),
    Cleanup(io::ErrorKind),
    SizeMismatch,
    ChecksumMismatch,
    ExistingFile,
    InvalidPlan,
}

impl fmt::Display for Failure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled => f.write_str("Download cancelled"),
            Self::Network(kind) => write!(f, "Network error ({kind})"),
            Self::Storage(kind) => write!(f, "File error ({kind})"),
            Self::Cleanup(kind) => write!(f, "Could not remove the temporary file ({kind})"),
            Self::SizeMismatch => f.write_str("Download has an unexpected size"),
            Self::ChecksumMismatch => f.write_str("Download failed checksum verification"),
            Self::InvalidPlan => f.write_str("Invalid verified download plan"),
            Self::ExistingFile => {
                f.write_str("An existing file differs from the expected contents and was preserved")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub component: Arc<str>,
    pub failure: Failure,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} — {}", self.component, self.failure)
    }
}

impl std::error::Error for Error {}

#[derive(Clone)]
pub struct Cancellation(Shared<oneshot::Receiver<()>>);

pub fn cancellation() -> (oneshot::Sender<()>, Cancellation) {
    let (sender, receiver) = oneshot::channel();
    (sender, Cancellation(receiver.shared()))
}

impl Cancellation {
    pub(crate) fn check(&self) -> Result<(), Failure> {
        if self.0.clone().now_or_never().is_some() {
            Err(Failure::Cancelled)
        } else {
            Ok(())
        }
    }

    pub(crate) async fn wait<T>(&self, work: impl Future<Output = T>) -> Result<T, Failure> {
        match select(self.0.clone(), pin!(work)).await {
            Either::Left(_) => Err(Failure::Cancelled),
            Either::Right((result, _)) => Ok(result),
        }
    }
}

/// A failed plan can be run again. Earlier files that pass checksum verification
/// will be reused. The caller must serialize plans sharing destinations and must
/// treat only verified artifacts as eligible for an installed inventory.
///
/// Run this future off the UI thread. It performs bounded synchronous file reads
/// and writes between async transport reads. The transport owns network timeouts.
pub async fn run<F: Future<Output = ()>>(
    plan: &[Download],
    cancel: Cancellation,
    mut report: impl FnMut(Progress) -> F,
) -> Result<Vec<ArtifactStatus>, Error> {
    let mut completed = Vec::with_capacity(plan.len());
    let mut buffer = vec![0; 64 * 1024];

    for (index, download) in plan.iter().enumerate() {
        let mut progress = Progress {
            component: download.name.clone(),
            step: index + 1,
            count: plan.len(),
            stage: Stage::Checking,
            overall: None,
        };
        let status = transfer(download, &cancel, &mut buffer, &mut progress, &mut report)
            .await
            .map_err(|failure| Error {
                component: download.name.clone(),
                failure,
            })?;
        completed.push(status);
    }
    Ok(completed)
}

async fn transfer<F: Future<Output = ()>>(
    download: &Download,
    cancel: &Cancellation,
    buffer: &mut [u8],
    progress: &mut Progress,
    report: &mut impl FnMut(Progress) -> F,
) -> Result<ArtifactStatus, Failure> {
    cancel.wait(report(progress.clone())).await?;
    if existing(download, cancel, buffer)? {
        return Ok(ArtifactStatus::Reused);
    }
    let response = cancel.wait((download.open)()).await?.map_err(network)?;
    if let (Some(expected), Some(advertised)) = (download.expected_size, response.total)
        && expected != advertised
    {
        return Err(Failure::SizeMismatch);
    }
    let parent = download
        .destination
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    cancel.check()?;
    fs::create_dir_all(parent).map_err(storage)?;
    let mut stage = Builder::new()
        .prefix(".averion-")
        .suffix(".part")
        .tempfile_in(parent)
        .map_err(storage)?;

    let result = receive(
        stage.as_file_mut(),
        response,
        download,
        cancel,
        buffer,
        progress,
        report,
    )
    .await;
    if let Err(failure) = result {
        return discard(stage, failure);
    }

    // There is no await between this check and publication. Once replacement
    // starts, this artifact has reached its commit point.
    if let Err(failure) = cancel.check() {
        return discard(stage, failure);
    }
    let status = if download.checksum.is_some() {
        ArtifactStatus::Verified
    } else {
        ArtifactStatus::Transferred
    };
    let published = match download.existing {
        ExistingFile::Preserve => stage.persist_noclobber(&download.destination),
        ExistingFile::ReplaceInvalid => stage.persist(&download.destination),
    };
    match published {
        Ok(_) => Ok(status),
        Err(error)
            if download.existing == ExistingFile::Preserve
                && error.error.kind() == io::ErrorKind::AlreadyExists =>
        {
            discard(error.file, Failure::ExistingFile)
        }
        Err(error) => discard(error.file, storage(error.error)),
    }
}

async fn receive<F: Future<Output = ()>>(
    file: &mut fs::File,
    mut response: Response,
    download: &Download,
    cancel: &Cancellation,
    buffer: &mut [u8],
    progress: &mut Progress,
    report: &mut impl FnMut(Progress) -> F,
) -> Result<(), Failure> {
    let total = download.expected_size.or(response.total);
    let mut hasher = download.checksum.as_ref().map(Checksum::hasher);
    let mut received = 0_u64;
    let mut reported = 0;
    let mut last_report = Instant::now();
    progress.stage = Stage::Receiving { received, total };
    cancel.wait(report(progress.clone())).await?;

    loop {
        let count = cancel
            .wait(response.body.read(buffer))
            .await?
            .map_err(network)?;
        if count == 0 {
            break;
        }
        received = received
            .checked_add(count as u64)
            .ok_or(Failure::SizeMismatch)?;
        if total.is_some_and(|size| received > size) {
            return Err(Failure::SizeMismatch);
        }
        file.write_all(&buffer[..count]).map_err(storage)?;
        if let Some(hash) = &mut hasher {
            hash.update(&buffer[..count]);
        }
        if reported == 0
            || total == Some(received)
            || last_report.elapsed() >= Duration::from_millis(100)
        {
            progress.stage = Stage::Receiving { received, total };
            cancel.wait(report(progress.clone())).await?;
            reported = received;
            last_report = Instant::now();
        }
    }

    if total.is_some_and(|size| received != size) {
        return Err(Failure::SizeMismatch);
    }
    if reported != received {
        progress.stage = Stage::Receiving { received, total };
        cancel.wait(report(progress.clone())).await?;
    }
    if let Some((hash, checksum)) = hasher.zip(download.checksum.as_ref())
        && !hash.matches(checksum)
    {
        return Err(Failure::ChecksumMismatch);
    }

    cancel.check()?;
    file.sync_all().map_err(storage)
}

fn existing(
    download: &Download,
    cancel: &Cancellation,
    buffer: &mut [u8],
) -> Result<bool, Failure> {
    match verify_existing(
        &download.destination,
        download.expected_size,
        download.checksum.as_ref(),
        cancel,
        buffer,
    )? {
        None => Ok(false),
        Some(true) => Ok(true),
        Some(false) if download.existing == ExistingFile::ReplaceInvalid => Ok(false),
        Some(false) => Err(Failure::ExistingFile),
    }
}

pub(crate) fn verify_existing(
    path: &Path,
    expected_size: Option<u64>,
    checksum: Option<&Checksum>,
    cancel: &Cancellation,
    buffer: &mut [u8],
) -> Result<Option<bool>, Failure> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !crate::game_directory::is_link(&metadata) => {
            metadata
        }
        Ok(_) => return Err(Failure::Storage(io::ErrorKind::InvalidInput)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(storage(error)),
    };
    let Some(checksum) = checksum else {
        return Ok(Some(false));
    };
    if expected_size.is_some_and(|size| size != metadata.len()) {
        return Ok(Some(false));
    }

    let mut file = fs::File::open(path).map_err(storage)?;
    let mut hasher = checksum.hasher();
    let mut received = 0_u64;
    loop {
        cancel.check()?;
        let count = file.read(buffer).map_err(storage)?;
        if count == 0 {
            break;
        }
        received += count as u64;
        hasher.update(&buffer[..count]);
    }
    cancel.check()?;
    Ok(Some(
        expected_size.is_none_or(|size| size == received) && hasher.matches(checksum),
    ))
}

fn network(error: io::Error) -> Failure {
    Failure::Network(error.kind())
}

fn storage(error: io::Error) -> Failure {
    Failure::Storage(error.kind())
}

fn discard(stage: NamedTempFile, cause: Failure) -> Result<ArtifactStatus, Failure> {
    stage
        .close()
        .map_err(|error| Failure::Cleanup(error.kind()))?;
    Err(cause)
}

#[cfg(test)]
mod tests;
