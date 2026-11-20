use crate::download;
use futures::TryStreamExt;
use reqwest::{Client, StatusCode, Url, redirect, tls};
use std::{
    fmt, io,
    sync::{Arc, OnceLock},
    time::Duration,
};

const METADATA_TIMEOUT: Duration = Duration::from_secs(45);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(30 * 60);

#[derive(Clone, Default)]
pub(crate) struct Https(Arc<OnceLock<Client>>);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Error {
    Initialize,
    Timeout,
    Transport,
    Status(StatusCode),
    TooLarge,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initialize => f.write_str("Could not initialize HTTPS"),
            Self::Timeout => f.write_str("The HTTPS request timed out"),
            Self::Transport => f.write_str("Could not complete the HTTPS request"),
            Self::Status(status) => write!(f, "Mojang returned HTTP {}", status.as_u16()),
            Self::TooLarge => f.write_str("Mojang metadata exceeds the size limit"),
        }
    }
}

impl Https {
    fn client(&self) -> Result<&Client, Error> {
        if let Some(client) = self.0.get() {
            return Ok(client);
        }
        let client = Client::builder()
            .https_only(true)
            .redirect(redirect::Policy::none())
            .min_tls_version(tls::Version::TLS_1_2)
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(30))
            .user_agent(concat!("AverionLauncher/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|_| Error::Initialize)?;
        Ok(self.0.get_or_init(|| client))
    }

    async fn get(&self, url: Url, timeout: Duration) -> Result<reqwest::Response, Error> {
        let response = self
            .client()?
            .get(url)
            .timeout(timeout)
            .send()
            .await
            .map_err(request_error)?;
        // No partial responses or redirects are useful for a full artifact request.
        if response.status() != StatusCode::OK {
            return Err(Error::Status(response.status()));
        }
        Ok(response)
    }

    pub(crate) async fn metadata(&self, url: Url, limit: usize) -> Result<Vec<u8>, Error> {
        let mut response = self.get(url, METADATA_TIMEOUT).await?;
        if response
            .content_length()
            .is_some_and(|size| size > limit as u64)
        {
            return Err(Error::TooLarge);
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(request_error)? {
            if chunk.len() > limit - bytes.len() {
                return Err(Error::TooLarge);
            }
            bytes.extend_from_slice(&chunk);
        }
        Ok(bytes)
    }

    pub(crate) async fn download(&self, url: Url) -> io::Result<download::Response> {
        let response = self.get(url, DOWNLOAD_TIMEOUT).await.map_err(io_error)?;
        let total = response.content_length();
        let reader = response
            .bytes_stream()
            .map_err(|error| io_error(request_error(error)))
            .into_async_read();
        Ok(download::Response {
            body: Box::new(reader),
            total,
        })
    }
}

fn request_error(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::Timeout
    } else {
        Error::Transport
    }
}

fn io_error(error: Error) -> io::Error {
    let kind = match error {
        Error::Timeout => io::ErrorKind::TimedOut,
        Error::Status(StatusCode::NOT_FOUND) => io::ErrorKind::NotFound,
        Error::Status(StatusCode::FORBIDDEN | StatusCode::UNAUTHORIZED) => {
            io::ErrorKind::PermissionDenied
        }
        _ => io::ErrorKind::Other,
    };
    io::Error::new(kind, error.to_string())
}
