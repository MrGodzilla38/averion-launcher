mod detect;
pub(crate) use detect::configure;
mod package;

use super::{
    Artifact, ArtifactJson, Error, Https, METADATA_HOSTS, cache, hex_sha1, parse_sha1, trusted_url,
};
use crate::{
    download::{Cancellation, Failure, Progress, Reporter, Stage},
    game_directory::{GameDirectory, checked_subdirectory, is_link},
};
use serde::Deserialize;
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    time::Duration,
};

const CATALOG: &str = "https://piston-meta.mojang.com/v1/products/java-runtime/2ec0cc96c44e5a76b9c8b7c39df7210883d12871/all.json";
const CATALOG_LIMIT: usize = 1024 * 1024;
const PACKAGE_LIMIT: usize = 2 * 1024 * 1024;
const CATALOG_AGE: Duration = Duration::from_secs(24 * 60 * 60);

#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Runtime {
    pub executable: PathBuf,
    pub major: u16,
}

#[derive(Clone, Deserialize)]
pub(super) struct Requirement {
    pub component: String,
    #[serde(rename = "majorVersion")]
    pub major: u16,
}

impl Requirement {
    pub fn validate(&self) -> Result<(), Error> {
        if self.major < 8
            || self.component.is_empty()
            || self.component.len() > 64
            || !self
                .component
                .bytes()
                .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        {
            Err(Error::Invalid("Java runtime requirement"))
        } else {
            Ok(())
        }
    }
}

type Catalog = BTreeMap<String, BTreeMap<String, Vec<Release>>>;

#[derive(Deserialize)]
struct Release {
    availability: Availability,
    manifest: ArtifactJson,
    version: Version,
}

#[derive(Deserialize)]
struct Availability {
    progress: u8,
}

#[derive(Deserialize)]
struct Version {
    name: String,
}

fn select_catalog(
    bytes: &[u8],
    platform: &str,
    requirement: &Requirement,
) -> Result<Artifact, Error> {
    let catalog: Catalog =
        serde_json::from_slice(bytes).map_err(|_| Error::Java("invalid Mojang runtime catalog"))?;
    let release = catalog
        .get(platform)
        .and_then(|platform| platform.get(&requirement.component))
        .and_then(|releases| {
            releases.iter().find(|release| {
                release.availability.progress == 100
                    && detect::version_major(&release.version.name) == Some(requirement.major)
            })
        })
        .ok_or(Error::Java(
            "Mojang has no matching runtime for this platform",
        ))?;
    if release.manifest.size > PACKAGE_LIMIT as u64 {
        return Err(Error::Java("runtime manifest size limit"));
    }
    Artifact::validate(
        ArtifactJson {
            sha1: release.manifest.sha1.clone(),
            size: release.manifest.size,
            url: release.manifest.url.clone(),
        },
        METADATA_HOSTS,
    )
}

fn platform() -> Result<&'static str, Error> {
    if std::env::consts::ARCH != "x86_64" {
        return Err(Error::Java("unsupported runtime architecture"));
    }
    match std::env::consts::OS {
        "linux" => Ok("linux"),
        "windows" => Ok("windows-x64"),
        _ => Err(Error::Java("unsupported runtime platform")),
    }
}

pub(super) async fn resolve(
    requirement: &Requirement,
    directory: &GameDirectory,
    https: &Https,
    cancel: &Cancellation,
    report: &Reporter,
) -> Result<Runtime, Error> {
    status(requirement, Stage::Checking, cancel, report).await?;
    if let Some(runtime) = detect::system_java(requirement.major, &directory.java(), cancel).await?
    {
        return Ok(runtime);
    }
    managed(requirement, directory, https, cancel, report).await
}

/// Probes a specific executable for the required Java major version. Used by
/// the launch engine to re-validate the runtime recorded at install time.
pub(super) async fn probe(
    executable: PathBuf,
    major: u16,
    cancel: &Cancellation,
) -> Result<Option<Runtime>, Failure> {
    detect::compatible(executable, major, cancel).await
}

/// Looks for a compatible system Java outside the managed runtime directory.
pub(super) async fn probe_system(
    major: u16,
    managed_root: &Path,
    cancel: &Cancellation,
) -> Result<Option<Runtime>, Failure> {
    detect::system_java(major, managed_root, cancel).await
}

async fn managed(
    requirement: &Requirement,
    directory: &GameDirectory,
    https: &Https,
    cancel: &Cancellation,
    report: &Reporter,
) -> Result<Runtime, Error> {
    let platform = platform()?;
    let root = managed_root(directory, requirement.major, platform)?;
    let cache_dir = directory.metadata_cache();
    if let Some(runtime) = reuse(&root, &cache_dir, requirement.major, platform, cancel).await? {
        return Ok(runtime);
    }

    status(requirement, Stage::Resolving, cancel, report).await?;
    let url = trusted_url(CATALOG, METADATA_HOSTS)?;
    let manifest = cancel
        .wait(cache::load(
            &cache_dir.join("java-runtime-catalog.cache"),
            Some(CATALOG_AGE),
            CATALOG_LIMIT,
            |bytes| select_catalog(bytes, platform, requirement),
            https.metadata(url, CATALOG_LIMIT),
        ))
        .await??;
    let manifest_hash = hex_sha1(&manifest.sha1);
    let package = cancel
        .wait(cache::load(
            &cache_file(&cache_dir, &manifest_hash),
            None,
            PACKAGE_LIMIT,
            |bytes| package::Package::parse(bytes, &manifest.sha1, Some(manifest.size), platform),
            https.metadata(manifest.url.clone(), PACKAGE_LIMIT),
        ))
        .await??;
    let slot = checked_subdirectory(&root, Path::new(&manifest_hash)).map_err(Error::directory)?;
    let executable = package
        .install(&slot, requirement.major, https, cancel, report)
        .await?;
    detect::compatible(executable, requirement.major, cancel)
        .await?
        .ok_or(Error::Java(
            "the verified runtime failed its compatibility probe",
        ))
}

fn managed_root(directory: &GameDirectory, major: u16, platform: &str) -> Result<PathBuf, Error> {
    let relative = PathBuf::from(major.to_string()).join(platform);
    checked_subdirectory(&directory.java(), &relative).map_err(Error::directory)
}

fn cache_file(cache: &Path, hash: &str) -> PathBuf {
    cache.join(format!("java-package-{hash}.cache"))
}

async fn reuse(
    root: &Path,
    cache_dir: &Path,
    major: u16,
    platform: &str,
    cancel: &Cancellation,
) -> Result<Option<Runtime>, Error> {
    let read = fs::read_dir(root).map_err(Error::directory)?;
    let mut slots = Vec::new();
    for entry in read {
        cancel.check()?;
        let entry = entry.map_err(Error::directory)?;
        let metadata = fs::symlink_metadata(entry.path()).map_err(Error::directory)?;
        if metadata.is_dir() && !is_link(&metadata) {
            slots.push(entry.path());
        }
    }
    slots.sort();
    for slot in slots {
        cancel.check()?;
        let Some(name) = slot.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        let Ok(hash) = parse_sha1(name) else { continue };
        let Ok(Some(bytes)) = cache::read(&cache_file(cache_dir, name), None, PACKAGE_LIMIT) else {
            continue;
        };
        let Ok(package) = package::Package::parse(&bytes, &hash, None, platform) else {
            continue;
        };
        let executable = match package.ready(&slot, cancel) {
            Ok(Some(path)) => path,
            Err(Error::Download(error)) if error.failure == Failure::Cancelled => {
                return Err(Error::Download(error));
            }
            _ => continue,
        };
        if let Some(runtime) = detect::compatible(executable, major, cancel).await? {
            return Ok(Some(runtime));
        }
    }
    Ok(None)
}

async fn status(
    requirement: &Requirement,
    stage: Stage,
    cancel: &Cancellation,
    report: &Reporter,
) -> Result<(), Error> {
    cancel
        .wait(report(Progress {
            component: format!("Java {} runtime", requirement.major).into(),
            step: 0,
            count: 0,
            stage,
            overall: None,
        }))
        .await?;
    Ok(())
}

#[cfg(test)]
mod tests;
