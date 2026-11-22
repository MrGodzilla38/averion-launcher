use crate::download;
use futures::TryStreamExt;
use reqwest::{Client, StatusCode, Url, header::LOCATION, redirect, tls};
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
    InvalidRedirect,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Initialize => f.write_str("Could not initialize HTTPS"),
            Self::Timeout => f.write_str("The HTTPS request timed out"),
            Self::Transport => f.write_str("Could not complete the HTTPS request"),
            Self::Status(status) => write!(f, "Download server returned HTTP {}", status.as_u16()),
            Self::InvalidRedirect => f.write_str("Invalid GitHub release asset redirect"),
            Self::TooLarge => f.write_str("Download metadata exceeds the size limit"),
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

    pub(crate) async fn size(&self, url: Url) -> Result<u64, Error> {
        let response = self
            .client()?
            .head(url)
            .timeout(METADATA_TIMEOUT)
            .send()
            .await
            .map_err(request_error)?;
        if response.status() != StatusCode::OK {
            return Err(Error::Status(response.status()));
        }
        response
            .headers()
            .get(reqwest::header::CONTENT_LENGTH)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse().ok())
            .ok_or(Error::Transport)
    }

    pub(crate) async fn download(&self, url: Url) -> io::Result<download::Response> {
        let response = self.get(url, DOWNLOAD_TIMEOUT).await.map_err(io_error)?;
        Ok(download_response(response))
    }

    pub(crate) async fn github_release_asset(&self, url: Url) -> io::Result<download::Response> {
        let response = self
            .client()
            .map_err(io_error)?
            .get(url)
            .timeout(DOWNLOAD_TIMEOUT)
            .send()
            .await
            .map_err(|error| io_error(request_error(error)))?;
        let response = match response.status() {
            StatusCode::OK => response,
            StatusCode::FOUND | StatusCode::TEMPORARY_REDIRECT => {
                let target = response
                    .headers()
                    .get(LOCATION)
                    .and_then(|value| value.to_str().ok())
                    .and_then(|value| Url::parse(value).ok())
                    .filter(github_asset_target)
                    .ok_or_else(|| io_error(Error::InvalidRedirect))?;
                self.get(target, DOWNLOAD_TIMEOUT).await.map_err(io_error)?
            }
            status => return Err(io_error(Error::Status(status))),
        };
        Ok(download_response(response))
    }
}

fn github_asset_target(url: &Url) -> bool {
    url.scheme() == "https"
        && url.host_str() == Some("release-assets.githubusercontent.com")
        && url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && url.fragment().is_none()
}

fn download_response(response: reqwest::Response) -> download::Response {
    let total = response.content_length();
    let reader = response
        .bytes_stream()
        .map_err(|error| io_error(request_error(error)))
        .into_async_read();
    download::Response {
        body: Box::new(reader),
        total,
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
