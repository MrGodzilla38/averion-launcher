//! Resolves the selected Mojang stable release into verified download tasks.

mod assets;
mod cache;
mod java;
mod libraries;

pub use java::Runtime;
mod natives;

use crate::{
    download::{
        self, ArtifactStatus, Cancellation, Checksum, Download, ExistingFile, Failure, Progress,
        Reporter, Stage,
    },
    game_directory::GameDirectory,
    http::{self, Https},
};
use futures::FutureExt;
use reqwest::Url;
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::{
    fmt, io,
    path::{Path, PathBuf},
    time::Duration,
};

use libraries::{Library, Platform};

const MANIFEST_URL: &str = "https://piston-meta.mojang.com/mc/game/version_manifest_v2.json";
const METADATA_HOSTS: &[&str] = &["piston-meta.mojang.com", "launchermeta.mojang.com"];
const CLIENT_HOSTS: &[&str] = &["piston-data.mojang.com", "launcher.mojang.com"];
const MANIFEST_AGE: Duration = Duration::from_secs(24 * 60 * 60);
const MANIFEST_LIMIT: usize = 4 * 1024 * 1024;
const VERSION_LIMIT: usize = 2 * 1024 * 1024;

#[derive(Clone, Default)]
pub(crate) struct Mojang {
    https: Https,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Error {
    Directory(io::ErrorKind),
    Cache(io::ErrorKind),
    Https(http::Error),
    MissingRelease,
    Invalid(&'static str),
    Native(&'static str),
    Java(&'static str),
    Download(download::Error),
}

impl Error {
    fn directory(error: io::Error) -> Self {
        Self::Directory(error.kind())
    }

    fn cache(error: io::Error) -> Self {
        Self::Cache(error.kind())
    }
}

impl From<Failure> for Error {
    fn from(failure: Failure) -> Self {
        Self::Download(download::Error {
            component: "Minecraft files".into(),
            failure,
        })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Directory(kind) => {
                write!(f, "Could not prepare the Averion data directory ({kind})")
            }
            Self::Cache(kind) => write!(f, "Could not access the Mojang metadata cache ({kind})"),
            Self::Https(error) => error.fmt(f),
            Self::MissingRelease => f.write_str("Mojang did not list this stable release"),
            Self::Invalid(reason) => write!(f, "Invalid Mojang metadata ({reason})"),
            Self::Native(reason) => write!(f, "Could not extract natives ({reason})"),
            Self::Java(reason) => write!(f, "Could not prepare Java ({reason})"),
            Self::Download(error) => error.fmt(f),
        }
    }
}

#[derive(Deserialize)]
struct Manifest {
    versions: Vec<Release>,
}

#[derive(Deserialize)]
struct Release {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    url: String,
    sha1: String,
}

#[derive(Deserialize)]
struct VersionJson {
    id: String,
    #[serde(rename = "type")]
    kind: String,
    downloads: ClientDownloads,
    #[serde(rename = "assetIndex")]
    asset_index: AssetIndex,
    libraries: Vec<Library>,
    #[serde(rename = "javaVersion")]
    java_version: java::Requirement,
}

#[derive(Deserialize)]
struct ClientDownloads {
    client: ArtifactJson,
}

#[derive(Deserialize)]
struct AssetIndex {
    id: String,
    #[serde(flatten)]
    artifact: ArtifactJson,
}

#[derive(Deserialize)]
struct ArtifactJson {
    sha1: String,
    size: u64,
    url: String,
}

struct Artifact {
    sha1: [u8; 20],
    size: u64,
    url: Url,
}

struct Version {
    client: Artifact,
    index_id: String,
    index: Artifact,
    libraries: Vec<Library>,
    java_version: java::Requirement,
}

struct InstallationPlan {
    index: Download,
    files: Vec<Download>,
    natives: Vec<natives::NativeArchive>,
    version_dir: PathBuf,
    platform: Platform,
    java_version: java::Requirement,
}

impl Mojang {
    pub(crate) async fn install(
        &self,
        selected: &str,
        cancel: &Cancellation,
        report: &Reporter,
    ) -> Result<Runtime, Error> {
        cancel.check()?;
        let directory = GameDirectory::discover().map_err(Error::directory)?;
        self.install_in(selected, &directory, cancel, report).await
    }

    async fn install_in(
        &self,
        selected: &str,
        directory: &GameDirectory,
        cancel: &Cancellation,
        report: &Reporter,
    ) -> Result<Runtime, Error> {
        let plan = cancel
            .wait(self.resolve_in(selected, directory, cancel))
            .await??;
        self.execute(selected, directory, plan, cancel, report)
            .await
    }

    async fn execute(
        &self,
        selected: &str,
        directory: &GameDirectory,
        mut plan: InstallationPlan,
        cancel: &Cancellation,
        report: &Reporter,
    ) -> Result<Runtime, Error> {
        let index_size = plan
            .index
            .expected_size
            .ok_or(Error::Invalid("missing asset index size"))?;
        let index_status = download::run(std::slice::from_ref(&plan.index), cancel.clone(), report)
            .await
            .map_err(Error::Download)?;
        if !matches!(
            index_status.as_slice(),
            [ArtifactStatus::Reused | ArtifactStatus::Verified]
        ) {
            return Err(Error::Invalid("unverified asset index"));
        }
        cancel
            .wait(report(Progress {
                component: format!("Minecraft {selected} asset index").into(),
                step: 0,
                count: 0,
                stage: Stage::Resolving,
                overall: None,
            }))
            .await?;
        let asset_dir = directory.asset_objects().map_err(Error::directory)?;
        assets::resolve(
            &plan.index.destination,
            &asset_dir,
            &self.https,
            cancel,
            &mut plan.files,
        )?;
        let overall = download::run_verified(&plan.files, Some(index_size), cancel, report)
            .await
            .map_err(Error::Download)?;
        for (index, native) in plan.natives.iter().enumerate() {
            cancel
                .wait(report(Progress {
                    component: native.name.clone(),
                    step: index + 1,
                    count: plan.natives.len(),
                    stage: Stage::Extracting,
                    overall: Some(overall),
                }))
                .await?;
            natives::extract(native, &plan.version_dir, plan.platform, cancel)?;
        }
        java::resolve(&plan.java_version, directory, &self.https, cancel, report).await
    }

    async fn resolve_in(
        &self,
        selected: &str,
        directory: &GameDirectory,
        cancel: &Cancellation,
    ) -> Result<InstallationPlan, Error> {
        validate_id(selected)?;
        directory.prepare().map_err(Error::directory)?;
        let cache_dir = directory.metadata_cache();
        let url = trusted_url(MANIFEST_URL, METADATA_HOSTS)?;
        let (hash, hash_hex, url) = cache::load(
            &cache_dir.join("version-manifest-v2.cache"),
            Some(MANIFEST_AGE),
            MANIFEST_LIMIT,
            |bytes| {
                let manifest = Manifest::parse(bytes)?;
                let release = manifest.release(selected)?;
                Ok((
                    parse_sha1(&release.sha1)?,
                    release.sha1.to_ascii_lowercase(),
                    trusted_url(&release.url, METADATA_HOSTS)?,
                ))
            },
            self.https.metadata(url, MANIFEST_LIMIT),
        )
        .await?;
        let version = cache::load(
            &cache_dir.join(format!("{selected}-{hash_hex}.cache")),
            None,
            VERSION_LIMIT,
            |bytes| Version::parse(bytes, selected, hash),
            self.https.metadata(url, VERSION_LIMIT),
        )
        .await?;
        let version_dir = directory
            .version_directory(selected)
            .map_err(Error::directory)?;
        let platform = Platform::current()?;
        let libraries = libraries::resolve(
            version.libraries,
            platform,
            &directory.libraries(),
            &self.https,
            cancel,
        )?;
        let mut files = Vec::with_capacity(libraries.downloads.len() + 1);
        files.push(version.client.download(
            format!("Minecraft {selected} client"),
            version_dir.join(format!("{selected}.jar")),
            &self.https,
        ));
        files.extend(libraries.downloads);
        let index = version.index.download(
            format!("Minecraft {selected} asset index"),
            directory.asset_index(&version.index_id),
            &self.https,
        );
        Ok(InstallationPlan {
            index,
            files,
            natives: libraries.natives,
            version_dir,
            platform,
            java_version: version.java_version,
        })
    }
}

impl Manifest {
    fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let manifest: Self =
            serde_json::from_slice(bytes).map_err(|_| Error::Invalid("version manifest"))?;
        if manifest.versions.is_empty() {
            return Err(Error::Invalid("empty version list"));
        }
        Ok(manifest)
    }

    fn release(&self, selected: &str) -> Result<&Release, Error> {
        let mut matches = self
            .versions
            .iter()
            .filter(|release| release.id == selected);
        let release = matches.next().ok_or(Error::MissingRelease)?;
        if matches.next().is_some() {
            return Err(Error::Invalid("duplicate version ID"));
        }
        if release.kind != "release" {
            return Err(Error::MissingRelease);
        }
        Ok(release)
    }
}

impl Version {
    fn parse(bytes: &[u8], selected: &str, sha1: [u8; 20]) -> Result<Self, Error> {
        if Sha1::digest(bytes)[..] != sha1 {
            return Err(Error::Invalid("version metadata checksum"));
        }
        let value: VersionJson =
            serde_json::from_slice(bytes).map_err(|_| Error::Invalid("version JSON"))?;
        if value.id != selected || value.kind != "release" {
            return Err(Error::Invalid("version ID or release type"));
        }
        validate_id(&value.asset_index.id)?;
        value.java_version.validate()?;
        if value.asset_index.artifact.size > assets::INDEX_LIMIT || value.libraries.is_empty() {
            return Err(Error::Invalid("asset index size or missing libraries"));
        }
        Ok(Self {
            client: Artifact::validate(value.downloads.client, CLIENT_HOSTS)?,
            index_id: value.asset_index.id,
            index: Artifact::validate(value.asset_index.artifact, METADATA_HOSTS)?,
            libraries: value.libraries,
            java_version: value.java_version,
        })
    }
}

impl Artifact {
    fn validate(value: ArtifactJson, hosts: &[&str]) -> Result<Self, Error> {
        if value.size == 0 {
            return Err(Error::Invalid("empty artifact"));
        }
        Ok(Self {
            sha1: parse_sha1(&value.sha1)?,
            size: value.size,
            url: trusted_url(&value.url, hosts)?,
        })
    }

    fn download(self, name: String, destination: std::path::PathBuf, https: &Https) -> Download {
        let https = https.clone();
        let url = self.url;
        Download {
            name: name.into(),
            destination,
            expected_size: Some(self.size),
            checksum: Some(Checksum::Sha1(self.sha1)),
            existing: ExistingFile::Preserve,
            open: Box::new(move || {
                let https = https.clone();
                let url = url.clone();
                async move { https.download(url).await }.boxed()
            }),
        }
    }
}

fn validate_id(id: &str) -> Result<(), Error> {
    if id.len() <= 32
        && id
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        Ok(())
    } else {
        Err(Error::Invalid("version or asset index ID"))
    }
}

fn parse_sha1(value: &str) -> Result<[u8; 20], Error> {
    let invalid = || Error::Invalid("SHA-1 digest");
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(invalid());
    }
    let mut hash = [0; 20];
    for (index, byte) in hash.iter_mut().enumerate() {
        *byte = u8::from_str_radix(&value[index * 2..index * 2 + 2], 16).map_err(|_| invalid())?;
    }
    Ok(hash)
}

fn hex_sha1(bytes: &[u8; 20]) -> String {
    use std::fmt::Write;
    let mut value = String::with_capacity(40);
    for byte in bytes {
        write!(value, "{byte:02x}").expect("writing to a String cannot fail");
    }
    value
}

fn safe_path(value: &str) -> Result<PathBuf, Error> {
    let valid = value.len() <= 1024
        && value.split('/').all(|part| {
            if part.is_empty()
                || part.len() > 255
                || part.ends_with('.')
                || !part.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'+' | b'@')
                })
            {
                return false;
            }
            let stem = part
                .split('.')
                .next()
                .unwrap_or_default()
                .to_ascii_uppercase();
            !matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                && !["COM", "LPT"].iter().any(|prefix| {
                    stem.strip_prefix(prefix).is_some_and(|digit| {
                        digit.len() == 1 && digit.as_bytes()[0].is_ascii_digit()
                    })
                })
        });
    if valid {
        Ok(Path::new(value).to_owned())
    } else {
        Err(Error::Invalid("unsafe artifact path"))
    }
}

fn trusted_url(value: &str, hosts: &[&str]) -> Result<Url, Error> {
    let invalid = || Error::Invalid("untrusted download URL");
    let url = Url::parse(value).map_err(|_| invalid())?;
    if url.scheme() == "https"
        && url.host_str().is_some_and(|host| hosts.contains(&host))
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
    {
        Ok(url)
    } else {
        Err(invalid())
    }
}

#[cfg(test)]
mod tests;
