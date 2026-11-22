//! Resolves the selected Mojang stable release into verified download tasks.
//!
//! The same verified metadata also drives the launch engine: [`prepare`]
//! re-derives launch inputs from the record written at install time, and
//! [`verify`] re-checks every file the game needs before it starts.

pub(crate) mod arguments;
mod assets;
mod cache;
mod java;
mod libraries;

pub use java::Runtime;
pub(crate) use java::configure as configure_java;
mod natives;
pub(crate) mod record;

pub(crate) use libraries::Platform;
pub(crate) use record::Record;

use crate::{
    download::{
        self, ArtifactStatus, Cancellation, Checksum, Download, ExistingFile, Failure, Progress,
        Reporter, Stage,
    },
    game_directory::{GameDirectory, is_link},
    http::{self, Https},
};
use arguments::Arguments;
use futures::FutureExt;
use reqwest::Url;
use serde::Deserialize;
use sha1::{Digest, Sha1};
use std::{
    fmt, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    time::Duration,
};

use libraries::Library;

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
    Record(io::ErrorKind),
    Https(http::Error),
    MissingRelease,
    /// No completed installation exists for the selected version.
    NotInstalled,
    /// A file the installation depends on is missing, unreadable or corrupt.
    Damaged(String),
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

    fn record(error: io::Error) -> Self {
        Self::Record(error.kind())
    }

    /// True when reinstalling the version can fix the problem.
    pub(crate) fn repairable(&self) -> bool {
        matches!(
            self,
            Self::NotInstalled
                | Self::Damaged(_)
                | Self::Record(_)
                | Self::Directory(_)
                | Self::Native(_)
                | Self::Java(_)
                | Self::Download(_)
        )
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
            Self::Record(kind) => {
                write!(f, "Could not access the installation record ({kind})")
            }
            Self::NotInstalled => f.write_str("This version is not installed yet"),
            Self::Damaged(reason) => write!(
                f,
                "Damaged installation ({reason}). Repair the installation to fix it."
            ),
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
    #[serde(rename = "mainClass")]
    main_class: String,
    #[serde(default)]
    arguments: Option<Arguments>,
    #[serde(default, rename = "minecraftArguments")]
    minecraft_arguments: Option<String>,
    #[serde(default)]
    logging: Option<Logging>,
}

/// The `logging.client` block: which log configuration to download and how to
/// pass it to the JVM.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Logging {
    client: LoggingClient,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LoggingClient {
    argument: String,
    file: LogConfigFile,
    r#type: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct LogConfigFile {
    id: String,
    sha1: String,
    size: u64,
    url: String,
}

/// Validated logging metadata ready for download and JVM substitution.
struct LoggingSpec {
    argument: String,
    id: String,
    artifact: Artifact,
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
    logging: Option<LoggingSpec>,
    /// Verified metadata bytes, persisted beside the install so a later launch
    /// can re-derive arguments from checksum-pinned input without the network.
    raw: Vec<u8>,
    sha1: [u8; 20],
}

struct InstallationPlan {
    index: Download,
    files: Vec<Download>,
    natives: Vec<natives::NativeArchive>,
    version_dir: PathBuf,
    platform: Platform,
    java_version: java::Requirement,
    metadata: Vec<u8>,
    metadata_sha1: [u8; 20],
    /// Logging configuration id recorded after a successful install.
    logging: Option<String>,
    repair: bool,
}

impl Mojang {
    pub(crate) async fn install(
        &self,
        selected: &str,
        repair: bool,
        cancel: &Cancellation,
        report: &Reporter,
    ) -> Result<Runtime, Error> {
        cancel.check()?;
        let directory = GameDirectory::discover().map_err(Error::directory)?;
        self.install_in(selected, &directory, repair, cancel, report)
            .await
    }

    async fn install_in(
        &self,
        selected: &str,
        directory: &GameDirectory,
        repair: bool,
        cancel: &Cancellation,
        report: &Reporter,
    ) -> Result<Runtime, Error> {
        let _guard = directory.lock_operation().map_err(Error::directory)?;
        let mut plan = cancel
            .wait(self.resolve_in(selected, directory, cancel))
            .await??;
        plan.repair = repair;
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
        let mut archives: Vec<(String, Vec<PathBuf>)> = Vec::with_capacity(plan.natives.len());
        let index_size = plan
            .index
            .expected_size
            .ok_or(Error::Invalid("missing asset index size"))?;
        if plan.repair {
            plan.index.existing = ExistingFile::ReplaceInvalid;
        }
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
        if plan.repair {
            for artifact in &mut plan.files {
                artifact.existing = ExistingFile::ReplaceInvalid;
            }
        }
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
            let extracted = natives::extract(native, &plan.version_dir, plan.platform, cancel)?;
            archives.push((hex_sha1(&native.sha1), extracted));
        }
        // Merge every verified archive into the single directory the game's
        // `natives_directory` argument points at. This runs at install time so
        // launch only has to check that the recorded files are present.
        let assembled = natives::assemble(&plan.version_dir, &archives, cancel)?;
        let runtime =
            java::resolve(&plan.java_version, directory, &self.https, cancel, report).await?;
        let record = Record {
            schema: record::SCHEMA,
            version: selected.to_owned(),
            metadata_sha1: hex_sha1(&plan.metadata_sha1),
            runtime: runtime.clone(),
            natives: assembled
                .into_iter()
                .map(|(path, size, sha1)| {
                    Ok::<record::NativeEntry, Error>(record::NativeEntry {
                        path: path
                            .to_str()
                            .ok_or(Error::Invalid("native output path"))?
                            .replace(std::path::MAIN_SEPARATOR, "/"),
                        size,
                        sha1: hex_sha1(&sha1),
                    })
                })
                .collect::<Result<Vec<_>, _>>()?,
            log_config: plan.logging,
        };
        cancel.check()?;
        record::write(&plan.version_dir, selected, &plan.metadata, &record)?;
        Ok(runtime)
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
        let logging = version.logging;
        let logging_id = logging.as_ref().map(|spec| spec.id.clone());
        if let Some(spec) = logging {
            files.push(spec.artifact.download(
                format!("Minecraft {selected} logging configuration"),
                directory.log_config(&spec.id),
                &self.https,
            ));
        }
        Ok(InstallationPlan {
            index,
            files,
            natives: libraries.natives,
            version_dir,
            platform,
            java_version: version.java_version,
            metadata: version.raw,
            metadata_sha1: version.sha1,
            logging: logging_id,
            repair: false,
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
        let value = parse_version_json(bytes, selected, sha1)?;
        let logging = value.logging.map(logging_spec).transpose()?;
        Ok(Self {
            client: Artifact::validate(value.downloads.client, CLIENT_HOSTS)?,
            index_id: value.asset_index.id,
            index: Artifact::validate(value.asset_index.artifact, METADATA_HOSTS)?,
            libraries: value.libraries,
            java_version: value.java_version,
            logging,
            raw: bytes.to_vec(),
            sha1,
        })
    }
}

/// Verifies and bounds the launch-relevant parts of a version document. Used
/// both at install time and when the persisted metadata is re-read for a
/// launch, so either path rejects the same unsupported structures.
fn parse_version_json(bytes: &[u8], selected: &str, sha1: [u8; 20]) -> Result<VersionJson, Error> {
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
    validate_main_class(&value.main_class)?;
    match (&value.arguments, &value.minecraft_arguments) {
        (Some(arguments), _) => {
            arguments
                .validate()
                .map_err(Error::Invalid)
                .and_then(|()| {
                    if arguments.has_game() || value.minecraft_arguments.is_some() {
                        Ok(())
                    } else {
                        Err(Error::Invalid("missing game arguments"))
                    }
                })?;
        }
        (None, Some(legacy)) => validate_legacy_arguments(legacy)?,
        (None, None) => return Err(Error::Invalid("missing launch arguments")),
    }
    if let Some(legacy) = &value.minecraft_arguments {
        validate_legacy_arguments(legacy)?;
    }
    if let Some(logging) = &value.logging {
        validate_logging(logging)?;
    }
    Ok(value)
}

fn validate_main_class(value: &str) -> Result<(), Error> {
    let valid = !value.is_empty()
        && value.len() <= 512
        && value.contains('.')
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'$'));
    if valid {
        Ok(())
    } else {
        Err(Error::Invalid("main class"))
    }
}

/// Bounds the legacy `minecraftArguments` string. Tokens are split before
/// substitution, so values containing spaces survive; quoting is not
/// supported because no shipped release uses it.
fn validate_legacy_arguments(value: &str) -> Result<(), Error> {
    if value.trim().is_empty()
        || value.contains('\0')
        || value.len() > 8192
        || value.contains('"')
        || value.contains('\'')
        || value.split_whitespace().count() > 512
    {
        return Err(Error::Invalid("legacy game arguments"));
    }
    Ok(())
}

fn validate_logging(logging: &Logging) -> Result<(), Error> {
    let client = &logging.client;
    if client.r#type.len() > 32 || !client.r#type.starts_with("log4j2") {
        return Err(Error::Invalid("logging configuration type"));
    }
    if client.argument != "-Dlog4j.configurationFile=${path}" {
        return Err(Error::Invalid("logging configuration argument"));
    }
    let file = &client.file;
    if !file.id.ends_with(".xml")
        || file.id.len() > 64
        || !file
            .id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'-' | b'_'))
        || file.size == 0
        || file.size > 1024 * 1024
    {
        return Err(Error::Invalid("logging configuration file"));
    }
    parse_sha1(&file.sha1)?;
    trusted_url(&file.url, CLIENT_HOSTS)?;
    Ok(())
}

fn logging_spec(logging: Logging) -> Result<LoggingSpec, Error> {
    let file = logging.client.file;
    Ok(LoggingSpec {
        argument: logging.client.argument,
        id: file.id,
        artifact: Artifact {
            sha1: parse_sha1(&file.sha1)?,
            size: file.size,
            url: trusted_url(&file.url, CLIENT_HOSTS)?,
        },
    })
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

/// A file whose size and SHA-1 come from verified metadata or the record.
pub(crate) struct CheckedFile {
    pub path: PathBuf,
    pub size: u64,
    pub sha1: [u8; 20],
}

/// The logging configuration argument template (`${path}`) plus its file.
pub(crate) struct LogConfig {
    pub argument: String,
    pub file: CheckedFile,
}

/// Everything a launch needs, re-derived from the persisted record and the
/// checksum-pinned version metadata. Building it performs no network access
/// and writes nothing.
pub(crate) struct Prepared {
    pub id: String,
    /// Version type reported as `--versionType`, e.g. `release`.
    pub kind: String,
    pub main_class: String,
    /// Modern `arguments` sections; empty when only `minecraftArguments` exists.
    pub arguments: Arguments,
    /// Legacy `minecraftArguments` string for pre-1.13 versions.
    pub legacy: Option<String>,
    /// Java runtime recorded at install time, re-probed before launch.
    pub java: Runtime,
    pub game_directory: PathBuf,
    pub assets_root: PathBuf,
    pub asset_index_id: String,
    pub natives_directory: PathBuf,
    /// Assembled native binaries with the size and digest the record pinned.
    pub natives: Vec<CheckedFile>,
    /// Client JAR first, then every allowed library, in metadata order.
    pub classpath: Vec<PathBuf>,
    pub libraries: Vec<CheckedFile>,
    pub client: CheckedFile,
    pub asset_index: CheckedFile,
    pub asset_objects: PathBuf,
    pub log_config: Option<LogConfig>,
}

/// Rebuilds the launch inputs for an installed version from its record.
pub(crate) fn prepare(directory: &GameDirectory, id: &str) -> Result<Prepared, Error> {
    validate_id(id)?;
    let version_dir = directory.version_path(id);
    match version_dir.symlink_metadata() {
        Ok(metadata) if metadata.is_dir() && !is_link(&metadata) => {}
        Ok(_) => return Err(Error::Damaged("version directory".into())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(Error::NotInstalled),
        Err(error) => return Err(Error::directory(error)),
    }
    let record = record::load(&version_dir)
        .map_err(as_record_damage)?
        .ok_or(Error::NotInstalled)?;
    if record.version != id {
        return Err(Error::Damaged("installation record version".into()));
    }
    let metadata = record::metadata(&version_dir, id, &record).map_err(as_record_damage)?;
    let value = parse_version_json(&metadata, id, parse_sha1(&record.metadata_sha1)?)
        .map_err(as_record_damage)?;
    let logging = value.logging.map(logging_spec).transpose()?;
    let platform = Platform::current()?;
    let libraries = libraries::classpath(value.libraries, platform, &directory.libraries())?;
    if record.runtime.major != value.java_version.major {
        return Err(Error::Damaged("recorded Java major version".into()));
    }
    let client_path = version_dir.join(format!("{id}.jar"));
    let client = CheckedFile {
        path: client_path.clone(),
        size: value.downloads.client.size,
        sha1: parse_sha1(&value.downloads.client.sha1)?,
    };
    let asset_index = CheckedFile {
        path: directory.asset_index(&value.asset_index.id),
        size: value.asset_index.artifact.size,
        sha1: parse_sha1(&value.asset_index.artifact.sha1)?,
    };
    let log_config = match (record.log_config, logging) {
        (Some(log_id), Some(spec)) if spec.id == log_id => Some(LogConfig {
            argument: spec.argument,
            file: CheckedFile {
                path: directory.log_config(&log_id),
                size: spec.artifact.size,
                sha1: spec.artifact.sha1,
            },
        }),
        (None, None) => None,
        _ => return Err(Error::Damaged("logging configuration record".into())),
    };
    let classpath = std::iter::once(client_path.clone())
        .chain(libraries.iter().map(|file| file.path.clone()))
        .collect();
    Ok(Prepared {
        id: id.to_owned(),
        kind: value.kind,
        main_class: value.main_class,
        arguments: value.arguments.unwrap_or_default(),
        legacy: value.minecraft_arguments,
        java: record.runtime,
        game_directory: directory.root().to_owned(),
        assets_root: directory.root().join("assets"),
        asset_index_id: value.asset_index.id,
        natives_directory: version_dir.join("natives").join("launch"),
        natives: record
            .natives
            .iter()
            .map(|entry| {
                Ok(CheckedFile {
                    path: version_dir.join(&entry.path),
                    size: entry.size,
                    sha1: parse_sha1(&entry.sha1)?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?,
        classpath,
        libraries,
        client,
        asset_index,
        asset_objects: directory.asset_objects().map_err(Error::directory)?,
        log_config,
    })
}

fn as_record_damage(error: Error) -> Error {
    match error {
        Error::Invalid(reason) => Error::Damaged(reason.into()),
        error => error,
    }
}

pub(crate) fn has_installation_record(directory: &GameDirectory, id: &str) -> bool {
    match directory
        .version_path(id)
        .join(record::FILE)
        .symlink_metadata()
    {
        Ok(_) => true,
        Err(error) => error.kind() != io::ErrorKind::NotFound,
    }
}

/// Full pre-launch validation: every file the game will read must exist with
/// the recorded size, the client JAR and asset index must match their SHA-1,
/// the assembled natives must be present and the asset index must resolve to
/// complete objects. Failures name the damaged component so the UI can offer
/// a repair install.
pub(crate) fn verify(prepared: &Prepared, cancel: &Cancellation) -> Result<(), Error> {
    verify_checked(&prepared.client, cancel)?;
    for library in &prepared.libraries {
        cancel.check()?;
        verify_managed_directories(&prepared.game_directory, library.path.parent())?;
        verify_checked(library, cancel)?;
    }
    verify_managed_directories(&prepared.game_directory, Some(&prepared.natives_directory))?;
    for native in &prepared.natives {
        verify_managed_directories(&prepared.natives_directory, native.path.parent())?;
        verify_checked(native, cancel)?;
    }
    verify_checked(&prepared.asset_index, cancel)?;
    assets::validate(&prepared.asset_index.path, &prepared.asset_objects, cancel)?;
    if let Some(log) = &prepared.log_config {
        verify_checked(&log.file, cancel)?;
    }
    Ok(())
}

fn verify_managed_directories(root: &Path, mut parent: Option<&Path>) -> Result<(), Error> {
    while let Some(path) = parent {
        if path == root {
            return Ok(());
        }
        if !path
            .symlink_metadata()
            .is_ok_and(|metadata| metadata.is_dir() && !is_link(&metadata))
        {
            return Err(Error::Damaged(format!(
                "invalid {} directory",
                display_name(path)
            )));
        }
        parent = path.parent();
    }
    Err(Error::Invalid(
        "launch file is outside its managed directory",
    ))
}

fn verify_checked(file: &CheckedFile, cancel: &Cancellation) -> Result<(), Error> {
    let name = display_name(&file.path);
    let Some(metadata) = regular_file(&file.path)? else {
        return Err(Error::Damaged(format!("missing {name}")));
    };
    if metadata.len() != file.size {
        return Err(Error::Damaged(format!("{name} has an unexpected size")));
    }
    let mut handle = fs::File::open(&file.path).map_err(Error::directory)?;
    let mut hasher = Sha1::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        cancel.check()?;
        let count = handle.read(&mut buffer).map_err(|error| {
            Error::Damaged(format!("{name} could not be read ({})", error.kind()))
        })?;
        if count == 0 {
            break;
        }
        hasher.update(&buffer[..count]);
    }
    if hasher.finalize()[..] != file.sha1 {
        return Err(Error::Damaged(format!(
            "{name} failed checksum verification"
        )));
    }
    Ok(())
}

fn regular_file(path: &Path) -> Result<Option<fs::Metadata>, Error> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_file() && !is_link(&metadata) => Ok(Some(metadata)),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::directory(error)),
    }
}

fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.display().to_string())
}

/// Probes the recorded Java executable for the required major version.
pub(crate) async fn probe_java(
    executable: PathBuf,
    major: u16,
    cancel: &Cancellation,
) -> Result<Option<Runtime>, Failure> {
    java::probe(executable, major, cancel).await
}

/// Searches for a compatible system Java when the recorded one is gone.
pub(crate) async fn probe_system_java(
    major: u16,
    managed_root: &Path,
    cancel: &Cancellation,
) -> Result<Option<Runtime>, Failure> {
    java::probe_system(major, managed_root, cancel).await
}

#[cfg(test)]
mod tests;
